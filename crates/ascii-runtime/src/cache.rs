//! Content-addressed derived caching, explicit resource leases, and transactional disk output.
//!
//! A source digest is the reusable identity. Paths and stat metadata are kept separately as
//! revalidation hints and never participate in cache-key equality. `CacheLease::acknowledge` is an
//! explicit owner boundary: lock failures leave the lease available for retry, and dropping a
//! lease does not imply decoder/GPU completion or release its entry pin.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::hash::{Hash, Hasher};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};

const CACHE_INSTANCE_DIR: &str = ".asciiwall-cache-v1";
const CACHE_OWNER_FILE: &str = ".asciiwall-owner";
const CACHE_OWNER_MARKER: &[u8] = b"AsciiWall derived cache ownership v1\n";
const CACHE_OPERATIONS_LOCK: &str = ".operations.lock";
const CACHE_FILE_ID_HIGH_WATER: &str = ".file-id-high-water";
const CACHE_FILE_ID_HIGH_WATER_BACKUP: &str = ".file-id-high-water.backup";
const CACHE_FILE_ID_HIGH_WATER_MARKER: &str = ".file-id-high-water-v1";

const CACHE_INITIALIZATION_LOCK: &str = ".asciiwall-cache-v1.init.lock";

// Bound managed-file metadata independently of payload size so zero-byte entries also remain
// bounded while preserving their validity under a zero-byte payload budget.
const MAX_CACHE_FILES: usize = 1024;

static CACHE_INITIALIZATION_TEMP_ID: AtomicU64 = AtomicU64::new(0);
static CACHE_FILE_ID_HIGH_WATER_TEMP_ID: AtomicU64 = AtomicU64::new(0);
/// Retains one shared file lock per path until every issued lease is explicitly acknowledged.
static CACHE_LEASE_PINS: LazyLock<Mutex<HashMap<PathBuf, CacheLeasePin>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Serializes operations for independently opened cache instances in this process.
static CACHE_ROOT_OPERATION_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_root_operation_lock(root: &Path) -> Arc<Mutex<()>> {
    let mut locks = lock_unpoisoned(&CACHE_ROOT_OPERATION_LOCKS);
    if let Some(lock) = locks.get(root).and_then(Weak::upgrade) {
        return lock;
    }

    locks.retain(|_, lock| lock.strong_count() != 0);
    let lock = Arc::new(Mutex::new(()));
    locks.insert(root.to_path_buf(), Arc::downgrade(&lock));
    lock
}

/// A caller-computed cryptographic digest of content or shader bytes.
///
/// Hashing is deliberately provided by the source/renderer owner; this portable crate has no
/// cryptographic dependency and never substitutes path/stat data for a digest.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContentDigest(pub [u8; 32]);

/// Source content state used in semantic identities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceContent {
    /// The source has been read/validated and this digest identifies its bytes.
    Digest(ContentDigest),
    /// The source is explicitly known to be missing; this is distinct from every content digest.
    Missing,
}

/// Non-identity metadata useful for deciding when a source digest should be refreshed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceStatHint {
    pub length_bytes: u64,
    /// Nanoseconds since the Unix epoch when available from the host filesystem.
    pub modified_unix_ns: Option<i128>,
    /// Opaque platform file identity, if available. It is a hint and is not portable identity.
    pub file_id: Option<Vec<u8>>,
}

/// An observed source, kept separate from conversion keys so path/stat values cannot enter key
/// equality accidentally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceObservation {
    pub path: PathBuf,
    pub stat_hint: Option<SourceStatHint>,
    pub content: SourceContent,
}

impl SourceObservation {
    pub fn available(
        path: impl Into<PathBuf>,
        stat_hint: Option<SourceStatHint>,
        digest: ContentDigest,
    ) -> Self {
        Self {
            path: path.into(),
            stat_hint,
            content: SourceContent::Digest(digest),
        }
    }

    pub fn missing(path: impl Into<PathBuf>, stat_hint: Option<SourceStatHint>) -> Self {
        Self {
            path: path.into(),
            stat_hint,
            content: SourceContent::Missing,
        }
    }

    pub fn identity(&self) -> SourceContent {
        self.content
    }

    /// A changed hint for an otherwise unchanged available source requires content revalidation.
    /// A changed digest already supplies the new identity; an explicit missing state is also
    /// conclusive and does not require another revalidation.
    pub fn requires_content_revalidation_from(&self, previous: &Self) -> bool {
        matches!(self.content, SourceContent::Digest(_))
            && self.content == previous.content
            && (self.path != previous.path || self.stat_hint != previous.stat_hint)
    }
}

/// Raster parameters that can affect font-to-cell sampling.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FontRasterSettings {
    pub size_millipoints: u32,
    pub horizontal_scale_milli: u16,
    pub vertical_scale_milli: u16,
    pub hinting: FontHinting,
    pub antialias: bool,
    pub subpixel: bool,
    /// Additional named integral raster parameters owned by the font/raster pipeline.
    pub extra_parameters: Vec<(String, i64)>,
}

impl Default for FontRasterSettings {
    fn default() -> Self {
        Self {
            size_millipoints: 12_000,
            horizontal_scale_milli: 1_000,
            vertical_scale_milli: 1_000,
            hinting: FontHinting::Default,
            antialias: true,
            subpixel: false,
            extra_parameters: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FontHinting {
    None,
    Light,
    Full,
    Default,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GridSize {
    pub columns: u32,
    pub rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FitMode {
    Fit,
    Crop,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Fit/crop mode and crop coordinates, when present.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FitCrop {
    pub mode: FitMode,
    pub crop: Option<CropRect>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CellGeometry {
    pub cell_width_milli_px: u32,
    pub cell_height_milli_px: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PatchDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgba8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// All semantic inputs to a conversion cache entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ConversionKey {
    pub source: SourceContent,
    pub decode_version: String,
    pub orientation_version: String,
    pub color_version: String,
    pub algorithm_version: String,
    pub font_digest: ContentDigest,
    pub font_raster_settings: FontRasterSettings,
    /// Ordered glyphs: changing membership or order changes identity.
    pub glyphs: Vec<String>,
    pub grid: GridSize,
    pub fit_crop: FitCrop,
    /// Stable sampling digest supplied by the sampling pipeline (fit/crop, filter, grid,
    /// cell geometry, and optional patch dimensions).
    pub sampling_identity_digest: ContentDigest,
    /// Explicit components make sampling-setting changes visible even before digest adoption.
    pub sampling_filter: String,
    pub cell_geometry: CellGeometry,
    pub patch_dimensions: Option<PatchDimensions>,
    pub palette: Vec<Rgba8>,
    pub settings_digest: ContentDigest,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rational {
    numerator: u32,
    denominator: u32,
}

impl Rational {
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, IdentityError> {
        if numerator == 0 || denominator == 0 {
            return Err(IdentityError::ZeroRationalComponent);
        }
        let divisor = greatest_common_divisor(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub fn numerator(self) -> u32 {
        self.numerator
    }

    pub fn denominator(self) -> u32 {
        self.denominator
    }
}

fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoStreamSelection {
    pub stream_index: u32,
    pub stream_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VideoTimeRange {
    pub start_ns: i64,
    pub end_ns: i64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoCacheKey {
    pub conversion: ConversionKey,
    pub stream: VideoStreamSelection,
    pub time_range: VideoTimeRange,
    pub fps: Rational,
    pub temporal_algorithm_version: String,
    pub seed: u64,
}

/// The concrete values consumed by the GPU rendering stage.
///
/// Floating-point settings compare and hash by their IEEE bit patterns so cache identity matches
/// the values supplied to the shader.
#[derive(Clone, Copy, Debug)]
pub struct GpuUniformSettings {
    pub foreground: Rgba8,
    pub background: Rgba8,
    pub secondary: Rgba8,
    pub opacity: f32,
    pub glitch_enabled: bool,
    pub glitch_strength: f32,
    pub scanlines_enabled: bool,
    pub scanlines_strength: f32,
    pub bloom_enabled: bool,
    pub bloom_strength: f32,
    pub trails_enabled: bool,
    pub trails_decay: f32,
    pub vignette: f32,
    pub noise: f32,
    pub interaction_pointer_radius_cells: f32,
    pub interaction_pointer_strength: f32,
    pub interaction_pointer_max_offset_cells: f32,
    pub interaction_click_radius_cells: f32,
    pub interaction_click_strength: f32,
    pub interaction_window_margin_cells: f32,
    pub interaction_window_strength: f32,
}

impl PartialEq for GpuUniformSettings {
    fn eq(&self, other: &Self) -> bool {
        self.foreground == other.foreground
            && self.background == other.background
            && self.secondary == other.secondary
            && self.opacity.to_bits() == other.opacity.to_bits()
            && self.glitch_enabled == other.glitch_enabled
            && self.glitch_strength.to_bits() == other.glitch_strength.to_bits()
            && self.scanlines_enabled == other.scanlines_enabled
            && self.scanlines_strength.to_bits() == other.scanlines_strength.to_bits()
            && self.bloom_enabled == other.bloom_enabled
            && self.bloom_strength.to_bits() == other.bloom_strength.to_bits()
            && self.trails_enabled == other.trails_enabled
            && self.trails_decay.to_bits() == other.trails_decay.to_bits()
            && self.vignette.to_bits() == other.vignette.to_bits()
            && self.noise.to_bits() == other.noise.to_bits()
            && self.interaction_pointer_radius_cells.to_bits()
                == other.interaction_pointer_radius_cells.to_bits()
            && self.interaction_pointer_strength.to_bits()
                == other.interaction_pointer_strength.to_bits()
            && self.interaction_pointer_max_offset_cells.to_bits()
                == other.interaction_pointer_max_offset_cells.to_bits()
            && self.interaction_click_radius_cells.to_bits()
                == other.interaction_click_radius_cells.to_bits()
            && self.interaction_click_strength.to_bits()
                == other.interaction_click_strength.to_bits()
            && self.interaction_window_margin_cells.to_bits()
                == other.interaction_window_margin_cells.to_bits()
            && self.interaction_window_strength.to_bits()
                == other.interaction_window_strength.to_bits()
    }
}

impl Eq for GpuUniformSettings {}

impl Hash for GpuUniformSettings {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.foreground.hash(state);
        self.background.hash(state);
        self.secondary.hash(state);
        self.opacity.to_bits().hash(state);
        self.glitch_enabled.hash(state);
        self.glitch_strength.to_bits().hash(state);
        self.scanlines_enabled.hash(state);
        self.scanlines_strength.to_bits().hash(state);
        self.bloom_enabled.hash(state);
        self.bloom_strength.to_bits().hash(state);
        self.trails_enabled.hash(state);
        self.trails_decay.to_bits().hash(state);
        self.vignette.to_bits().hash(state);
        self.noise.to_bits().hash(state);
        self.interaction_pointer_radius_cells.to_bits().hash(state);
        self.interaction_pointer_strength.to_bits().hash(state);
        self.interaction_pointer_max_offset_cells
            .to_bits()
            .hash(state);
        self.interaction_click_radius_cells.to_bits().hash(state);
        self.interaction_click_strength.to_bits().hash(state);
        self.interaction_window_margin_cells.to_bits().hash(state);
        self.interaction_window_strength.to_bits().hash(state);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GpuCacheKey {
    pub upstream: Box<CacheIdentity>,
    pub backend: String,
    pub device: String,
    pub shader_digest: ContentDigest,
    /// Exact settings sent to the GPU stage, rather than a caller-reusable digest.
    pub uniforms: GpuUniformSettings,
}

/// Namespaced content identity for conversion, video, or GPU-derived artifacts.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CacheIdentity {
    Conversion(ConversionKey),
    Video(VideoCacheKey),
    Gpu(GpuCacheKey),
}

impl CacheIdentity {
    /// Returns the source state included in this identity, including for nested video/GPU keys.
    pub fn source_content(&self) -> SourceContent {
        match self {
            Self::Conversion(key) => key.source,
            Self::Video(key) => key.conversion.source,
            Self::Gpu(key) => key.upstream.source_content(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    ZeroRationalComponent,
}

impl core::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroRationalComponent => {
                f.write_str("rational numerator and denominator must be non-zero")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

#[derive(Debug)]
pub enum CacheError {
    Io(io::Error),
    EntryExceedsBudget {
        entry_bytes: u64,
        budget_bytes: u64,
    },
    BudgetPinned {
        used_bytes: u64,
        budget_bytes: u64,
        requested_bytes: u64,
    },
    FileCountPinned {
        /// Managed files remaining after attempting eligible LRU evictions.
        current_files: usize,
        /// Per-root managed-file cap.
        max_files: usize,
        /// Additional files needed by the pending insert, or zero during trimming.
        requested_files: usize,
    },
    NotFound,
    UnsafeDerivedPath(PathBuf),
    InvalidLease,
    SourceIdentityMismatch,
    SourceObservationUnavailable,
    SourceObservationRequired,
    SourceRevalidationRequired,
    ArithmeticOverflow,
}

impl core::fmt::Display for CacheError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cache I/O failed: {error}"),
            Self::EntryExceedsBudget {
                entry_bytes,
                budget_bytes,
            } => {
                write!(
                    f,
                    "entry of {entry_bytes} bytes exceeds cache budget of {budget_bytes} bytes"
                )
            }
            Self::BudgetPinned {
                used_bytes,
                budget_bytes,
                requested_bytes,
            } => write!(
                f,
                "cache budget {budget_bytes} bytes cannot fit {requested_bytes} requested bytes while {used_bytes} bytes are pinned"
            ),
            Self::FileCountPinned {
                current_files,
                max_files,
                requested_files,
            } => {
                if *requested_files == 0 {
                    write!(
                        f,
                        "cache file-count limit {max_files} cannot be met: \
                         {current_files} managed files remain; exceeding the limit because files are \
                         pinned or locked"
                    )
                } else {
                    write!(
                        f,
                        "cache file-count limit {max_files} blocks {requested_files} additional {} \
                         while {current_files} managed files are pinned or locked",
                        if *requested_files == 1 {
                            "file"
                        } else {
                            "files"
                        }
                    )
                }
            }
            Self::NotFound => f.write_str("cache identity is not present"),
            Self::UnsafeDerivedPath(path) => write!(
                f,
                "refusing to delete unmanaged cache path {}",
                path.display()
            ),
            Self::InvalidLease => {
                f.write_str("cache lease no longer identifies an active resource")
            }
            Self::SourceIdentityMismatch => {
                f.write_str("source observation does not match cache identity")
            }
            Self::SourceObservationRequired => {
                f.write_str("current source observation is required before cache reuse")
            }
            Self::SourceObservationUnavailable => {
                f.write_str("cache entry has no source observation for revalidation")
            }
            Self::SourceRevalidationRequired => {
                f.write_str("source stat/path hint changed; revalidate content before cache reuse")
            }
            Self::ArithmeticOverflow => f.write_str("cache byte or LRU arithmetic overflowed"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for CacheError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOutcome {
    Inserted,
    AlreadyPresent,
}

/// Current cache accounting reconciled against all generated files beneath the shared root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheSnapshot {
    /// Byte limit configured on this cache instance.
    pub budget_bytes: u64,
    /// Bytes occupied by all managed generated files, including files unknown to this instance.
    pub used_bytes: u64,
    /// Semantic identities registered in this instance; unknown files are counted as used bytes.
    pub entries: usize,
    /// Active leases known to this instance.
    pub active_leases: u64,
}

#[derive(Debug)]
struct CacheEntry {
    file_id: u64,
    path: PathBuf,
    bytes: u64,
    last_used: u64,
    active_leases: u64,
    source_observation: Option<SourceObservation>,
}

#[derive(Debug)]
struct CacheLeasePin {
    _file: File,
    lease_count: u64,
}

/// Generated file recovered on reopen without an in-memory identity; it remains accounted and
/// evictable, but cannot be leased as a semantic cache entry.
#[derive(Debug)]
struct OrphanedFile {
    file_id: u64,
    path: PathBuf,
    bytes: u64,
    last_used: u64,
}

#[derive(Debug)]
struct CacheState {
    budget_bytes: u64,
    used_bytes: u64,
    next_file_id: u64,
    lru_clock: u64,
    entries: HashMap<CacheIdentity, CacheEntry>,
    entries_by_file_id: HashMap<u64, CacheIdentity>,
    orphaned_files: HashMap<u64, OrphanedFile>,
}

#[derive(Debug)]
struct CacheInner {
    root: PathBuf,
    managed_dir: PathBuf,
    root_operations_lock: Arc<Mutex<()>>,
    state: Mutex<CacheState>,
}

/// Thread-safe byte- and file-bounded cache of generated files beneath a private root. It retains
/// at most 1,024 managed files, including recovered orphans, so zero-byte entries remain bounded.
/// Each instance keeps LRU identity state for entries it knows; files known only through root
/// reconciliation remain accounted and are selected as orphaned eviction candidates by generated
/// file ID. Operations that account for root-wide byte usage serialize with a process-local
/// per-root mutex and an OS lock; instances sharing a root must use the same budget.
/// Active leases keep process-wide portable OS shared file locks until explicit acknowledgement;
/// deletion obtains an exclusive file lock and fails closed while any process holds a lease. The
/// caller must keep the root private and stable; portable path APIs cannot defend against a hostile
/// concurrent root-directory replacement.
#[derive(Clone, Debug)]
pub struct DerivedCache {
    inner: Arc<CacheInner>,
}

impl DerivedCache {
    /// Creates the private cache root if needed, canonicalizes it once, and creates or reuses a
    /// marked cache-owned subdirectory. Existing generated files are reconciled before use.
    /// Instances sharing a root must use the same byte budget.
    pub fn open(root: impl AsRef<Path>, budget_bytes: u64) -> Result<Self, CacheError> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root.as_ref())?;
        if !root.is_dir() {
            return Err(CacheError::Io(io::Error::new(
                ErrorKind::NotADirectory,
                "cache root is not a directory",
            )));
        }
        let root_operations_lock = cache_root_operation_lock(&root);
        let _root_operations_lock = lock_unpoisoned(&root_operations_lock);
        let managed_dir = create_cache_instance_dir(&root)?;
        let _operations_lock = lock_cache_directory(&root, &managed_dir)?;
        let (orphaned_files, used_bytes, next_file_id, lru_clock) = scan_orphaned_files(&root)?;
        let mut state = CacheState {
            budget_bytes,
            used_bytes,
            next_file_id,
            lru_clock,
            entries: HashMap::new(),
            entries_by_file_id: HashMap::new(),
            orphaned_files: orphaned_files
                .into_iter()
                .map(|file| (file.file_id, file))
                .collect(),
        };
        trim_to_budget(&root, &mut state)?;
        Ok(Self {
            inner: Arc::new(CacheInner {
                root,
                managed_dir,
                root_operations_lock: Arc::clone(&root_operations_lock),
                state: Mutex::new(state),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Stores data under an identity whose source digest has already been computed. For
    /// path-backed sources, use `store_observed` to retain revalidation hints.
    pub fn store(&self, identity: CacheIdentity, bytes: &[u8]) -> Result<StoreOutcome, CacheError> {
        self.store_with_observation(identity, bytes, None)
    }

    /// Stores a source observation alongside the content identity. Supplying a changed stat hint
    /// with the same digest here represents the caller's completed content revalidation.
    pub fn store_observed(
        &self,
        identity: CacheIdentity,
        observation: SourceObservation,
        bytes: &[u8],
    ) -> Result<StoreOutcome, CacheError> {
        if identity.source_content() != observation.identity() {
            return Err(CacheError::SourceIdentityMismatch);
        }
        self.store_with_observation(identity, bytes, Some(observation))
    }

    fn store_with_observation(
        &self,
        identity: CacheIdentity,
        bytes: &[u8],
        observation: Option<SourceObservation>,
    ) -> Result<StoreOutcome, CacheError> {
        let entry_bytes = u64::try_from(bytes.len()).map_err(|_| CacheError::ArithmeticOverflow)?;
        let mut state = lock_unpoisoned(&self.inner.state);
        #[cfg(test)]
        notify_root_operation_test_hook(
            RootOperationTestPoint::BeforeAcquire,
            &self.inner.root_operations_lock,
        );
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        #[cfg(test)]
        notify_root_operation_test_hook(
            RootOperationTestPoint::Acquired,
            &self.inner.root_operations_lock,
        );
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        if let Some((path, active_leases)) = state
            .entries
            .get(&identity)
            .map(|entry| (entry.path.clone(), entry.active_leases))
        {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    let tick = next_tick(&mut state)?;
                    if let Some(entry) = state.entries.get_mut(&identity) {
                        entry.last_used = tick;
                        if observation.is_some() {
                            entry.source_observation = observation;
                        }
                    }
                    return Ok(StoreOutcome::AlreadyPresent);
                }
                Ok(_) => return Err(CacheError::UnsafeDerivedPath(path)),
                Err(error) if error.kind() == ErrorKind::NotFound && active_leases == 0 => {
                    let removed = state
                        .entries
                        .remove(&identity)
                        .ok_or(CacheError::NotFound)?;
                    state.entries_by_file_id.remove(&removed.file_id);
                    state.used_bytes = state
                        .used_bytes
                        .checked_sub(removed.bytes)
                        .ok_or(CacheError::ArithmeticOverflow)?;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Err(CacheError::NotFound);
                }
                Err(error) => return Err(CacheError::Io(error)),
            }
        }
        if entry_bytes > state.budget_bytes {
            return Err(CacheError::EntryExceedsBudget {
                entry_bytes,
                budget_bytes: state.budget_bytes,
            });
        }
        evict_for(&self.inner.root, &mut state, entry_bytes)?;
        let new_used_bytes = state
            .used_bytes
            .checked_add(entry_bytes)
            .ok_or(CacheError::ArithmeticOverflow)?;
        let tick = next_tick(&mut state)?;
        let (file_id, path, mut file) =
            create_derived_file(&self.inner.root, &self.inner.managed_dir, &mut state)?;
        if let Err(error) = file.write_all(bytes).and_then(|()| file.flush()) {
            drop(file);
            delete_managed_file(&self.inner.root, &path)?;
            return Err(CacheError::Io(error));
        }
        state.used_bytes = new_used_bytes;
        state.entries_by_file_id.insert(file_id, identity.clone());
        state.entries.insert(
            identity,
            CacheEntry {
                file_id,
                path,
                bytes: entry_bytes,
                last_used: tick,
                active_leases: 0,
                source_observation: observation,
            },
        );
        Ok(StoreOutcome::Inserted)
    }

    /// Leases an entry from a caller-supplied identity. Path-backed entries stored with a source
    /// observation require `lease_observed`, so this method cannot bypass their revalidation.
    pub fn lease(&self, identity: &CacheIdentity) -> Result<CacheLease, CacheError> {
        self.lease_inner(identity, None)
    }

    /// Leases an external-source entry only when its current observation is backed by a fresh
    /// content digest and its path/stat hints still match the last validated observation. Stat
    /// metadata alone is never proof that source bytes are unchanged. A changed hint requires the
    /// caller to hash/revalidate and call `store_observed` before reuse.
    pub fn lease_observed(
        &self,
        identity: &CacheIdentity,
        current_observation: &SourceObservation,
    ) -> Result<CacheLease, CacheError> {
        if identity.source_content() != current_observation.identity() {
            return Err(CacheError::SourceIdentityMismatch);
        }
        self.lease_inner(identity, Some(current_observation))
    }

    fn lease_inner(
        &self,
        identity: &CacheIdentity,
        current_observation: Option<&SourceObservation>,
    ) -> Result<CacheLease, CacheError> {
        verify_managed_root(&self.inner.root)?;
        let mut state = lock_unpoisoned(&self.inner.state);
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        let entry = state.entries.get(identity).ok_or(CacheError::NotFound)?;
        match (current_observation, entry.source_observation.as_ref()) {
            (Some(current), Some(stored)) => {
                if current.requires_content_revalidation_from(stored) {
                    return Err(CacheError::SourceRevalidationRequired);
                }
            }
            (Some(_), None) => return Err(CacheError::SourceObservationUnavailable),
            (None, Some(_)) => return Err(CacheError::SourceObservationRequired),
            (None, None) => {}
        }
        let path = entry.path.clone();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => return Err(CacheError::UnsafeDerivedPath(path)),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let removed = state.entries.remove(identity).ok_or(CacheError::NotFound)?;
                state.entries_by_file_id.remove(&removed.file_id);
                state.used_bytes = state
                    .used_bytes
                    .checked_sub(removed.bytes)
                    .ok_or(CacheError::ArithmeticOverflow)?;
                return Err(CacheError::NotFound);
            }
            Err(error) => return Err(CacheError::Io(error)),
        }
        let path = entry.path.clone();
        let next_active_leases = entry
            .active_leases
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
        let tick = state
            .lru_clock
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
        retain_cache_lease_pin(&path)?;
        state.lru_clock = tick;
        let entry = state
            .entries
            .get_mut(identity)
            .expect("validated leased entry remains present under the state lock");
        entry.active_leases = next_active_leases;
        entry.last_used = tick;
        Ok(CacheLease {
            inner: Arc::clone(&self.inner),
            identity: identity.clone(),
            path,
            released: false,
        })
    }

    /// Changes the configured limit and immediately evicts eligible LRU entries. If pinned
    /// entries alone exceed the new limit, the lower limit remains in force and the error reports
    /// that completion acknowledgements are needed before it can be met.
    pub fn set_budget_bytes(&self, budget_bytes: u64) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        state.budget_bytes = budget_bytes;
        trim_to_budget(&self.inner.root, &mut state)
    }

    /// Retries eviction after an owner boundary or other external state change.
    pub fn trim_to_budget(&self) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        trim_to_budget(&self.inner.root, &mut state)
    }

    pub fn snapshot(&self) -> Result<CacheSnapshot, CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        let active_leases = state.entries.values().try_fold(0u64, |sum, entry| {
            sum.checked_add(entry.active_leases)
                .ok_or(CacheError::ArithmeticOverflow)
        })?;
        Ok(CacheSnapshot {
            budget_bytes: state.budget_bytes,
            used_bytes: state.used_bytes,
            entries: state.entries.len(),
            active_leases,
        })
    }
}

/// A non-cloneable resource pin. Dropping it intentionally does not release the pin; a
/// process-wide keeper retains the shared file lock even after the lease and cache handles drop.
/// With no remaining lease handle to acknowledge, the pin remains until process exit.
/// Call `acknowledge` only when every decoder/upload/user has completed.
#[must_use = "explicitly acknowledge resource completion to unpin this cache entry"]
#[derive(Debug)]
pub struct CacheLease {
    inner: Arc<CacheInner>,
    identity: CacheIdentity,
    path: PathBuf,
    released: bool,
}

impl CacheLease {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> &CacheIdentity {
        &self.identity
    }

    /// Releases this lease's pin after obtaining the cache operations lock. Errors before release
    /// preserve the lease for retry; errors during trimming leave the pin released and a retry only
    /// retries trimming.
    pub fn acknowledge(&mut self) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _root_operations_lock = lock_unpoisoned(&self.inner.root_operations_lock);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        if !self.released {
            let active_leases = state
                .entries
                .get(&self.identity)
                .ok_or(CacheError::InvalidLease)?
                .active_leases
                .checked_sub(1)
                .ok_or(CacheError::InvalidLease)?;
            let tick = state
                .lru_clock
                .checked_add(1)
                .ok_or(CacheError::ArithmeticOverflow)?;
            let released_pin = release_cache_lease_pin(&self.path)?;
            state.lru_clock = tick;
            let entry = state
                .entries
                .get_mut(&self.identity)
                .expect("validated leased entry remains present under the state lock");
            entry.active_leases = active_leases;
            entry.last_used = tick;
            self.released = true;
            drop(released_pin);
        }
        trim_to_budget(&self.inner.root, &mut state)
    }
}

fn retain_cache_lease_pin(path: &Path) -> Result<(), CacheError> {
    let mut pins = lock_unpoisoned(&CACHE_LEASE_PINS);
    if let Some(pin) = pins.get_mut(path) {
        pin.lease_count = pin
            .lease_count
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
        return Ok(());
    }

    let file = File::open(path)?;
    file.try_lock_shared().map_err(io::Error::from)?;
    pins.insert(
        path.to_path_buf(),
        CacheLeasePin {
            _file: file,
            lease_count: 1,
        },
    );
    Ok(())
}

fn release_cache_lease_pin(path: &Path) -> Result<Option<CacheLeasePin>, CacheError> {
    let mut pins = lock_unpoisoned(&CACHE_LEASE_PINS);
    let remaining = pins
        .get(path)
        .ok_or(CacheError::InvalidLease)?
        .lease_count
        .checked_sub(1)
        .ok_or(CacheError::InvalidLease)?;
    if remaining == 0 {
        return pins.remove(path).map(Some).ok_or(CacheError::InvalidLease);
    }
    pins.get_mut(path)
        .ok_or(CacheError::InvalidLease)?
        .lease_count = remaining;
    Ok(None)
}

fn next_tick(state: &mut CacheState) -> Result<u64, CacheError> {
    state.lru_clock = state
        .lru_clock
        .checked_add(1)
        .ok_or(CacheError::ArithmeticOverflow)?;
    Ok(state.lru_clock)
}
fn reconcile_cache_state(root: &Path, state: &mut CacheState) -> Result<(), CacheError> {
    let (mut discovered, used_bytes, next_file_id, lru_clock) = scan_orphaned_files(root)?;
    if state.entries.values().any(|entry| {
        entry.active_leases > 0
            && discovered
                .binary_search_by_key(&entry.file_id, |file| file.file_id)
                .is_err()
    }) {
        return Err(CacheError::InvalidLease);
    }

    state.entries.retain(|_, entry| {
        if let Ok(index) = discovered.binary_search_by_key(&entry.file_id, |file| file.file_id) {
            entry.bytes = discovered[index].bytes;
            true
        } else {
            false
        }
    });
    let entries = &state.entries;
    state
        .entries_by_file_id
        .retain(|_, identity| entries.contains_key(identity));
    discovered.retain(|file| !state.entries_by_file_id.contains_key(&file.file_id));
    state.orphaned_files = discovered
        .into_iter()
        .map(|file| (file.file_id, file))
        .collect();
    state.used_bytes = used_bytes;
    state.next_file_id = state.next_file_id.max(next_file_id);
    state.lru_clock = state.lru_clock.max(lru_clock);
    Ok(())
}

fn cache_file_count(state: &CacheState) -> usize {
    state
        .entries
        .len()
        .saturating_add(state.orphaned_files.len())
}

fn evict_for(root: &Path, state: &mut CacheState, requested_bytes: u64) -> Result<(), CacheError> {
    let target_bytes = state
        .budget_bytes
        .checked_sub(requested_bytes)
        .ok_or(CacheError::ArithmeticOverflow)?;
    evict_to_limits(root, state, target_bytes, 1, requested_bytes)
}

fn trim_to_budget(root: &Path, state: &mut CacheState) -> Result<(), CacheError> {
    let target_bytes = state.budget_bytes;
    evict_to_limits(root, state, target_bytes, 0, 0)
}

struct EvictionCandidate {
    rank: (u64, bool, u64),
}

fn evict_to_limits(
    root: &Path,
    state: &mut CacheState,
    target_bytes: u64,
    requested_files: usize,
    requested_bytes: u64,
) -> Result<(), CacheError> {
    let target_files = MAX_CACHE_FILES.saturating_sub(requested_files);
    if state.used_bytes <= target_bytes && cache_file_count(state) <= target_files {
        return Ok(());
    }
    // Build one stable LRU snapshot so blocked candidates are skipped in linear traversal rather
    // than rescanning the complete entry/orphan sets after every deletion attempt.
    let mut candidates = Vec::with_capacity(cache_file_count(state));
    candidates.extend(
        state
            .entries
            .values()
            .filter(|entry| entry.active_leases == 0)
            .map(|entry| EvictionCandidate {
                rank: (entry.last_used, false, entry.file_id),
            }),
    );
    candidates.extend(state.orphaned_files.values().map(|file| EvictionCandidate {
        rank: (file.last_used, true, file.file_id),
    }));
    candidates.sort_unstable_by_key(|candidate| candidate.rank);
    #[cfg(test)]
    notify_eviction_test_hook(EvictionTestPoint::CandidatesSnapshotted(candidates.len()));

    let mut remaining_files = cache_file_count(state);
    for candidate in candidates {
        if state.used_bytes <= target_bytes && remaining_files <= target_files {
            return Ok(());
        }
        #[cfg(test)]
        notify_eviction_test_hook(EvictionTestPoint::CandidateExamined);
        let (_, is_orphan, file_id) = candidate.rank;
        let removed = if is_orphan {
            remove_orphaned_file(root, state, file_id)?
        } else {
            remove_entry_by_file_id(root, state, file_id)?
        };
        if removed {
            remaining_files = remaining_files
                .checked_sub(1)
                .ok_or(CacheError::ArithmeticOverflow)?;
        }
    }
    if remaining_files > target_files {
        return Err(CacheError::FileCountPinned {
            current_files: remaining_files,
            max_files: MAX_CACHE_FILES,
            requested_files,
        });
    }
    if state.used_bytes > target_bytes {
        return Err(CacheError::BudgetPinned {
            used_bytes: state.used_bytes,
            budget_bytes: state.budget_bytes,
            requested_bytes,
        });
    }
    Ok(())
}

fn remove_orphaned_file(
    root: &Path,
    state: &mut CacheState,
    file_id: u64,
) -> Result<bool, CacheError> {
    let orphan = state
        .orphaned_files
        .get(&file_id)
        .ok_or(CacheError::NotFound)?;
    if !delete_managed_file(root, &orphan.path)? {
        return Ok(false);
    }
    let removed = state
        .orphaned_files
        .remove(&file_id)
        .ok_or(CacheError::NotFound)?;
    state.used_bytes = state
        .used_bytes
        .checked_sub(removed.bytes)
        .ok_or(CacheError::ArithmeticOverflow)?;
    Ok(true)
}

fn remove_entry_by_file_id(
    root: &Path,
    state: &mut CacheState,
    file_id: u64,
) -> Result<bool, CacheError> {
    let deleted = {
        let identity = state
            .entries_by_file_id
            .get(&file_id)
            .ok_or(CacheError::NotFound)?;
        let entry = state.entries.get(identity).ok_or(CacheError::NotFound)?;
        if entry.active_leases != 0 {
            return Err(CacheError::InvalidLease);
        }
        delete_managed_file(root, &entry.path)?
    };
    if !deleted {
        return Ok(false);
    }
    let identity = state
        .entries_by_file_id
        .remove(&file_id)
        .ok_or(CacheError::NotFound)?;
    let removed = state
        .entries
        .remove(&identity)
        .ok_or(CacheError::NotFound)?;
    state.used_bytes = state
        .used_bytes
        .checked_sub(removed.bytes)
        .ok_or(CacheError::ArithmeticOverflow)?;
    Ok(true)
}

fn verify_managed_root(root: &Path) -> Result<(), CacheError> {
    let metadata = fs::symlink_metadata(root)?;
    let canonical_root = fs::canonicalize(root)?;
    if !metadata.file_type().is_dir() || canonical_root.as_path() != root {
        return Err(CacheError::UnsafeDerivedPath(root.to_path_buf()));
    }
    Ok(())
}

fn cache_instance_is_owned(root: &Path, managed_dir: &Path) -> Result<bool, CacheError> {
    if managed_dir.parent() != Some(root)
        || managed_dir.file_name() != Some(OsStr::new(CACHE_INSTANCE_DIR))
    {
        return Ok(false);
    }
    let directory_metadata = match fs::symlink_metadata(managed_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CacheError::Io(error)),
    };
    if !directory_metadata.file_type().is_dir()
        || fs::canonicalize(managed_dir)?.as_path() != managed_dir
    {
        return Ok(false);
    }
    let marker_path = managed_dir.join(CACHE_OWNER_FILE);
    let marker_metadata = match fs::symlink_metadata(&marker_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(CacheError::Io(error)),
    };
    if !marker_metadata.file_type().is_file() {
        return Ok(false);
    }
    Ok(fs::read(marker_path)? == CACHE_OWNER_MARKER)
}

fn verify_owned_cache_dir(root: &Path, managed_dir: &Path) -> Result<(), CacheError> {
    verify_managed_root(root)?;
    if !cache_instance_is_owned(root, managed_dir)? {
        return Err(CacheError::UnsafeDerivedPath(managed_dir.to_path_buf()));
    }
    Ok(())
}

fn write_cache_owner_marker(directory: &Path) -> Result<(), CacheError> {
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(CACHE_OWNER_FILE))?;
    marker.write_all(CACHE_OWNER_MARKER)?;
    marker.sync_all()?;
    Ok(())
}

fn lock_cache_initialization(root: &Path) -> Result<File, CacheError> {
    let lock_path = root.join(CACHE_INITIALIZATION_LOCK);
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&lock_path)?;
            if !metadata.file_type().is_file() {
                return Err(CacheError::UnsafeDerivedPath(lock_path));
            }
            OpenOptions::new().read(true).write(true).open(lock_path)?
        }
        Err(error) => return Err(CacheError::Io(error)),
    };
    file.lock()?;
    Ok(file)
}

fn create_cache_instance_staging_dir(root: &Path) -> Result<PathBuf, CacheError> {
    loop {
        let id = CACHE_INITIALIZATION_TEMP_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| CacheError::ArithmeticOverflow)?;
        let name = format!("{CACHE_INSTANCE_DIR}.init-{}-{id:016x}", std::process::id());
        let path = root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CacheError::Io(error)),
        }
    }
}

fn create_cache_instance_dir(root: &Path) -> Result<PathBuf, CacheError> {
    create_cache_instance_dir_with_sync(root, crate::storage::sync_parent_directory)
}

fn create_cache_instance_dir_with_sync(
    root: &Path,
    mut sync_directory: impl FnMut(&Path) -> io::Result<()>,
) -> Result<PathBuf, CacheError> {
    verify_managed_root(root)?;
    let _initialization_lock = lock_cache_initialization(root)?;
    let managed_dir = root.join(CACHE_INSTANCE_DIR);
    match fs::symlink_metadata(&managed_dir) {
        Ok(_) if cache_instance_is_owned(root, &managed_dir)? => return Ok(managed_dir),
        Ok(_) => return Err(CacheError::UnsafeDerivedPath(managed_dir)),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(CacheError::Io(error)),
    }

    let staging_dir = create_cache_instance_staging_dir(root)?;
    if let Err(error) = write_cache_owner_marker(&staging_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }
    if let Err(error) = sync_directory(&staging_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(CacheError::Io(error));
    }
    if let Err(error) = fs::rename(&staging_dir, &managed_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
        if cache_instance_is_owned(root, &managed_dir)? {
            sync_directory(root)?;
            return Ok(managed_dir);
        }
        return match fs::symlink_metadata(&managed_dir) {
            Ok(_) => Err(CacheError::UnsafeDerivedPath(managed_dir)),
            Err(path_error) if path_error.kind() == ErrorKind::NotFound => {
                Err(CacheError::Io(error))
            }
            Err(path_error) => Err(CacheError::Io(path_error)),
        };
    }
    sync_directory(root)?;
    Ok(managed_dir)
}

fn lock_cache_directory(root: &Path, managed_dir: &Path) -> Result<File, CacheError> {
    verify_owned_cache_dir(root, managed_dir)?;
    let lock_path = managed_dir.join(CACHE_OPERATIONS_LOCK);
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&lock_path)?;
            if !metadata.file_type().is_file() {
                return Err(CacheError::UnsafeDerivedPath(lock_path));
            }
            OpenOptions::new().read(true).write(true).open(lock_path)?
        }
        Err(error) => return Err(CacheError::Io(error)),
    };
    file.lock()?;
    Ok(file)
}

fn file_id_high_water_initialized(managed_dir: &Path) -> Result<bool, CacheError> {
    let path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER_MARKER);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => Err(CacheError::UnsafeDerivedPath(path)),
        Ok(metadata) if metadata.len() == 0 => Ok(true),
        Ok(_) => Err(invalid_file_id_high_water()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn invalid_file_id_high_water() -> CacheError {
    CacheError::Io(invalid_file_id_high_water_data())
}

fn invalid_file_id_high_water_data() -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        "invalid derived cache file ID high-water mark",
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HighWaterFileState {
    Missing,
    Valid(u64),
    Corrupt,
}

fn decode_file_id_high_water(file: &mut File) -> io::Result<u64> {
    let mut encoded = [0u8; 17];
    if let Err(error) = file.read_exact(&mut encoded) {
        return if error.kind() == ErrorKind::UnexpectedEof {
            Err(invalid_file_id_high_water_data())
        } else {
            Err(error)
        };
    }
    let mut trailing = [0u8; 1];
    if file.read(&mut trailing)? != 0 || encoded[16] != b'\n' {
        return Err(invalid_file_id_high_water_data());
    }

    let mut high_water = 0u64;
    for &byte in &encoded[..16] {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(invalid_file_id_high_water_data()),
        };
        high_water = (high_water << 4) | u64::from(digit);
    }
    Ok(high_water)
}

fn high_water_file_state(path: &Path) -> io::Result<HighWaterFileState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(HighWaterFileState::Missing);
        }
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "high-water path is not a regular file",
        ));
    }

    match decode_file_id_high_water(&mut File::open(path)?) {
        Ok(high_water) => Ok(HighWaterFileState::Valid(high_water)),
        Err(error) if error.kind() == ErrorKind::InvalidData => Ok(HighWaterFileState::Corrupt),
        Err(error) => Err(error),
    }
}

fn read_file_id_high_water(managed_dir: &Path) -> Result<u64, CacheError> {
    recover_file_id_high_water_backup(managed_dir)?;
    let path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_file() {
        return Err(CacheError::UnsafeDerivedPath(path));
    }
    decode_file_id_high_water(&mut File::open(path)?).map_err(CacheError::Io)
}

fn write_file_id_high_water(
    managed_dir: &Path,
    high_water: u64,
    create_new: bool,
) -> Result<(), CacheError> {
    write_file_id_high_water_with_sync(
        managed_dir,
        high_water,
        create_new,
        HighWaterWriteHooks {
            write_encoded: |file: &mut File, encoded: &[u8]| file.write_all(encoded),
            sync_file: File::sync_all,
            replace: replace_file_id_high_water,
            sync_parent_directory: crate::storage::sync_parent_directory,
        },
    )
}

fn create_file_id_high_water_temp(managed_dir: &Path) -> Result<(PathBuf, File), CacheError> {
    loop {
        let id = CACHE_FILE_ID_HIGH_WATER_TEMP_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| CacheError::ArithmeticOverflow)?;
        let path = managed_dir.join(format!(
            ".{CACHE_FILE_ID_HIGH_WATER}.update-{}-{id:016x}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CacheError::Io(error)),
        }
    }
}

struct HighWaterWriteHooks<W, S, R, D> {
    write_encoded: W,
    sync_file: S,
    replace: R,
    sync_parent_directory: D,
}

fn write_file_id_high_water_with_sync<W, S, R, D>(
    managed_dir: &Path,
    high_water: u64,
    create_new: bool,
    hooks: HighWaterWriteHooks<W, S, R, D>,
) -> Result<(), CacheError>
where
    W: FnOnce(&mut File, &[u8]) -> io::Result<()>,
    S: FnOnce(&File) -> io::Result<()>,
    R: FnOnce(&Path, &Path) -> io::Result<()>,
    D: FnOnce(&Path) -> io::Result<()>,
{
    let HighWaterWriteHooks {
        write_encoded,
        sync_file,
        replace,
        sync_parent_directory,
    } = hooks;
    let path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER);
    let mut encoded = [b'0'; 17];
    encoded[16] = b'\n';
    let mut value = high_water;
    for digit in encoded[..16].iter_mut().rev() {
        *digit = b"0123456789abcdef"[(value & 0x0f) as usize];
        value >>= 4;
    }

    let (temp_path, mut file) = create_file_id_high_water_temp(managed_dir)?;
    let write_result = write_encoded(&mut file, &encoded).and_then(|()| sync_file(&file));
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(CacheError::Io(error));
    }

    if create_new {
        #[cfg(windows)]
        let publish_result = move_file_write_through(&temp_path, &path, false);
        #[cfg(not(windows))]
        let publish_result = fs::hard_link(&temp_path, &path);
        if let Err(error) = publish_result {
            let _ = fs::remove_file(&temp_path);
            return Err(CacheError::Io(error));
        }
        #[cfg(not(windows))]
        let _ = fs::remove_file(&temp_path);
        sync_parent_directory(managed_dir)?;
        return Ok(());
    }

    if let Err(error) = replace(&temp_path, &path) {
        let _ = fs::remove_file(&temp_path);
        return Err(CacheError::Io(error));
    }
    sync_parent_directory(managed_dir)?;
    Ok(())
}

fn mark_file_id_high_water_initialized(managed_dir: &Path) -> Result<(), CacheError> {
    mark_file_id_high_water_initialized_with_sync(
        managed_dir,
        File::sync_all,
        crate::storage::sync_parent_directory,
    )
}

fn mark_file_id_high_water_initialized_with_sync(
    managed_dir: &Path,
    sync_file: impl FnOnce(&File) -> io::Result<()>,
    sync_parent_directory: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), CacheError> {
    let path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER_MARKER);
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    sync_file(&file)?;
    sync_parent_directory(managed_dir)?;
    Ok(())
}

// The initialization marker is synced before a cache can allocate IDs. If an unmarked
// high-water file is incomplete, its value is not trustworthy; reconcile live filenames
// first, then replace only invalid counter artifacts with that discovered lower bound.
fn discard_corrupt_initial_file_id_high_water(managed_dir: &Path) -> Result<(), CacheError> {
    for name in [CACHE_FILE_ID_HIGH_WATER, CACHE_FILE_ID_HIGH_WATER_BACKUP] {
        let path = managed_dir.join(name);
        match high_water_file_state(&path)? {
            HighWaterFileState::Missing => {}
            HighWaterFileState::Corrupt => fs::remove_file(path)?,
            HighWaterFileState::Valid(_) => return Err(invalid_file_id_high_water()),
        }
    }
    crate::storage::sync_parent_directory(managed_dir)?;
    Ok(())
}

// Callers hold the cache operations lock so migration and reservations serialize across instances.
fn ensure_file_id_high_water(root: &Path, discovered_next_id: u64) -> Result<u64, CacheError> {
    let managed_dir = root.join(CACHE_INSTANCE_DIR);
    verify_owned_cache_dir(root, &managed_dir)?;
    let initialized = file_id_high_water_initialized(&managed_dir)?;
    match recover_file_id_high_water_backup(&managed_dir) {
        Ok(()) => {}
        Err(error) if !initialized && error.kind() == ErrorKind::InvalidData => {
            discard_corrupt_initial_file_id_high_water(&managed_dir)?;
        }
        Err(error) => return Err(CacheError::Io(error)),
    }
    let counter_path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER);
    let high_water = match fs::symlink_metadata(&counter_path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(CacheError::UnsafeDerivedPath(counter_path));
            }
            let stored = match read_file_id_high_water(&managed_dir) {
                Ok(stored) => stored,
                Err(CacheError::Io(error))
                    if !initialized && error.kind() == ErrorKind::InvalidData =>
                {
                    discard_corrupt_initial_file_id_high_water(&managed_dir)?;
                    write_file_id_high_water(&managed_dir, discovered_next_id, true)?;
                    discovered_next_id
                }
                Err(error) => return Err(error),
            };
            let high_water = stored.max(discovered_next_id);
            if high_water != stored {
                write_file_id_high_water(&managed_dir, high_water, false)?;
            }
            high_water
        }
        Err(error) if error.kind() == ErrorKind::NotFound && initialized => {
            return Err(invalid_file_id_high_water());
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            write_file_id_high_water(&managed_dir, discovered_next_id, true)?;
            discovered_next_id
        }
        Err(error) => return Err(CacheError::Io(error)),
    };
    if !initialized {
        mark_file_id_high_water_initialized(&managed_dir)?;
    }
    Ok(high_water)
}

// The high-water mark is synchronized before the corresponding filename is created.

fn reserve_file_id(managed_dir: &Path, state: &mut CacheState) -> Result<u64, CacheError> {
    let id = state
        .next_file_id
        .max(read_file_id_high_water(managed_dir)?);
    let next_file_id = id.checked_add(1).ok_or(CacheError::ArithmeticOverflow)?;
    write_file_id_high_water(managed_dir, next_file_id, false)?;
    state.next_file_id = next_file_id;
    Ok(id)
}

fn create_derived_file(
    root: &Path,
    managed_dir: &Path,
    state: &mut CacheState,
) -> Result<(u64, PathBuf, File), CacheError> {
    verify_owned_cache_dir(root, managed_dir)?;
    loop {
        let id = reserve_file_id(managed_dir, state)?;
        let path = managed_dir.join(format!("derived-{id:016x}.bin"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                file.try_lock().map_err(io::Error::from)?;
                return Ok((id, path, file));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CacheError::Io(error)),
        }
    }
}

fn delete_managed_file(root: &Path, path: &Path) -> Result<bool, CacheError> {
    verify_managed_root(root)?;
    let Some(managed_dir) = path.parent() else {
        return Err(CacheError::UnsafeDerivedPath(path.to_path_buf()));
    };
    let filename_is_managed = path
        .file_name()
        .map(|name| derived_file_id(name).is_some())
        .unwrap_or(false);
    if !filename_is_managed {
        return Err(CacheError::UnsafeDerivedPath(path.to_path_buf()));
    }
    verify_owned_cache_dir(root, managed_dir)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            let file = File::open(path)?;
            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => return Ok(false),
                Err(TryLockError::Error(error)) => return Err(CacheError::Io(error)),
            }
            fs::remove_file(path).map(|()| true).map_err(CacheError::Io)
        }
        Ok(_) => Err(CacheError::UnsafeDerivedPath(path.to_path_buf())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn derived_file_id(file_name: &OsStr) -> Option<u64> {
    let name = file_name.to_str()?;
    let digits = name.strip_prefix("derived-")?.strip_suffix(".bin")?;
    if digits.len() != 16 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

fn scan_orphaned_files(root: &Path) -> Result<(Vec<OrphanedFile>, u64, u64, u64), CacheError> {
    let mut discovered = Vec::new();
    let mut next_file_id = 0u64;
    for directory_entry in fs::read_dir(root)? {
        let directory_entry = directory_entry?;
        let managed_dir = directory_entry.path();
        if !cache_instance_is_owned(root, &managed_dir)? {
            continue;
        }
        for entry in fs::read_dir(&managed_dir)? {
            let entry = entry?;
            let Some(file_id) = derived_file_id(&entry.file_name()) else {
                continue;
            };
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            next_file_id = next_file_id.max(
                file_id
                    .checked_add(1)
                    .ok_or(CacheError::ArithmeticOverflow)?,
            );
            if !metadata.file_type().is_file() {
                continue;
            }
            discovered.push((file_id, path, metadata.len()));
        }
    }
    let next_file_id = ensure_file_id_high_water(root, next_file_id)?;
    discovered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut used_bytes = 0u64;
    let mut lru_clock = 0u64;
    let mut orphaned_files = Vec::with_capacity(discovered.len());
    for (file_id, path, bytes) in discovered {
        used_bytes = used_bytes
            .checked_add(bytes)
            .ok_or(CacheError::ArithmeticOverflow)?;
        lru_clock = lru_clock
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
        orphaned_files.push(OrphanedFile {
            file_id,
            path,
            bytes,
            last_used: lru_clock,
        });
    }
    Ok((orphaned_files, used_bytes, next_file_id, lru_clock))
}

/// Three independent categories of bytes needed by a disk operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiskRequirements {
    pub expanded_input_bytes: u64,
    pub scratch_bytes: u64,
    pub output_bytes: u64,
}

impl DiskRequirements {
    pub fn total_bytes(self) -> Result<u64, DiskReservationError> {
        self.expanded_input_bytes
            .checked_add(self.scratch_bytes)
            .and_then(|sum| sum.checked_add(self.output_bytes))
            .ok_or(DiskReservationError::ArithmeticOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskReservationError {
    ArithmeticOverflow,
    InsufficientSpace {
        available_bytes: u64,
        safety_reserve_bytes: u64,
        currently_reserved_bytes: u64,
        requested_bytes: u64,
    },
}

impl core::fmt::Display for DiskReservationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ArithmeticOverflow => f.write_str("disk reservation byte arithmetic overflowed"),
            Self::InsufficientSpace {
                available_bytes,
                safety_reserve_bytes,
                currently_reserved_bytes,
                requested_bytes,
            } => write!(
                f,
                "disk reservation needs {requested_bytes} bytes with {currently_reserved_bytes} already reserved and {safety_reserve_bytes} held in reserve, but only {available_bytes} bytes are available"
            ),
        }
    }
}

impl std::error::Error for DiskReservationError {}

#[derive(Debug)]
struct DiskBudgetState {
    available_bytes: u64,
    reserved_bytes: u64,
    consumed_bytes_since_refresh: u64,
    total_consumed_bytes: u128,
    next_probe_id: u64,
    applied_probe_id: u64,
}

#[derive(Debug)]
struct DiskBudgetInner {
    safety_reserve_bytes: u64,
    state: Mutex<DiskBudgetState>,
}

/// Snapshot token captured before probing host disk space. Applying a token charges disk bytes
/// accounted after it was created and ignores readings from older or unrelated probes.
#[derive(Clone, Debug)]
pub struct DiskBudgetProbe {
    inner: Arc<DiskBudgetInner>,
    probe_id: u64,
    total_consumed_bytes: u128,
}

/// Shared reservation ledger. The host supplies available bytes from its platform disk-space
/// probe; every reservation holds back the configured safety reserve. Bytes recorded as consumed
/// after a probe token is captured remain charged against the reading applied with that token.
#[derive(Clone, Debug)]
pub struct DiskBudget {
    inner: Arc<DiskBudgetInner>,
}

impl DiskBudget {
    pub fn new(available_bytes: u64, safety_reserve_bytes: u64) -> Self {
        Self {
            inner: Arc::new(DiskBudgetInner {
                safety_reserve_bytes,
                state: Mutex::new(DiskBudgetState {
                    available_bytes,
                    reserved_bytes: 0,
                    consumed_bytes_since_refresh: 0,
                    total_consumed_bytes: 0,
                    next_probe_id: 0,
                    applied_probe_id: 0,
                }),
            }),
        }
    }

    /// Starts a host-space probe. Capture this token before invoking the host probe, then apply it
    /// with `refresh_available_bytes`; bytes consumed after this point remain charged to it.
    pub fn begin_available_bytes_probe(&self) -> Result<DiskBudgetProbe, DiskReservationError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let probe_id = state
            .next_probe_id
            .checked_add(1)
            .ok_or(DiskReservationError::ArithmeticOverflow)?;
        state.next_probe_id = probe_id;
        Ok(DiskBudgetProbe {
            inner: Arc::clone(&self.inner),
            probe_id,
            total_consumed_bytes: state.total_consumed_bytes,
        })
    }

    /// Applies a host-probed free-space estimate. Returns false for a stale token or a token from
    /// another ledger. Bytes accounted as consumed after capture remain charged to the reading.
    pub fn refresh_available_bytes(&self, probe: DiskBudgetProbe, available_bytes: u64) -> bool {
        if !Arc::ptr_eq(&probe.inner, &self.inner) {
            return false;
        }
        let mut state = lock_unpoisoned(&self.inner.state);
        if probe.probe_id <= state.applied_probe_id {
            return false;
        }
        let Some(bytes_consumed_after_probe) = state
            .total_consumed_bytes
            .checked_sub(probe.total_consumed_bytes)
        else {
            return false;
        };
        state.available_bytes = available_bytes;
        state.consumed_bytes_since_refresh =
            u64::try_from(bytes_consumed_after_probe).unwrap_or(u64::MAX);
        state.applied_probe_id = probe.probe_id;
        true
    }

    pub fn reserve(
        &self,
        requirements: DiskRequirements,
    ) -> Result<DiskReservation, DiskReservationError> {
        let requested_bytes = requirements.total_bytes()?;
        let mut state = lock_unpoisoned(&self.inner.state);
        let currently_accounted = state
            .reserved_bytes
            .checked_add(state.consumed_bytes_since_refresh)
            .ok_or(DiskReservationError::ArithmeticOverflow)?;
        let with_request = currently_accounted
            .checked_add(requested_bytes)
            .ok_or(DiskReservationError::ArithmeticOverflow)?;
        let allocatable = state
            .available_bytes
            .saturating_sub(self.inner.safety_reserve_bytes);
        if with_request > allocatable {
            return Err(DiskReservationError::InsufficientSpace {
                available_bytes: state.available_bytes,
                safety_reserve_bytes: self.inner.safety_reserve_bytes,
                currently_reserved_bytes: currently_accounted,
                requested_bytes,
            });
        }
        state.reserved_bytes = state
            .reserved_bytes
            .checked_add(requested_bytes)
            .ok_or(DiskReservationError::ArithmeticOverflow)?;
        Ok(DiskReservation {
            inner: Arc::clone(&self.inner),
            requirements,
            reserved_bytes: requested_bytes,
            output_written_bytes: Mutex::new(0),
        })
    }

    pub fn snapshot(&self) -> DiskBudgetSnapshot {
        let state = lock_unpoisoned(&self.inner.state);
        DiskBudgetSnapshot {
            available_bytes: state.available_bytes,
            safety_reserve_bytes: self.inner.safety_reserve_bytes,
            reserved_bytes: state.reserved_bytes,
            consumed_bytes_since_refresh: state.consumed_bytes_since_refresh,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskBudgetSnapshot {
    pub available_bytes: u64,
    pub safety_reserve_bytes: u64,
    pub reserved_bytes: u64,
    pub consumed_bytes_since_refresh: u64,
}

/// RAII accounting for expanded input, scratch, and output capacity. Dropping releases unwritten
/// output capacity; written output remains charged, while non-output capacity is conservatively
/// charged until refresh because temporary files may survive cancellation or errors.
#[derive(Debug)]
pub struct DiskReservation {
    inner: Arc<DiskBudgetInner>,
    requirements: DiskRequirements,
    reserved_bytes: u64,
    output_written_bytes: Mutex<u64>,
}

impl DiskReservation {
    pub fn requirements(&self) -> DiskRequirements {
        self.requirements
    }

    fn record_output_bytes(
        &self,
        output_written_bytes: &mut u64,
        written_bytes: u64,
    ) -> io::Result<()> {
        let next_output_written = output_written_bytes
            .checked_add(written_bytes)
            .ok_or_else(|| io::Error::other("written output byte accounting overflowed"))?;
        let mut state = lock_unpoisoned(&self.inner.state);
        let next_reserved = state
            .reserved_bytes
            .checked_sub(written_bytes)
            .ok_or_else(|| io::Error::other("disk reservation accounting invariant violated"))?;
        let next_consumed = state
            .consumed_bytes_since_refresh
            .checked_add(written_bytes)
            .ok_or_else(|| io::Error::other("consumed output byte accounting overflowed"))?;
        let next_total_consumed_bytes = state
            .total_consumed_bytes
            .checked_add(u128::from(written_bytes))
            .ok_or_else(|| io::Error::other("total disk consumption accounting overflowed"))?;
        state.reserved_bytes = next_reserved;
        state.consumed_bytes_since_refresh = next_consumed;
        state.total_consumed_bytes = next_total_consumed_bytes;
        *output_written_bytes = next_output_written;
        Ok(())
    }
}

impl Drop for DiskReservation {
    fn drop(&mut self) {
        let output_written_bytes = lock_unpoisoned(&self.output_written_bytes);
        let unwritten_reserved_bytes = self
            .reserved_bytes
            .checked_sub(*output_written_bytes)
            .expect("disk reservation output accounting invariant violated");
        let non_output_bytes = self
            .requirements
            .expanded_input_bytes
            .checked_add(self.requirements.scratch_bytes)
            .expect("disk reservation non-output accounting invariant violated");
        let mut state = lock_unpoisoned(&self.inner.state);
        let next_reserved = state
            .reserved_bytes
            .checked_sub(unwritten_reserved_bytes)
            .expect("disk reservation accounting invariant violated");
        let next_consumed = state
            .consumed_bytes_since_refresh
            .saturating_add(non_output_bytes);
        let next_total_consumed = state
            .total_consumed_bytes
            .checked_add(u128::from(non_output_bytes))
            .expect("total disk consumption accounting invariant violated");
        state.reserved_bytes = next_reserved;
        state.consumed_bytes_since_refresh = next_consumed;
        state.total_consumed_bytes = next_total_consumed;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFailureKind {
    TemporaryCreate,
    Writer,
    DiskFull,
    OutputLimitExceeded,
    Flush,
    Sync,
    Commit,
}

/// Failed output transaction. Before replacement, staged bytes remain at `partial_temp_path`.
/// After publication, a parent-sync failure triggers rollback to the previous destination (or
/// removal of a newly created destination). `recovery_path` identifies data retained when that
/// rollback cannot complete or its durability cannot be confirmed.
#[derive(Debug)]
pub struct OutputWriteError {
    pub kind: OutputFailureKind,
    /// The staged output file before publication; never the committed destination.
    pub partial_temp_path: Option<PathBuf>,
    /// A retained path that may need caller-directed recovery after rollback could not complete.
    pub recovery_path: Option<PathBuf>,
    source: Option<io::Error>,
}

impl OutputWriteError {
    fn new(
        kind: OutputFailureKind,
        partial_temp_path: Option<PathBuf>,
        source: Option<io::Error>,
    ) -> Self {
        Self {
            kind,
            partial_temp_path,
            recovery_path: None,
            source,
        }
    }

    pub fn io_source(&self) -> Option<&io::Error> {
        self.source.as_ref()
    }
}

impl core::fmt::Display for OutputWriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.source {
            Some(source) => write!(f, "output transaction {:?} failed: {source}", self.kind),
            None => write!(f, "output transaction {:?} failed", self.kind),
        }
    }
}

impl std::error::Error for OutputWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

#[derive(Debug, Default)]
struct OutputRegistryState {
    valid: HashMap<CacheIdentity, RegisteredOutput>,
}

#[derive(Debug)]
struct RegisteredOutput {
    path: PathBuf,
    canonical_path: PathBuf,
    file_identity: Option<OutputFileIdentity>,
}

/// Valid-output registry. A key is inserted only after its staged file has been flushed, synced,
/// committed at the destination, and the destination parent has been synced. Transactions for
/// this registry are serialized; writer callbacks must not re-enter the same registry.
#[derive(Clone, Debug, Default)]
pub struct OutputRegistry {
    state: Arc<Mutex<OutputRegistryState>>,
}

struct OutputTransactionHooks<S, R, D> {
    sync_file: S,
    replace: R,
    sync_parent_directory: D,
}

impl OutputRegistry {
    pub fn registered_path(&self, identity: &CacheIdentity) -> Option<PathBuf> {
        lock_unpoisoned(&self.state)
            .valid
            .get(identity)
            .map(|output| output.path.clone())
    }

    /// Writes into a unique sibling temporary file. Unix and Windows atomically replace an
    /// existing destination; other targets commit with no-clobber creation and reject replacement.
    /// Existing destinations are backed up by a same-directory hard link when supported.
    /// Symbolic-link destinations are rejected before publication because hard-link behavior for
    /// symlinks differs across platforms. Relative destinations are resolved to an absolute path
    /// once at transaction start, and that captured path is used for staging, verification,
    /// commit, errors, and registration. Registration follows parent-directory sync and backup
    /// cleanup. If sync fails after publication, the previous destination is restored (or a new
    /// destination is removed); `partial_temp_path` never points at the published file.
    ///
    /// The stage handle stays open and its file identity is checked before the path-based commit.
    /// Unix/Windows use stable file identifiers; other targets compare available file metadata.
    /// This detects substitutions before that check, but std has no portable rename-by-handle API;
    /// therefore the destination parent must not be writable by an untrusted concurrent actor
    /// during a transaction (a check-to-rename race remains otherwise).
    pub fn write_transactional<F>(
        &self,
        identity: CacheIdentity,
        destination: impl AsRef<Path>,
        reservation: &DiskReservation,
        writer: F,
    ) -> Result<PathBuf, OutputWriteError>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        self.write_transactional_with_sync(
            identity,
            destination.as_ref(),
            reservation,
            writer,
            OutputTransactionHooks {
                sync_file: File::sync_all,
                replace: replace_destination,
                sync_parent_directory: crate::storage::sync_parent_directory,
            },
        )
    }

    fn write_transactional_with_sync<F, S, R, D>(
        &self,
        identity: CacheIdentity,
        destination: &Path,
        reservation: &DiskReservation,
        writer: F,
        hooks: OutputTransactionHooks<S, R, D>,
    ) -> Result<PathBuf, OutputWriteError>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
        S: FnOnce(&File) -> io::Result<()>,
        R: FnMut(&Path, &Path) -> io::Result<()>,
        D: FnMut(&Path) -> io::Result<()>,
    {
        let OutputTransactionHooks {
            sync_file,
            mut replace,
            mut sync_parent_directory,
        } = hooks;
        let mut registry = lock_unpoisoned(&self.state);
        let destination = absolute_output_destination(destination).map_err(|error| {
            OutputWriteError::new(OutputFailureKind::TemporaryCreate, None, Some(error))
        })?;
        let canonical_destination =
            canonical_output_destination(&destination).map_err(|error| {
                OutputWriteError::new(OutputFailureKind::TemporaryCreate, None, Some(error))
            })?;
        let (temp_path, mut file) = create_output_temp(&destination).map_err(|error| {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::TemporaryCreate
            };
            OutputWriteError::new(kind, None, Some(error))
        })?;
        let (write_result, limit_exceeded, underlying_error) = {
            let mut limited = LimitedWriter::new(&mut file, reservation);
            let write_result = writer(&mut limited);
            (
                write_result,
                limited.limit_exceeded,
                limited.underlying_error,
            )
        };
        if let Some(error) = underlying_error {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Writer
            };
            return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
        }
        if limit_exceeded {
            return Err(OutputWriteError::new(
                OutputFailureKind::OutputLimitExceeded,
                Some(temp_path),
                None,
            ));
        }
        if let Err(error) = write_result {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Writer
            };
            return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
        }
        if let Err(error) = file.flush() {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Flush
            };
            return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
        }
        if let Err(error) = sync_file(&file) {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Sync
            };
            return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
        }
        if let Err(error) = verify_output_temp(&temp_path, &file) {
            return Err(OutputWriteError::new(
                OutputFailureKind::Commit,
                Some(temp_path),
                Some(error),
            ));
        }
        let previous_destination_identity = existing_output_file_identity(&destination);
        let destination_parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let backup_path = match create_output_backup(&destination) {
            Ok(backup_path) => backup_path,
            Err(error) => {
                let kind = if is_disk_full(&error) {
                    OutputFailureKind::DiskFull
                } else {
                    OutputFailureKind::Commit
                };
                return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
            }
        };
        if let Some(backup_path) = backup_path.as_ref()
            && let Err(error) = sync_parent_directory(destination_parent)
        {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Sync
            };
            let recovery_path = if fs::remove_file(backup_path).is_err() && backup_path.exists() {
                Some(backup_path.clone())
            } else {
                None
            };
            let mut output_error = OutputWriteError::new(kind, Some(temp_path), Some(error));
            output_error.recovery_path = recovery_path;
            return Err(output_error);
        }
        if let Err(error) = replace(&temp_path, &destination) {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Commit
            };
            let recovery_path = backup_path.as_ref().and_then(|backup_path| {
                if fs::remove_file(backup_path).is_err() && backup_path.exists() {
                    Some(backup_path.clone())
                } else {
                    None
                }
            });
            let mut output_error = OutputWriteError::new(kind, Some(temp_path), Some(error));
            output_error.recovery_path = recovery_path;
            return Err(output_error);
        }
        let committed_file_identity = output_file_identity_from_file(&file).ok().flatten();
        drop(file);
        let committed_path = destination.clone();
        let committed_canonical_destination = canonical_destination;
        if let Err(error) = sync_parent_directory(destination_parent) {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Sync
            };
            if let Err((rollback_error, destination_restored)) = rollback_published_output(
                &destination,
                destination_parent,
                backup_path.as_deref(),
                &mut replace,
                &mut sync_parent_directory,
            ) {
                if !destination_restored {
                    registry.valid.retain(|_, output| {
                        !same_output_destination(
                            &output.canonical_path,
                            &committed_canonical_destination,
                        ) && !previous_destination_identity.is_some_and(|file_identity| {
                            output.file_identity == Some(file_identity)
                        })
                    });
                }
                let recovery_path = output_recovery_path(backup_path.as_deref(), &destination);
                let source = io::Error::new(
                    error.kind(),
                    format!(
                        "parent-directory sync failed: {error}; rollback failed: {rollback_error}"
                    ),
                );
                let mut output_error = OutputWriteError::new(kind, None, Some(source));
                output_error.recovery_path = recovery_path;
                return Err(output_error);
            }
            return Err(OutputWriteError::new(kind, None, Some(error)));
        }
        if let Some(backup_path) = backup_path.as_ref() {
            if let Err(error) = fs::remove_file(backup_path) {
                let kind = if is_disk_full(&error) {
                    OutputFailureKind::DiskFull
                } else {
                    OutputFailureKind::Commit
                };
                if let Err((rollback_error, destination_restored)) = rollback_published_output(
                    &destination,
                    destination_parent,
                    Some(backup_path),
                    &mut replace,
                    &mut sync_parent_directory,
                ) {
                    if !destination_restored {
                        registry.valid.retain(|_, output| {
                            !same_output_destination(
                                &output.canonical_path,
                                &committed_canonical_destination,
                            ) && !previous_destination_identity.is_some_and(|file_identity| {
                                output.file_identity == Some(file_identity)
                            })
                        });
                    }
                    let recovery_path = output_recovery_path(Some(backup_path), &destination);
                    let source = io::Error::new(
                        error.kind(),
                        format!(
                            "output backup cleanup failed: {error}; rollback failed: {rollback_error}"
                        ),
                    );
                    let mut output_error = OutputWriteError::new(kind, None, Some(source));
                    output_error.recovery_path = recovery_path;
                    return Err(output_error);
                }
                return Err(OutputWriteError::new(kind, None, Some(error)));
            }
            let _ = sync_parent_directory(destination_parent);
        }
        registry.valid.retain(|_, output| {
            !same_output_destination(&output.canonical_path, &committed_canonical_destination)
                && !previous_destination_identity
                    .is_some_and(|file_identity| output.file_identity == Some(file_identity))
        });
        registry.valid.insert(
            identity,
            RegisteredOutput {
                path: committed_path.clone(),
                canonical_path: committed_canonical_destination,
                file_identity: committed_file_identity,
            },
        );
        Ok(committed_path)
    }
}

fn rollback_published_output<R, D>(
    destination: &Path,
    destination_parent: &Path,
    backup_path: Option<&Path>,
    replace: &mut R,
    sync_parent_directory: &mut D,
) -> Result<(), (io::Error, bool)>
where
    R: FnMut(&Path, &Path) -> io::Result<()>,
    D: FnMut(&Path) -> io::Result<()>,
{
    let restore_result = match backup_path {
        Some(backup_path) => replace(backup_path, destination),
        None => fs::remove_file(destination),
    };
    if let Err(error) = restore_result {
        return Err((error, false));
    }
    if let Err(error) = sync_parent_directory(destination_parent) {
        return Err((error, true));
    }
    Ok(())
}

fn output_recovery_path(backup_path: Option<&Path>, destination: &Path) -> Option<PathBuf> {
    backup_path
        .filter(|path| fs::symlink_metadata(path).is_ok())
        .or_else(|| {
            fs::symlink_metadata(destination)
                .is_ok()
                .then_some(destination)
        })
        .map(Path::to_path_buf)
}

struct LimitedWriter<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    reservation: &'a DiskReservation,
    limit_exceeded: bool,
    underlying_error: Option<io::Error>,
}

impl<'a, W: Write + ?Sized> LimitedWriter<'a, W> {
    fn new(inner: &'a mut W, reservation: &'a DiskReservation) -> Self {
        Self {
            inner,
            reservation,
            limit_exceeded: false,
            underlying_error: None,
        }
    }

    fn remember_io_error(&mut self, error: io::Error) -> io::Error {
        if self.underlying_error.is_some() {
            return error;
        }
        let returned_error = match error.raw_os_error() {
            Some(code) => io::Error::from_raw_os_error(code),
            None => io::Error::new(error.kind(), error.to_string()),
        };
        self.underlying_error = Some(error);
        returned_error
    }
}

impl<W: Write + ?Sized> Write for LimitedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = match u64::try_from(bytes.len()) {
            Ok(count) => count,
            Err(_) => {
                let error = io::Error::other("write length overflow");
                return Err(self.remember_io_error(error));
            }
        };
        let mut output_written = lock_unpoisoned(&self.reservation.output_written_bytes);
        let remaining = self
            .reservation
            .requirements
            .output_bytes
            .checked_sub(*output_written);
        let Some(remaining) = remaining else {
            self.limit_exceeded = true;
            return Err(io::Error::other("reserved output capacity exceeded"));
        };
        if count > remaining {
            self.limit_exceeded = true;
            return Err(io::Error::other("reserved output capacity exceeded"));
        }
        let written = match self.inner.write(bytes) {
            Ok(0) if !bytes.is_empty() => {
                let error = io::Error::new(ErrorKind::WriteZero, "output writer made no progress");
                drop(output_written);
                return Err(self.remember_io_error(error));
            }
            Ok(written) => written,
            Err(error) => {
                drop(output_written);
                return Err(self.remember_io_error(error));
            }
        };
        let written_bytes = match u64::try_from(written) {
            Ok(written_bytes) => written_bytes,
            Err(_) => {
                let error = io::Error::other("write length overflow");
                drop(output_written);
                return Err(self.remember_io_error(error));
            }
        };
        if let Err(error) = self
            .reservation
            .record_output_bytes(&mut output_written, written_bytes)
        {
            drop(output_written);
            return Err(self.remember_io_error(error));
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.inner.flush() {
            Ok(()) => Ok(()),
            Err(error) => Err(self.remember_io_error(error)),
        }
    }
}

fn is_disk_full(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded
    ) {
        return true;
    }
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    let raw_error = error.raw_os_error();

    #[cfg(windows)]
    {
        matches!(raw_error, Some(39 | 112))
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        matches!(raw_error, Some(28 | 122))
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        matches!(raw_error, Some(28 | 69))
    }
    #[cfg(not(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OutputFileIdentity {
    volume: u64,
    file: u64,
}

#[cfg(unix)]
fn output_file_identity_from_file(file: &File) -> io::Result<Option<OutputFileIdentity>> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    Ok(Some(OutputFileIdentity {
        volume: metadata.dev(),
        file: metadata.ino(),
    }))
}

#[cfg(windows)]
fn output_file_identity_from_file(file: &File) -> io::Result<Option<OutputFileIdentity>> {
    let (volume, file) = windows_file_identity(file)?;
    Ok(Some(OutputFileIdentity {
        volume: u64::from(volume),
        file,
    }))
}

#[cfg(not(any(unix, windows)))]
fn output_file_identity_from_file(_file: &File) -> io::Result<Option<OutputFileIdentity>> {
    Ok(None)
}

fn existing_output_file_identity(path: &Path) -> Option<OutputFileIdentity> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        Some(OutputFileIdentity {
            volume: metadata.dev(),
            file: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        let file = OpenOptions::new().read(true).open(path).ok()?;
        let (volume, file) = windows_file_identity(&file).ok()?;
        Some(OutputFileIdentity {
            volume: u64::from(volume),
            file,
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

fn canonical_output_destination(destination: &Path) -> io::Result<PathBuf> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "destination has no file name"))?;
    let canonical_parent = fs::canonicalize(parent)?;
    #[cfg(windows)]
    if let Ok(metadata) = fs::symlink_metadata(destination)
        && metadata.file_type().is_file()
        && let Ok(canonical_destination) = fs::canonicalize(destination)
    {
        return Ok(canonical_destination);
    }
    Ok(canonical_parent.join(file_name))
}

fn absolute_output_destination(destination: &Path) -> io::Result<PathBuf> {
    if destination.is_absolute() {
        return Ok(destination.to_path_buf());
    }
    #[cfg(windows)]
    if windows_is_drive_relative_destination(destination) {
        return windows_absolute_drive_relative_destination(destination);
    }
    Ok(std::env::current_dir()?.join(destination))
}

#[cfg(windows)]
fn windows_is_drive_relative_destination(destination: &Path) -> bool {
    matches!(
        destination.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_))
    ) && !destination.has_root()
}

#[cfg(windows)]
fn windows_absolute_drive_relative_destination(destination: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFullPathNameW(
            file_name: *const u16,
            buffer_length: u32,
            buffer: *mut u16,
            file_part: *mut *mut u16,
        ) -> u32;
    }

    let mut wide_destination: Vec<u16> = destination.as_os_str().encode_wide().collect();
    if wide_destination.contains(&0) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "drive-relative destination contains a NUL",
        ));
    }
    wide_destination.push(0);

    let mut buffer = vec![0u16; 260];
    loop {
        let buffer_length = u32::try_from(buffer.len()).map_err(|_| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "resolved destination path exceeds Windows API limits",
            )
        })?;
        // SAFETY: the input is NUL-terminated, the output buffer has buffer_length elements,
        // and the unused file-part output is null.
        let required_or_written = unsafe {
            GetFullPathNameW(
                wide_destination.as_ptr(),
                buffer_length,
                buffer.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        if required_or_written == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = required_or_written as usize;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)));
        }
        buffer
            .try_reserve_exact(length.saturating_sub(buffer.len()))
            .map_err(io::Error::other)?;
        buffer.resize(length, 0);
    }
}

#[cfg(unix)]
fn same_output_destination(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    // Resolve existing spellings through the actual filesystem instead of assuming Unix paths
    // are case-sensitive or folding their names unconditionally.
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
fn same_output_destination(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn same_output_destination(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let (Some(left_parent), Some(right_parent)) = (left.parent(), right.parent()) else {
        return false;
    };
    if left_parent != right_parent {
        return false;
    }
    let (Some(left_name), Some(right_name)) = (left.file_name(), right.file_name()) else {
        return false;
    };
    let case_sensitive = match windows_directory_case_sensitive(left_parent) {
        Ok(case_sensitive) => case_sensitive,
        Err(_) => {
            // Unknown is not evidence that aliases are safe to merge. Only match spellings that
            // the filesystem itself resolves to the same existing entry.
            return match (fs::canonicalize(left), fs::canonicalize(right)) {
                (Ok(left), Ok(right)) => left == right,
                _ => false,
            };
        }
    };
    if case_sensitive {
        left_name == right_name
    } else {
        windows_filename_eq_ordinal_ignore_case(left_name, right_name)
    }
}

#[cfg(windows)]
fn windows_directory_case_sensitive(directory: &Path) -> io::Result<bool> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};

    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const FILE_SHARE_READ: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0002;
    const FILE_SHARE_DELETE: u32 = 0x0004;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_CASE_SENSITIVE_INFO_CLASS: i32 = 23;
    const FILE_CS_FLAG_CASE_SENSITIVE_DIR: u32 = 0x0000_0001;

    #[repr(C)]
    #[derive(Default)]
    struct FileCaseSensitiveInfo {
        flags: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *const c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            file_information_class: i32,
            file_information: *mut c_void,
            buffer_size: u32,
        ) -> i32;
    }

    let directory_wide: Vec<u16> = directory
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: the path is NUL-terminated UTF-16 and the remaining arguments are valid
    // CreateFileW constants/null pointers for opening an existing directory.
    let handle = unsafe {
        CreateFileW(
            directory_wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == (-1isize as *mut c_void) {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a valid owned handle; File closes it on drop.
    let directory_file = unsafe { File::from_raw_handle(handle) };
    let mut information = FileCaseSensitiveInfo::default();
    // SAFETY: the handle is open for FILE_READ_ATTRIBUTES and information points to a
    // correctly sized FILE_CASE_SENSITIVE_INFO buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            directory_file.as_raw_handle(),
            FILE_CASE_SENSITIVE_INFO_CLASS,
            &mut information as *mut FileCaseSensitiveInfo as *mut c_void,
            std::mem::size_of::<FileCaseSensitiveInfo>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0)
}

#[cfg(windows)]
fn windows_filename_eq_ordinal_ignore_case(left: &OsStr, right: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CompareStringOrdinal(
            string1: *const u16,
            count1: i32,
            string2: *const u16,
            count2: i32,
            ignore_case: i32,
        ) -> i32;
    }

    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    let Ok(left_len) = i32::try_from(left.len()) else {
        return false;
    };
    let Ok(right_len) = i32::try_from(right.len()) else {
        return false;
    };
    unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == 2 }
}

#[cfg(not(windows))]
fn verify_output_temp(temp_path: &Path, file: &File) -> io::Result<()> {
    let path_metadata = fs::symlink_metadata(temp_path)?;
    if !path_metadata.file_type().is_file()
        || !same_file_identity(&file.metadata()?, &path_metadata)
    {
        return Err(io::Error::other(
            "staged output path no longer names the file opened for writing",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn same_file_identity(open_file: &fs::Metadata, path_file: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    open_file.dev() == path_file.dev() && open_file.ino() == path_file.ino()
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(open_file: &fs::Metadata, path_file: &fs::Metadata) -> bool {
    match (
        open_file.len() == path_file.len(),
        open_file.modified().ok(),
        path_file.modified().ok(),
        open_file.created().ok(),
        path_file.created().ok(),
    ) {
        (
            true,
            Some(open_modified),
            Some(path_modified),
            Some(open_created),
            Some(path_created),
        ) => open_modified == path_modified && open_created == path_created,
        _ => false,
    }
}

#[cfg(windows)]
fn verify_output_temp(temp_path: &Path, file: &File) -> io::Result<()> {
    let path_metadata = fs::symlink_metadata(temp_path)?;
    if !path_metadata.file_type().is_file() {
        return Err(io::Error::other(
            "staged output path no longer names a regular file",
        ));
    }
    let opened_identity = windows_file_identity(file)?;
    let staged_path_file = OpenOptions::new().read(true).open(temp_path)?;
    if opened_identity != windows_file_identity(&staged_path_file)? {
        return Err(io::Error::other(
            "staged output path no longer names the file opened for writing",
        ));
    }
    let final_metadata = fs::symlink_metadata(temp_path)?;
    if !final_metadata.file_type().is_file() {
        return Err(io::Error::other(
            "staged output path no longer names a regular file",
        ));
    }
    let final_path_file = OpenOptions::new().read(true).open(temp_path)?;
    if opened_identity != windows_file_identity(&final_path_file)? {
        return Err(io::Error::other(
            "staged output path no longer names the file opened for writing",
        ));
    }
    if !fs::symlink_metadata(temp_path)?.file_type().is_file() {
        return Err(io::Error::other(
            "staged output path no longer names a regular file",
        ));
    }
    Ok(())
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct WindowsFileTime {
    low_date_time: u32,
    high_date_time: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct WindowsByHandleFileInformation {
    file_attributes: u32,
    creation_time: WindowsFileTime,
    last_access_time: WindowsFileTime,
    last_write_time: WindowsFileTime,
    volume_serial_number: u32,
    file_size_high: u32,
    file_size_low: u32,
    number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> io::Result<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            file: *mut std::ffi::c_void,
            information: *mut WindowsByHandleFileInformation,
        ) -> i32;
    }

    let mut information = WindowsByHandleFileInformation::default();
    let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let file_index =
        (u64::from(information.file_index_high) << 32) | u64::from(information.file_index_low);
    Ok((information.volume_serial_number, file_index))
}

static OUTPUT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn create_output_temp(destination: &Path) -> io::Result<(PathBuf, File)> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut attempts = 0u16;
    loop {
        let id = OUTPUT_TEMP_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| io::Error::other("temporary output identifier exhausted"))?;
        let temp_name = format!(".asciiwall-output-{}-{id:016x}.partial", std::process::id());
        if destination.file_name() == Some(OsStr::new(&temp_name)) {
            continue;
        }
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                attempts = attempts
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("temporary output name exhausted"))?;
                if attempts >= 256 {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}
fn create_output_backup(destination: &Path) -> io::Result<Option<PathBuf>> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "symbolic-link output destinations are not supported",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for _ in 0..256 {
        let id = OUTPUT_TEMP_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| io::Error::other("temporary output identifier exhausted"))?;
        let backup_name = format!(".asciiwall-output-{}-{id:016x}.backup", std::process::id());
        if destination.file_name() == Some(OsStr::new(&backup_name)) {
            continue;
        }
        let backup_path = parent.join(backup_name);
        match fs::hard_link(destination, &backup_path) {
            Ok(()) => return Ok(Some(backup_path)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "temporary output backup name exhausted",
    ))
}

#[cfg(unix)]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp_path, destination)
}

#[cfg(windows)]
fn move_file_write_through(
    existing_path: &Path,
    new_path: &Path,
    replace_existing: bool,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let existing: Vec<u16> = existing_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let new: Vec<u16> = new_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // MOVEFILE_WRITE_THROUGH, with optional MOVEFILE_REPLACE_EXISTING.
    let flags = 0x8 | if replace_existing { 0x1 } else { 0 };
    let success = unsafe { MoveFileExW(existing.as_ptr(), new.as_ptr(), flags) };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    move_file_write_through(temp_path, destination, true)
}

#[cfg(not(any(unix, windows)))]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    replace_destination_portable(temp_path, destination)
}

#[cfg(any(not(any(unix, windows)), test))]
fn replace_destination_portable(temp_path: &Path, destination: &Path) -> io::Result<()> {
    // A hard link commits only if the destination is absent, so unsupported targets never clobber.
    fs::hard_link(temp_path, destination)?;
    let _ = fs::remove_file(temp_path);
    Ok(())
}
fn replace_file_id_high_water(temp_path: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(unix, windows))]
    {
        replace_destination(temp_path, destination)
    }

    #[cfg(not(any(unix, windows)))]
    {
        replace_file_id_high_water_portable(temp_path, destination)
    }
}

#[cfg(any(not(any(unix, windows)), test))]
fn replace_file_id_high_water_portable(temp_path: &Path, destination: &Path) -> io::Result<()> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    recover_file_id_high_water_backup_portable(parent)?;

    let backup_path = parent.join(CACHE_FILE_ID_HIGH_WATER_BACKUP);
    match fs::rename(destination, &backup_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            replace_destination_portable(temp_path, destination)?;
            return crate::storage::sync_parent_directory(parent);
        }
        Err(error) => return Err(error),
    }
    if let Err(error) = crate::storage::sync_parent_directory(parent) {
        let _ = restore_file_id_high_water_backup(parent, destination, &backup_path, false);
        return Err(error);
    }

    if let Err(error) = replace_destination_portable(temp_path, destination) {
        let _ = restore_file_id_high_water_backup(parent, destination, &backup_path, false);
        return Err(error);
    }
    crate::storage::sync_parent_directory(parent)?;
    fs::remove_file(&backup_path)?;
    crate::storage::sync_parent_directory(parent)
}

fn restore_file_id_high_water_backup(
    managed_dir: &Path,
    counter_path: &Path,
    backup_path: &Path,
    remove_counter: bool,
) -> io::Result<()> {
    #[cfg(windows)]
    {
        let _ = (managed_dir, remove_counter);
        replace_destination(backup_path, counter_path)
    }

    #[cfg(not(windows))]
    {
        if remove_counter {
            fs::remove_file(counter_path)?;
            crate::storage::sync_parent_directory(managed_dir)?;
        }
        fs::hard_link(backup_path, counter_path)?;
        crate::storage::sync_parent_directory(managed_dir)?;
        fs::remove_file(backup_path)?;
        crate::storage::sync_parent_directory(managed_dir)
    }
}
fn recover_file_id_high_water_backup(managed_dir: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        recover_file_id_high_water_backup_windows(managed_dir)
    }

    #[cfg(not(windows))]
    {
        recover_file_id_high_water_backup_portable(managed_dir)
    }
}

#[cfg(windows)]
fn recover_file_id_high_water_backup_windows(managed_dir: &Path) -> io::Result<()> {
    let backup_path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER_BACKUP);
    let counter_path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER);
    let backup = high_water_file_state(&backup_path)?;
    if backup == HighWaterFileState::Missing {
        return Ok(());
    }
    let counter = high_water_file_state(&counter_path)?;

    match (counter, backup) {
        (_, HighWaterFileState::Missing) => Ok(()),
        (HighWaterFileState::Valid(current), HighWaterFileState::Valid(previous))
            if current >= previous =>
        {
            // The counter is authoritative; leave a stale backup rather than perform a
            // non-write-through deletion. This single reserved path cannot accumulate files.
            Ok(())
        }
        (HighWaterFileState::Valid(_), HighWaterFileState::Corrupt) => Ok(()),
        (HighWaterFileState::Missing, HighWaterFileState::Valid(_))
        | (HighWaterFileState::Corrupt, HighWaterFileState::Valid(_))
        | (HighWaterFileState::Valid(_), HighWaterFileState::Valid(_)) => {
            restore_file_id_high_water_backup(managed_dir, &counter_path, &backup_path, true)
        }
        (HighWaterFileState::Missing, HighWaterFileState::Corrupt)
        | (HighWaterFileState::Corrupt, HighWaterFileState::Corrupt) => {
            Err(invalid_file_id_high_water_data())
        }
    }
}

#[cfg(any(not(windows), test))]
fn recover_file_id_high_water_backup_portable(managed_dir: &Path) -> io::Result<()> {
    let backup_path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER_BACKUP);
    let counter_path = managed_dir.join(CACHE_FILE_ID_HIGH_WATER);
    let backup = high_water_file_state(&backup_path)?;
    if backup == HighWaterFileState::Missing {
        return Ok(());
    }
    let counter = high_water_file_state(&counter_path)?;

    match (counter, backup) {
        (_, HighWaterFileState::Missing) => Ok(()),
        (HighWaterFileState::Valid(current), HighWaterFileState::Valid(previous))
            if current >= previous =>
        {
            crate::storage::sync_parent_directory(managed_dir)?;
            fs::remove_file(&backup_path)?;
            crate::storage::sync_parent_directory(managed_dir)
        }
        (HighWaterFileState::Valid(_), HighWaterFileState::Corrupt) => {
            crate::storage::sync_parent_directory(managed_dir)?;
            fs::remove_file(&backup_path)?;
            crate::storage::sync_parent_directory(managed_dir)
        }
        (HighWaterFileState::Missing, HighWaterFileState::Valid(_)) => {
            restore_file_id_high_water_backup(managed_dir, &counter_path, &backup_path, false)
        }
        (HighWaterFileState::Corrupt, HighWaterFileState::Valid(_))
        | (HighWaterFileState::Valid(_), HighWaterFileState::Valid(_)) => {
            restore_file_id_high_water_backup(managed_dir, &counter_path, &backup_path, true)
        }
        (HighWaterFileState::Missing, HighWaterFileState::Corrupt)
        | (HighWaterFileState::Corrupt, HighWaterFileState::Corrupt) => {
            Err(invalid_file_id_high_water_data())
        }
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootOperationTestPoint {
    BeforeAcquire,
    Acquired,
}

#[cfg(test)]
type RootOperationTestHook = Box<dyn FnMut(RootOperationTestPoint, &Mutex<()>) + Send>;

#[cfg(test)]
thread_local! {
    static ROOT_OPERATION_TEST_HOOK: std::cell::RefCell<Option<RootOperationTestHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_root_operation_test_hook(
    hook: impl FnMut(RootOperationTestPoint, &Mutex<()>) + Send + 'static,
) {
    ROOT_OPERATION_TEST_HOOK.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(hook)).is_none());
    });
}

#[cfg(test)]
fn notify_root_operation_test_hook(point: RootOperationTestPoint, lock: &Mutex<()>) {
    ROOT_OPERATION_TEST_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(point, lock);
        }
    });
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvictionTestPoint {
    CandidatesSnapshotted(usize),
    CandidateExamined,
}

#[cfg(test)]
type EvictionTestHook = Box<dyn FnMut(EvictionTestPoint) + Send>;

#[cfg(test)]
thread_local! {
    static EVICTION_TEST_HOOK: std::cell::RefCell<Option<EvictionTestHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct EvictionTestHookReset;

#[cfg(test)]
impl Drop for EvictionTestHookReset {
    fn drop(&mut self) {
        EVICTION_TEST_HOOK.with(|slot| {
            drop(slot.borrow_mut().take());
        });
    }
}

#[cfg(test)]
fn set_eviction_test_hook(
    hook: impl FnMut(EvictionTestPoint) + Send + 'static,
) -> EvictionTestHookReset {
    EVICTION_TEST_HOOK.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(hook)).is_none());
    });
    EvictionTestHookReset
}

#[cfg(test)]
fn notify_eviction_test_hook(point: EvictionTestPoint) {
    EVICTION_TEST_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(point);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempManagedDirectory(std::path::PathBuf);

    impl TempManagedDirectory {
        fn new() -> Self {
            loop {
                let id = CACHE_INITIALIZATION_TEMP_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir()
                    .join(format!("asciiwall-cache-sync-{}-{id}", std::process::id()));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test cache directory: {error}"),
                }
            }
        }
    }

    impl Drop for TempManagedDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cache_instance_publication_syncs_staging_before_root() {
        use std::cell::RefCell;

        let directory = TempManagedDirectory::new();
        let root = fs::canonicalize(&directory.0).unwrap();
        let managed_dir = root.join(CACHE_INSTANCE_DIR);
        let sync_events = RefCell::new(Vec::with_capacity(2));
        let created = create_cache_instance_dir_with_sync(&root, |path| {
            if path == root.as_path() {
                assert_eq!(
                    fs::read(managed_dir.join(CACHE_OWNER_FILE))?,
                    CACHE_OWNER_MARKER
                );
                sync_events.borrow_mut().push("root");
            } else {
                assert_eq!(path.parent(), Some(root.as_path()));
                assert!(!managed_dir.exists());
                assert_eq!(fs::read(path.join(CACHE_OWNER_FILE))?, CACHE_OWNER_MARKER);
                sync_events.borrow_mut().push("staging");
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(created, managed_dir);
        assert_eq!(sync_events.into_inner(), vec!["staging", "root"]);
    }

    #[test]
    fn cache_instance_staging_sync_failure_prevents_publication() {
        let directory = TempManagedDirectory::new();
        let root = fs::canonicalize(&directory.0).unwrap();
        let managed_dir = root.join(CACHE_INSTANCE_DIR);
        let error = create_cache_instance_dir_with_sync(&root, |_| {
            Err(io::Error::other("injected staging-directory sync failure"))
        })
        .unwrap_err();

        assert!(matches!(error, CacheError::Io(_)));
        assert!(!managed_dir.exists());
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{CACHE_INSTANCE_DIR}.init-"))
        }));
    }

    #[test]
    fn cache_instance_root_sync_failure_is_reported_after_publication() {
        use std::cell::RefCell;

        let directory = TempManagedDirectory::new();
        let root = fs::canonicalize(&directory.0).unwrap();
        let managed_dir = root.join(CACHE_INSTANCE_DIR);
        let sync_events = RefCell::new(Vec::with_capacity(2));
        let error = create_cache_instance_dir_with_sync(&root, |path| {
            if path == root.as_path() {
                assert!(cache_instance_is_owned(&root, &managed_dir).unwrap());
                sync_events.borrow_mut().push("root");
                Err(io::Error::other("injected root-directory sync failure"))
            } else {
                assert!(!managed_dir.exists());
                sync_events.borrow_mut().push("staging");
                Ok(())
            }
        })
        .unwrap_err();

        assert!(matches!(error, CacheError::Io(_)));
        assert_eq!(sync_events.into_inner(), vec!["staging", "root"]);
        assert!(cache_instance_is_owned(&root, &managed_dir).unwrap());
    }

    #[test]
    fn portable_high_water_replacement_updates_existing_counter_and_recovers_backup() {
        let managed_dir = TempManagedDirectory::new();
        let destination = managed_dir.0.join(CACHE_FILE_ID_HIGH_WATER);
        let temporary = managed_dir.0.join("counter-update");
        fs::write(&destination, b"0000000000000007\n").unwrap();
        fs::write(&temporary, b"0000000000000008\n").unwrap();

        replace_file_id_high_water_portable(&temporary, &destination).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"0000000000000008\n");
        assert!(!temporary.exists());
        assert!(!managed_dir.0.join(CACHE_FILE_ID_HIGH_WATER_BACKUP).exists());

        let backup = managed_dir.0.join(CACHE_FILE_ID_HIGH_WATER_BACKUP);
        fs::rename(&destination, &backup).unwrap();
        recover_file_id_high_water_backup_portable(&managed_dir.0).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"0000000000000008\n");
        assert!(!backup.exists());

        fs::write(&backup, b"0000000000000007\n").unwrap();
        recover_file_id_high_water_backup_portable(&managed_dir.0).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"0000000000000008\n");
        assert!(!backup.exists());

        fs::write(&backup, b"0000000000000009\n").unwrap();
        recover_file_id_high_water_backup_portable(&managed_dir.0).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"0000000000000009\n");
        assert!(!backup.exists());

        fs::write(&backup, b"000000000000000a\n").unwrap();
        fs::write(&destination, b"partial").unwrap();
        recover_file_id_high_water_backup_portable(&managed_dir.0).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"000000000000000a\n");
        assert!(!backup.exists());

        fs::write(&backup, b"corrupt").unwrap();
        fs::write(&destination, b"also-corrupt").unwrap();
        let error = recover_file_id_high_water_backup_portable(&managed_dir.0).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(destination.exists());
        assert!(backup.exists());
    }

    #[test]
    fn portable_output_publication_never_clobbers_an_existing_destination() {
        let directory = TempManagedDirectory::new();
        let destination = directory.0.join("output.bin");
        let temporary = directory.0.join("output-update");
        fs::write(&destination, b"existing output").unwrap();
        fs::write(&temporary, b"replacement output").unwrap();

        let error = replace_destination_portable(&temporary, &destination).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"existing output");
        assert_eq!(fs::read(&temporary).unwrap(), b"replacement output");

        fs::remove_file(&destination).unwrap();
        replace_destination_portable(&temporary, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"replacement output");
        assert!(!temporary.exists());
    }

    #[test]
    fn failed_initial_high_water_partial_write_does_not_publish_counter() {
        use std::cell::RefCell;

        let managed_dir = TempManagedDirectory::new();
        let events = RefCell::new(Vec::with_capacity(4));
        let error = write_file_id_high_water_with_sync(
            &managed_dir.0,
            7,
            true,
            HighWaterWriteHooks {
                write_encoded: |file: &mut File, encoded: &[u8]| {
                    events.borrow_mut().push("write");
                    file.write_all(&encoded[..5])?;
                    Err(io::Error::other(
                        "injected partial high-water write failure",
                    ))
                },
                sync_file: |_: &File| {
                    events.borrow_mut().push("file sync");
                    Ok(())
                },
                replace: |_: &Path, _: &Path| {
                    events.borrow_mut().push("replace");
                    Ok(())
                },
                sync_parent_directory: |_: &Path| {
                    events.borrow_mut().push("parent sync");
                    Ok(())
                },
            },
        )
        .unwrap_err();

        assert!(matches!(error, CacheError::Io(_)));
        assert_eq!(events.into_inner(), vec!["write"]);
        assert!(!managed_dir.0.join(CACHE_FILE_ID_HIGH_WATER).exists());
        assert!(fs::read_dir(&managed_dir.0).unwrap().next().is_none());
    }

    #[test]
    fn high_water_counter_and_marker_sync_their_parent_after_file_sync() {
        use std::cell::RefCell;

        #[derive(Debug, Eq, PartialEq)]
        enum SyncEvent {
            File,
            Replace,
            ParentDirectory,
        }

        let managed_dir = TempManagedDirectory::new();
        let counter_events = RefCell::new(Vec::with_capacity(2));
        write_file_id_high_water_with_sync(
            &managed_dir.0,
            7,
            true,
            HighWaterWriteHooks {
                write_encoded: |file: &mut File, encoded: &[u8]| file.write_all(encoded),
                sync_file: |_: &File| {
                    counter_events.borrow_mut().push(SyncEvent::File);
                    Ok(())
                },
                replace: |_: &Path, _: &Path| {
                    counter_events.borrow_mut().push(SyncEvent::Replace);
                    Ok(())
                },
                sync_parent_directory: |directory: &Path| {
                    assert_eq!(directory, managed_dir.0.as_path());
                    assert_eq!(
                        fs::read(directory.join(CACHE_FILE_ID_HIGH_WATER))?,
                        b"0000000000000007\n"
                    );
                    counter_events.borrow_mut().push(SyncEvent::ParentDirectory);
                    Ok(())
                },
            },
        )
        .unwrap();
        assert_eq!(
            counter_events.into_inner(),
            vec![SyncEvent::File, SyncEvent::ParentDirectory]
        );

        let update_events = RefCell::new(Vec::with_capacity(3));
        write_file_id_high_water_with_sync(
            &managed_dir.0,
            8,
            false,
            HighWaterWriteHooks {
                write_encoded: |file: &mut File, encoded: &[u8]| file.write_all(encoded),
                sync_file: |_: &File| {
                    update_events.borrow_mut().push(SyncEvent::File);
                    Ok(())
                },
                replace: |temporary: &Path, destination: &Path| {
                    let expected_destination = managed_dir.0.join(CACHE_FILE_ID_HIGH_WATER);
                    assert_eq!(destination, expected_destination.as_path());
                    update_events.borrow_mut().push(SyncEvent::Replace);
                    replace_file_id_high_water(temporary, destination)
                },
                sync_parent_directory: |directory: &Path| {
                    assert_eq!(directory, managed_dir.0.as_path());
                    assert_eq!(
                        fs::read(directory.join(CACHE_FILE_ID_HIGH_WATER))?,
                        b"0000000000000008\n"
                    );
                    update_events.borrow_mut().push(SyncEvent::ParentDirectory);
                    Ok(())
                },
            },
        )
        .unwrap();
        assert_eq!(
            update_events.into_inner(),
            vec![
                SyncEvent::File,
                SyncEvent::Replace,
                SyncEvent::ParentDirectory
            ]
        );
        assert_eq!(read_file_id_high_water(&managed_dir.0).unwrap(), 8);

        let failed_update_events = RefCell::new(Vec::with_capacity(2));
        let mut failed_temp_path = None;
        let error = write_file_id_high_water_with_sync(
            &managed_dir.0,
            9,
            false,
            HighWaterWriteHooks {
                write_encoded: |file: &mut File, encoded: &[u8]| file.write_all(encoded),
                sync_file: |_: &File| {
                    failed_update_events.borrow_mut().push(SyncEvent::File);
                    Ok(())
                },
                replace: |temporary: &Path, _: &Path| {
                    failed_temp_path = Some(temporary.to_path_buf());
                    failed_update_events.borrow_mut().push(SyncEvent::Replace);
                    Err(io::Error::other("injected high-water replacement failure"))
                },
                sync_parent_directory: |_: &Path| {
                    failed_update_events
                        .borrow_mut()
                        .push(SyncEvent::ParentDirectory);
                    Ok(())
                },
            },
        )
        .unwrap_err();
        assert!(matches!(error, CacheError::Io(_)));
        assert_eq!(
            failed_update_events.into_inner(),
            vec![SyncEvent::File, SyncEvent::Replace]
        );
        assert_eq!(read_file_id_high_water(&managed_dir.0).unwrap(), 8);
        assert!(
            !failed_temp_path
                .expect("the staged path is captured")
                .exists()
        );

        let marker_events = RefCell::new(Vec::with_capacity(2));
        mark_file_id_high_water_initialized_with_sync(
            &managed_dir.0,
            |_| {
                marker_events.borrow_mut().push(SyncEvent::File);
                Ok(())
            },
            |directory| {
                assert_eq!(directory, managed_dir.0.as_path());
                assert_eq!(
                    fs::symlink_metadata(directory.join(CACHE_FILE_ID_HIGH_WATER_MARKER))?.len(),
                    0
                );
                marker_events.borrow_mut().push(SyncEvent::ParentDirectory);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            marker_events.into_inner(),
            vec![SyncEvent::File, SyncEvent::ParentDirectory]
        );
    }

    #[test]
    fn transactional_output_syncs_parent_after_publish_before_registration() {
        use std::cell::{Cell, RefCell};

        #[derive(Debug, Eq, PartialEq)]
        enum CommitEvent {
            FileSync,
            Replace,
            ParentDirectorySync,
        }

        let identity = |digest: u8| {
            CacheIdentity::Conversion(ConversionKey {
                source: SourceContent::Missing,
                decode_version: "decode-v1".into(),
                orientation_version: "orientation-v1".into(),
                color_version: "color-v1".into(),
                algorithm_version: "algorithm-v1".into(),
                font_digest: ContentDigest([0; 32]),
                font_raster_settings: FontRasterSettings::default(),
                glyphs: Vec::new(),
                grid: GridSize {
                    columns: 1,
                    rows: 1,
                },
                fit_crop: FitCrop {
                    mode: FitMode::Fit,
                    crop: None,
                },
                sampling_identity_digest: ContentDigest([1; 32]),
                sampling_filter: "nearest".into(),
                cell_geometry: CellGeometry {
                    cell_width_milli_px: 1_000,
                    cell_height_milli_px: 1_000,
                },
                patch_dimensions: None,
                palette: Vec::new(),
                settings_digest: ContentDigest([digest; 32]),
            })
        };

        let directory = TempManagedDirectory::new();
        let output_dir = fs::canonicalize(&directory.0).unwrap();
        let destination = output_dir.join("committed-output");
        let failed_destination = output_dir.join("failed-output");
        let committed_identity = identity(0);
        let failed_identity = identity(1);
        let registry = OutputRegistry::default();
        let budget = DiskBudget::new(128, 0);
        let reservation = budget
            .reserve(DiskRequirements {
                output_bytes: 32,
                ..DiskRequirements::default()
            })
            .unwrap();

        let commit_events = RefCell::new(Vec::with_capacity(3));
        let committed = registry
            .write_transactional_with_sync(
                committed_identity.clone(),
                &destination,
                &reservation,
                |output| output.write_all(b"committed"),
                OutputTransactionHooks {
                    sync_file: |_: &File| {
                        commit_events.borrow_mut().push(CommitEvent::FileSync);
                        Ok(())
                    },
                    replace: |temporary: &Path, destination: &Path| {
                        commit_events.borrow_mut().push(CommitEvent::Replace);
                        replace_destination(temporary, destination)
                    },
                    sync_parent_directory: |parent: &Path| {
                        assert_eq!(parent, output_dir.as_path());
                        assert_eq!(fs::read(&destination)?, b"committed");
                        commit_events
                            .borrow_mut()
                            .push(CommitEvent::ParentDirectorySync);
                        Ok(())
                    },
                },
            )
            .unwrap();
        assert_eq!(committed, destination);
        assert_eq!(
            commit_events.into_inner(),
            vec![
                CommitEvent::FileSync,
                CommitEvent::Replace,
                CommitEvent::ParentDirectorySync
            ]
        );
        assert_eq!(
            registry.registered_path(&committed_identity),
            Some(destination.clone())
        );

        let failed_events = RefCell::new(Vec::with_capacity(4));
        let failed_sync_calls = Cell::new(0usize);
        let error = registry
            .write_transactional_with_sync(
                failed_identity.clone(),
                &failed_destination,
                &reservation,
                |output| output.write_all(b"not registered"),
                OutputTransactionHooks {
                    sync_file: |_: &File| {
                        failed_events.borrow_mut().push(CommitEvent::FileSync);
                        Ok(())
                    },
                    replace: |temporary: &Path, destination: &Path| {
                        failed_events.borrow_mut().push(CommitEvent::Replace);
                        replace_destination(temporary, destination)
                    },
                    sync_parent_directory: |parent: &Path| {
                        assert_eq!(parent, output_dir.as_path());
                        let sync_call = failed_sync_calls.get();
                        failed_sync_calls.set(sync_call + 1);
                        failed_events
                            .borrow_mut()
                            .push(CommitEvent::ParentDirectorySync);
                        if sync_call == 0 {
                            assert_eq!(fs::read(&failed_destination)?, b"not registered");
                            Err(io::Error::other(
                                "injected output parent-directory sync failure",
                            ))
                        } else {
                            assert!(!failed_destination.exists());
                            Ok(())
                        }
                    },
                },
            )
            .unwrap_err();
        assert_eq!(error.kind, OutputFailureKind::Sync);
        assert_eq!(error.partial_temp_path, None);
        assert_eq!(error.recovery_path, None);
        assert_eq!(
            failed_events.into_inner(),
            vec![
                CommitEvent::FileSync,
                CommitEvent::Replace,
                CommitEvent::ParentDirectorySync,
                CommitEvent::ParentDirectorySync
            ]
        );
        assert_eq!(failed_sync_calls.get(), 2);
        assert!(!failed_destination.exists());
        assert_eq!(registry.registered_path(&failed_identity), None);
        assert_eq!(
            registry.registered_path(&committed_identity),
            Some(destination)
        );
    }
    #[test]
    fn transactional_output_parent_sync_failure_restores_existing_destination() {
        use std::cell::Cell;

        let directory = TempManagedDirectory::new();
        let output_dir = fs::canonicalize(&directory.0).unwrap();
        let destination = output_dir.join("existing-output");
        let existing_identity = cache_test_identity(200);
        let failed_identity = cache_test_identity(201);
        let registry = OutputRegistry::default();
        let budget = DiskBudget::new(128, 0);
        let reservation = budget
            .reserve(DiskRequirements {
                output_bytes: 64,
                ..DiskRequirements::default()
            })
            .unwrap();
        registry
            .write_transactional(
                existing_identity.clone(),
                &destination,
                &reservation,
                |output| output.write_all(b"original bytes"),
            )
            .unwrap();

        let sync_calls = Cell::new(0usize);
        let error = registry
            .write_transactional_with_sync(
                failed_identity.clone(),
                &destination,
                &reservation,
                |output| output.write_all(b"replacement bytes"),
                OutputTransactionHooks {
                    sync_file: |_: &File| Ok(()),
                    replace: replace_destination,
                    sync_parent_directory: |parent: &Path| {
                        assert_eq!(parent, output_dir.as_path());
                        let sync_call = sync_calls.get();
                        sync_calls.set(sync_call + 1);
                        match sync_call {
                            0 => {
                                assert_eq!(fs::read(&destination)?, b"original bytes");
                                Ok(())
                            }
                            1 => {
                                assert_eq!(fs::read(&destination)?, b"replacement bytes");
                                Err(io::Error::new(
                                    ErrorKind::StorageFull,
                                    "injected post-publish parent sync failure",
                                ))
                            }
                            2 => {
                                assert_eq!(fs::read(&destination)?, b"original bytes");
                                Ok(())
                            }
                            _ => panic!("unexpected parent-directory sync"),
                        }
                    },
                },
            )
            .unwrap_err();

        assert_eq!(error.kind, OutputFailureKind::DiskFull);
        assert_eq!(error.partial_temp_path, None);
        assert_eq!(error.recovery_path, None);
        assert_eq!(sync_calls.get(), 3);
        assert_eq!(fs::read(&destination).unwrap(), b"original bytes");
        assert_eq!(
            registry.registered_path(&existing_identity),
            Some(destination.clone())
        );
        assert_eq!(registry.registered_path(&failed_identity), None);
        let transaction_temps = fs::read_dir(&output_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".asciiwall-output-")
            })
            .count();
        assert_eq!(transaction_temps, 0);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn output_symlink_destination_is_rejected_before_publication() {
        let directory = TempManagedDirectory::new();
        let output_dir = fs::canonicalize(&directory.0).unwrap();
        let target = output_dir.join("symlink-target");
        let destination = output_dir.join("symlink-output");
        fs::write(&target, b"original bytes").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &destination).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &destination).unwrap();

        let identity = cache_test_identity(202);
        let registry = OutputRegistry::default();
        let budget = DiskBudget::new(64, 0);
        let reservation = budget
            .reserve(DiskRequirements {
                output_bytes: 32,
                ..DiskRequirements::default()
            })
            .unwrap();
        let error = registry
            .write_transactional(identity.clone(), &destination, &reservation, |output| {
                output.write_all(b"replacement bytes")
            })
            .unwrap_err();

        assert_eq!(error.kind, OutputFailureKind::Commit);
        assert_eq!(error.recovery_path, None);
        let partial_path = error
            .partial_temp_path
            .expect("rejected symlink leaves the staged output available");
        assert_eq!(fs::read(&partial_path).unwrap(), b"replacement bytes");
        assert!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&destination).unwrap(), b"original bytes");
        assert_eq!(fs::read(&target).unwrap(), b"original bytes");
        assert_eq!(registry.registered_path(&identity), None);
        fs::remove_file(partial_path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn output_symlink_recovery_path_reports_dangling_backup() {
        let directory = TempManagedDirectory::new();
        let backup_path = directory.0.join("dangling-backup");
        let destination = directory.0.join("missing-destination");
        std::os::unix::fs::symlink("missing-target", &backup_path).unwrap();
        assert!(!backup_path.exists());

        assert_eq!(
            output_recovery_path(Some(&backup_path), &destination),
            Some(backup_path)
        );
    }

    struct DiskFullAfterPrefix {
        remaining_bytes: usize,
    }

    impl Write for DiskFullAfterPrefix {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining_bytes == 0 {
                return Err(io::Error::new(
                    ErrorKind::StorageFull,
                    "injected underlying disk-full error",
                ));
            }
            let written = self.remaining_bytes.min(bytes.len());
            self.remaining_bytes -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn swallowed_underlying_disk_full_write_error_is_retained() {
        let budget = DiskBudget::new(100, 0);
        let reservation = budget
            .reserve(DiskRequirements {
                output_bytes: 8,
                ..DiskRequirements::default()
            })
            .unwrap();
        let mut file = DiskFullAfterPrefix { remaining_bytes: 4 };
        let (callback_result, underlying_error) = {
            let mut limited = LimitedWriter::new(&mut file, &reservation);
            let callback_result: io::Result<()> = {
                let output: &mut dyn Write = &mut limited;
                let _ = output.write_all(b"abcdefgh");
                Ok(())
            };
            (callback_result, limited.underlying_error.take())
        };

        assert!(callback_result.is_ok());
        let error = underlying_error.expect("the swallowed write failure remains latched");
        assert_eq!(error.kind(), ErrorKind::StorageFull);
        assert!(is_disk_full(&error));
        assert_eq!(budget.snapshot().reserved_bytes, 4);
        assert_eq!(budget.snapshot().consumed_bytes_since_refresh, 4);
    }
    struct DiskFullOnFlush;

    impl Write for DiskFullOnFlush {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(
                ErrorKind::StorageFull,
                "injected underlying disk-full flush error",
            ))
        }
    }

    #[test]
    fn swallowed_underlying_disk_full_flush_error_is_retained() {
        let budget = DiskBudget::new(100, 0);
        let reservation = budget
            .reserve(DiskRequirements {
                output_bytes: 8,
                ..DiskRequirements::default()
            })
            .unwrap();
        let mut file = DiskFullOnFlush;
        let (callback_result, underlying_error) = {
            let mut limited = LimitedWriter::new(&mut file, &reservation);
            let callback_result: io::Result<()> = {
                let output: &mut dyn Write = &mut limited;
                let _ = output.flush();
                Ok(())
            };
            (callback_result, limited.underlying_error.take())
        };

        assert!(callback_result.is_ok());
        let error = underlying_error.expect("the swallowed flush failure remains latched");
        assert_eq!(error.kind(), ErrorKind::StorageFull);
        assert!(is_disk_full(&error));
        assert_eq!(budget.snapshot().reserved_bytes, 8);
        assert_eq!(budget.snapshot().consumed_bytes_since_refresh, 0);
    }
    fn cache_test_identity(source: u8) -> CacheIdentity {
        CacheIdentity::Conversion(ConversionKey {
            source: SourceContent::Digest(ContentDigest([source; 32])),
            decode_version: "decode-v1".into(),
            orientation_version: "orientation-v1".into(),
            color_version: "color-v1".into(),
            algorithm_version: "algorithm-v1".into(),
            font_digest: ContentDigest([0; 32]),
            font_raster_settings: FontRasterSettings::default(),
            glyphs: Vec::new(),
            grid: GridSize {
                columns: 1,
                rows: 1,
            },
            fit_crop: FitCrop {
                mode: FitMode::Fit,
                crop: None,
            },
            sampling_identity_digest: ContentDigest([1; 32]),
            sampling_filter: "nearest".into(),
            cell_geometry: CellGeometry {
                cell_width_milli_px: 1_000,
                cell_height_milli_px: 1_000,
            },
            patch_dimensions: None,
            palette: Vec::new(),
            settings_digest: ContentDigest([source; 32]),
        })
    }

    #[test]
    fn independently_opened_caches_observe_overlapping_store_contention() {
        use std::sync::mpsc;
        use std::time::Duration;

        let directory = TempManagedDirectory::new();
        let cache_root = directory.0.join("cache");
        let first_cache = DerivedCache::open(&cache_root, 5).unwrap();
        let second_cache = DerivedCache::open(&cache_root, 5).unwrap();
        let first_worker_cache = first_cache.clone();
        let second_worker_cache = second_cache.clone();
        let (first_holds_lock_tx, first_holds_lock_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_contended_tx, second_contended_rx) = mpsc::channel();
        let timeout = Duration::from_secs(30);

        std::thread::scope(|scope| {
            let first_worker = scope.spawn(move || {
                set_root_operation_test_hook(move |point, _| {
                    if point == RootOperationTestPoint::Acquired {
                        let _ = first_holds_lock_tx.send(());
                        let _ = release_first_rx.recv();
                    }
                });
                first_worker_cache
                    .store(cache_test_identity(1), b"first")
                    .unwrap()
            });

            if let Err(error) = first_holds_lock_rx.recv_timeout(timeout) {
                let _ = release_first_tx.send(());
                let _ = first_worker.join();
                panic!("first store did not hold the root mutex: {error}");
            }

            let second_worker = scope.spawn(move || {
                set_root_operation_test_hook(move |point, root_lock| {
                    if point == RootOperationTestPoint::BeforeAcquire {
                        let contended = matches!(
                            root_lock.try_lock(),
                            Err(std::sync::TryLockError::WouldBlock)
                        );
                        let _ = second_contended_tx.send(contended);
                    }
                });
                second_worker_cache
                    .store(cache_test_identity(2), b"other")
                    .unwrap()
            });

            let second_saw_contention = second_contended_rx.recv_timeout(timeout);
            let _ = release_first_tx.send(());
            assert_eq!(first_worker.join().unwrap(), StoreOutcome::Inserted);
            assert_eq!(second_worker.join().unwrap(), StoreOutcome::Inserted);
            assert!(
                second_saw_contention.expect("second store reached the root lock"),
                "second store did not observe the first store holding the shared root mutex"
            );
        });

        assert_eq!(first_cache.snapshot().unwrap().used_bytes, 5);
        assert_eq!(second_cache.snapshot().unwrap().used_bytes, 5);
    }

    #[test]
    fn empty_entries_remain_valid_under_a_zero_byte_budget_and_file_count_is_bounded() {
        let directory = TempManagedDirectory::new();
        let cache_root = directory.0.join("cache");
        let _setup_cache = DerivedCache::open(&cache_root, u64::MAX).unwrap();
        let managed_dir = cache_root.join(CACHE_INSTANCE_DIR);

        for file_id in 0..=MAX_CACHE_FILES as u64 {
            fs::write(managed_dir.join(format!("derived-{file_id:016x}.bin")), b"").unwrap();
        }

        let cache = DerivedCache::open(&cache_root, 0).unwrap();
        let count_generated_files = || {
            fs::read_dir(&managed_dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
                .count()
        };
        assert_eq!(count_generated_files(), MAX_CACHE_FILES);

        let identity = cache_test_identity(42);
        assert_eq!(
            cache.store(identity.clone(), b"").unwrap(),
            StoreOutcome::Inserted
        );
        assert_eq!(cache.snapshot().unwrap().used_bytes, 0);
        assert_eq!(cache.snapshot().unwrap().entries, 1);
        let mut lease = cache.lease(&identity).unwrap();
        assert_eq!(fs::read(lease.path()).unwrap(), b"");
        lease.acknowledge().unwrap();
        assert_eq!(count_generated_files(), MAX_CACHE_FILES);

        let locked_files: Vec<_> = fs::read_dir(&managed_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
            .map(|entry| {
                let file = File::open(entry.path()).unwrap();
                file.try_lock().unwrap();
                file
            })
            .collect();
        let error = cache.store(cache_test_identity(43), b"").unwrap_err();
        assert!(matches!(
            &error,
            CacheError::FileCountPinned {
                current_files,
                max_files,
                requested_files,
            } if *current_files == MAX_CACHE_FILES
                && *max_files == MAX_CACHE_FILES
                && *requested_files == 1
        ));
        assert_eq!(
            error.to_string(),
            "cache file-count limit 1024 blocks 1 additional file while 1024 managed files are pinned or locked"
        );
        assert_eq!(locked_files.len(), MAX_CACHE_FILES);
        assert_eq!(count_generated_files(), MAX_CACHE_FILES);
        drop(locked_files);
    }

    #[test]
    fn opening_over_cap_locked_orphans_reports_file_count_trim_error() {
        let expected_files = MAX_CACHE_FILES + 1;
        let directory = TempManagedDirectory::new();
        let cache_root = directory.0.join("cache");
        let setup_cache = DerivedCache::open(&cache_root, u64::MAX).unwrap();
        let managed_dir = cache_root.join(CACHE_INSTANCE_DIR);
        drop(setup_cache);

        for file_id in 0..expected_files as u64 {
            fs::write(managed_dir.join(format!("derived-{file_id:016x}.bin")), b"").unwrap();
        }
        let locked_files: Vec<_> = (0..expected_files)
            .map(|file_id| {
                let path = managed_dir.join(format!("derived-{file_id:016x}.bin"));
                let file = File::open(path).unwrap();
                file.try_lock().unwrap();
                file
            })
            .collect();

        let error = match DerivedCache::open(&cache_root, u64::MAX) {
            Err(error) => error,
            Ok(_) => panic!("opening an over-cap cache with locked files must fail to trim"),
        };
        assert!(matches!(
            &error,
            CacheError::FileCountPinned {
                current_files,
                max_files,
                requested_files,
            } if *current_files == expected_files
                && *max_files == MAX_CACHE_FILES
                && *requested_files == 0
        ));
        assert_eq!(
            error.to_string(),
            format!(
                "cache file-count limit {MAX_CACHE_FILES} cannot be met: \
                 {expected_files} managed files remain; exceeding the limit because files are pinned or locked"
            )
        );
        assert_eq!(locked_files.len(), expected_files);
        assert_eq!(
            fs::read_dir(&managed_dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("derived-"))
                .count(),
            expected_files
        );
        drop(locked_files);
    }

    #[test]
    fn eviction_snapshots_candidates_once_and_traverses_lock_blocked_entries_linearly() {
        const FILES: usize = 16;

        let directory = TempManagedDirectory::new();
        let cache_root = directory.0.join("cache");
        let cache = DerivedCache::open(&cache_root, FILES as u64).unwrap();
        assert_eq!(
            cache.store(cache_test_identity(0), b"x").unwrap(),
            StoreOutcome::Inserted
        );
        let managed_dir = cache.root().join(CACHE_INSTANCE_DIR);
        for file_id in 1..FILES as u64 {
            fs::write(
                managed_dir.join(format!("derived-{file_id:016x}.bin")),
                b"x",
            )
            .unwrap();
        }
        let paths: Vec<_> = (0..FILES)
            .map(|file_id| managed_dir.join(format!("derived-{file_id:016x}.bin")))
            .collect();
        let locked_files: Vec<_> = paths[..FILES - 1]
            .iter()
            .map(|path| {
                let file = File::open(path).unwrap();
                file.try_lock().unwrap();
                file
            })
            .collect();

        let events = Arc::new(Mutex::new(Vec::new()));
        let hook_events = Arc::clone(&events);
        let hook_reset = set_eviction_test_hook(move |point| {
            lock_unpoisoned(&hook_events).push(point);
        });
        let result = cache.set_budget_bytes(1);
        drop(hook_reset);

        assert!(
            matches!(
                &result,
                Err(CacheError::BudgetPinned {
                    used_bytes,
                    budget_bytes: 1,
                    requested_bytes: 0,
                }) if *used_bytes == (FILES - 1) as u64
            ),
            "unexpected budget trim result: {result:?}"
        );
        assert!(!paths[FILES - 1].exists());
        assert!(paths[..FILES - 1].iter().all(|path| path.exists()));

        let events = lock_unpoisoned(&events);
        assert_eq!(
            events.first(),
            Some(&EvictionTestPoint::CandidatesSnapshotted(FILES))
        );
        assert_eq!(
            events
                .iter()
                .filter(|point| **point == EvictionTestPoint::CandidateExamined)
                .count(),
            FILES
        );
        assert_eq!(events.len(), FILES + 1);
        drop(locked_files);
    }
}
