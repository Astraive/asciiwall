//! Content-addressed derived caching, explicit resource leases, and transactional disk output.
//!
//! A source digest is the reusable identity. Paths and stat metadata are kept separately as
//! revalidation hints and never participate in cache-key equality. `CacheLease::acknowledge` is an
//! explicit owner boundary: dropping a lease does not imply decoder/GPU completion and therefore
//! does not unpin its entry.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const CACHE_INSTANCE_DIR: &str = ".asciiwall-cache-v1";
const CACHE_OWNER_FILE: &str = ".asciiwall-owner";
const CACHE_OWNER_MARKER: &[u8] = b"AsciiWall derived cache ownership v1\n";
const CACHE_OPERATIONS_LOCK: &str = ".operations.lock";

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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GpuCacheKey {
    pub upstream: Box<CacheIdentity>,
    pub backend: String,
    pub device: String,
    pub shader_digest: ContentDigest,
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
    orphaned_files: Vec<OrphanedFile>,
    active_lease_files: HashMap<CacheIdentity, Vec<File>>,
}

#[derive(Debug)]
struct CacheInner {
    root: PathBuf,
    managed_dir: PathBuf,
    state: Mutex<CacheState>,
}

/// Thread-safe byte-bounded cache of generated files beneath a private root. Each instance keeps
/// LRU identity state for entries it knows; files known only through root reconciliation remain
/// accounted and are selected as orphaned eviction candidates by generated file ID. Operations
/// that account for root-wide byte usage reconcile under an OS lock; instances sharing a root must
/// use the same budget.
/// Active leases hold portable OS shared file locks; deletion obtains an exclusive file lock and
/// fails closed while any instance holds a lease. The caller must keep the root private and stable;
/// portable path APIs cannot defend against a hostile concurrent root-directory replacement.
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
            orphaned_files,
            active_lease_files: HashMap::new(),
        };
        trim_to_budget(&root, &mut state)?;
        Ok(Self {
            inner: Arc::new(CacheInner {
                root,
                managed_dir,
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
        let lease_file = File::open(&path)?;
        lease_file.try_lock_shared().map_err(io::Error::from)?;
        let tick = next_tick(&mut state)?;
        let entry = state
            .entries
            .get_mut(identity)
            .ok_or(CacheError::NotFound)?;
        entry.active_leases = entry
            .active_leases
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
        entry.last_used = tick;
        state
            .active_lease_files
            .entry(identity.clone())
            .or_default()
            .push(lease_file);
        Ok(CacheLease {
            inner: Arc::clone(&self.inner),
            identity: identity.clone(),
            path,
        })
    }

    /// Changes the configured limit and immediately evicts eligible LRU entries. If pinned
    /// entries alone exceed the new limit, the lower limit remains in force and the error reports
    /// that completion acknowledgements are needed before it can be met.
    pub fn set_budget_bytes(&self, budget_bytes: u64) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        state.budget_bytes = budget_bytes;
        trim_to_budget(&self.inner.root, &mut state)
    }

    /// Retries eviction after an owner boundary or other external state change.
    pub fn trim_to_budget(&self) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        reconcile_cache_state(&self.inner.root, &mut state)?;
        trim_to_budget(&self.inner.root, &mut state)
    }

    pub fn snapshot(&self) -> Result<CacheSnapshot, CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
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

/// A non-cloneable resource pin. Dropping it intentionally does not release the pin; call
/// `acknowledge` only when every decoder/upload/user of this resource has completed.
#[must_use = "explicitly acknowledge resource completion to unpin this cache entry"]
#[derive(Debug)]
pub struct CacheLease {
    inner: Arc<CacheInner>,
    identity: CacheIdentity,
    path: PathBuf,
}

impl CacheLease {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> &CacheIdentity {
        &self.identity
    }

    pub fn acknowledge(self) -> Result<(), CacheError> {
        let mut state = lock_unpoisoned(&self.inner.state);
        let _operations_lock = lock_cache_directory(&self.inner.root, &self.inner.managed_dir)?;
        let active_leases = state
            .entries
            .get(&self.identity)
            .ok_or(CacheError::InvalidLease)?
            .active_leases
            .checked_sub(1)
            .ok_or(CacheError::InvalidLease)?;
        let tick = next_tick(&mut state)?;
        let entry = state
            .entries
            .get_mut(&self.identity)
            .ok_or(CacheError::InvalidLease)?;
        entry.active_leases = active_leases;
        entry.last_used = tick;
        let handles = state
            .active_lease_files
            .get_mut(&self.identity)
            .ok_or(CacheError::InvalidLease)?;
        let handle = handles.pop().ok_or(CacheError::InvalidLease)?;
        if handles.is_empty() {
            state.active_lease_files.remove(&self.identity);
        }
        drop(handle);
        trim_to_budget(&self.inner.root, &mut state)
    }
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
    state.orphaned_files = discovered;
    state.used_bytes = used_bytes;
    state.next_file_id = state.next_file_id.max(next_file_id);
    state.lru_clock = state.lru_clock.max(lru_clock);
    Ok(())
}

fn evict_for(root: &Path, state: &mut CacheState, requested_bytes: u64) -> Result<(), CacheError> {
    let target = state
        .budget_bytes
        .checked_sub(requested_bytes)
        .ok_or(CacheError::ArithmeticOverflow)?;
    while state.used_bytes > target {
        if !evict_oldest_file(root, state)? {
            return Err(CacheError::BudgetPinned {
                used_bytes: state.used_bytes,
                budget_bytes: state.budget_bytes,
                requested_bytes,
            });
        }
    }
    Ok(())
}

fn trim_to_budget(root: &Path, state: &mut CacheState) -> Result<(), CacheError> {
    while state.used_bytes > state.budget_bytes {
        if !evict_oldest_file(root, state)? {
            return Err(CacheError::BudgetPinned {
                used_bytes: state.used_bytes,
                budget_bytes: state.budget_bytes,
                requested_bytes: 0,
            });
        }
    }
    Ok(())
}

fn evict_oldest_file(root: &Path, state: &mut CacheState) -> Result<bool, CacheError> {
    let mut last_examined_tick = 0u64;
    loop {
        let entry_victim = state
            .entries
            .iter()
            .filter(|(_, entry)| entry.active_leases == 0 && entry.last_used > last_examined_tick)
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(identity, entry)| (entry.last_used, identity));
        let orphan_victim = state
            .orphaned_files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.last_used > last_examined_tick)
            .min_by_key(|(_, file)| file.last_used)
            .map(|(index, file)| (file.last_used, index));
        match (entry_victim, orphan_victim) {
            (Some((entry_tick, identity)), Some((orphan_tick, _))) if entry_tick <= orphan_tick => {
                let identity = identity.clone();
                if remove_entry(root, state, &identity)? {
                    return Ok(true);
                }
                last_examined_tick = entry_tick;
            }
            (Some((tick, identity)), None) => {
                let identity = identity.clone();
                if remove_entry(root, state, &identity)? {
                    return Ok(true);
                }
                last_examined_tick = tick;
            }
            (_, Some((tick, index))) => {
                if remove_orphaned_file(root, state, index)? {
                    return Ok(true);
                }
                last_examined_tick = tick;
            }
            (None, None) => return Ok(false),
        }
    }
}

fn remove_orphaned_file(
    root: &Path,
    state: &mut CacheState,
    index: usize,
) -> Result<bool, CacheError> {
    let orphan = state
        .orphaned_files
        .get(index)
        .ok_or(CacheError::NotFound)?;
    if !delete_managed_file(root, &orphan.path)? {
        return Ok(false);
    }
    let removed = state.orphaned_files.remove(index);
    state.used_bytes = state
        .used_bytes
        .checked_sub(removed.bytes)
        .ok_or(CacheError::ArithmeticOverflow)?;
    Ok(true)
}

fn remove_entry(
    root: &Path,
    state: &mut CacheState,
    identity: &CacheIdentity,
) -> Result<bool, CacheError> {
    let entry = state.entries.get(identity).ok_or(CacheError::NotFound)?;
    if entry.active_leases != 0 {
        return Err(CacheError::InvalidLease);
    }
    if !delete_managed_file(root, &entry.path)? {
        return Ok(false);
    }
    let removed = state.entries.remove(identity).ok_or(CacheError::NotFound)?;
    state.entries_by_file_id.remove(&removed.file_id);
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

fn create_cache_instance_dir(root: &Path) -> Result<PathBuf, CacheError> {
    verify_managed_root(root)?;
    let managed_dir = root.join(CACHE_INSTANCE_DIR);
    match fs::create_dir(&managed_dir) {
        Ok(()) => {
            if let Err(error) = write_cache_owner_marker(&managed_dir) {
                let _ = fs::remove_file(managed_dir.join(CACHE_OWNER_FILE));
                let _ = fs::remove_dir(&managed_dir);
                return Err(error);
            }
            Ok(managed_dir)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            if cache_instance_is_owned(root, &managed_dir)? {
                Ok(managed_dir)
            } else {
                Err(CacheError::UnsafeDerivedPath(managed_dir))
            }
        }
        Err(error) => Err(CacheError::Io(error)),
    }
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

fn create_derived_file(
    root: &Path,
    managed_dir: &Path,
    state: &mut CacheState,
) -> Result<(u64, PathBuf, File), CacheError> {
    verify_owned_cache_dir(root, managed_dir)?;
    loop {
        let id = state.next_file_id;
        state.next_file_id = state
            .next_file_id
            .checked_add(1)
            .ok_or(CacheError::ArithmeticOverflow)?;
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
            if !metadata.file_type().is_file() {
                continue;
            }
            next_file_id = next_file_id.max(
                file_id
                    .checked_add(1)
                    .ok_or(CacheError::ArithmeticOverflow)?,
            );
            discovered.push((file_id, path, metadata.len()));
        }
    }
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
    total_output_bytes_written: u128,
    next_probe_id: u64,
    applied_probe_id: u64,
}

#[derive(Debug)]
struct DiskBudgetInner {
    safety_reserve_bytes: u64,
    state: Mutex<DiskBudgetState>,
}


/// Snapshot token captured before probing host disk space. Applying a token charges output bytes
/// written since it was created and ignores readings from older or unrelated probes.
#[derive(Clone, Debug)]
pub struct DiskBudgetProbe {
    inner: Arc<DiskBudgetInner>,
    probe_id: u64,
    total_output_bytes_written: u128,
}

/// Shared reservation ledger. The host supplies available bytes from its platform disk-space
/// probe; every reservation holds back the configured safety reserve. Output bytes written after
/// a probe token is captured remain charged against the reading applied with that token.
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
                    total_output_bytes_written: 0,
                    next_probe_id: 0,
                    applied_probe_id: 0,
                }),
            }),
        }
    }

    /// Starts a host-space probe. Capture this token before invoking the host probe, then apply it
    /// with `refresh_available_bytes`; writes after this point remain charged against that reading.
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
            total_output_bytes_written: state.total_output_bytes_written,
        })
    }

    /// Applies a host-probed free-space estimate. Returns false for a stale token or a token from
    /// another ledger. Writes recorded after the token was captured remain charged to the reading.
    pub fn refresh_available_bytes(
        &self,
        probe: DiskBudgetProbe,
        available_bytes: u64,
    ) -> bool {
        if !Arc::ptr_eq(&probe.inner, &self.inner) {
            return false;
        }
        let mut state = lock_unpoisoned(&self.inner.state);
        if probe.probe_id <= state.applied_probe_id {
            return false;
        }
        let Some(bytes_written_after_probe) = state
            .total_output_bytes_written
            .checked_sub(probe.total_output_bytes_written)
        else {
            return false;
        };
        state.available_bytes = available_bytes;
        state.consumed_bytes_since_refresh =
            u64::try_from(bytes_written_after_probe).unwrap_or(u64::MAX);
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

/// RAII accounting for expanded input, scratch, and output capacity. Dropping it releases
/// unwritten capacity; bytes already written remain charged until a host-space refresh.
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
        let next_total_output_bytes_written = state
            .total_output_bytes_written
            .checked_add(u128::from(written_bytes))
            .ok_or_else(|| io::Error::other("total output byte accounting overflowed"))?;
        state.reserved_bytes = next_reserved;
        state.consumed_bytes_since_refresh = next_consumed;
        state.total_output_bytes_written = next_total_output_bytes_written;
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
        let mut state = lock_unpoisoned(&self.inner.state);
        state.reserved_bytes = state
            .reserved_bytes
            .checked_sub(unwritten_reserved_bytes)
            .expect("disk reservation accounting invariant violated");
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

/// Failed output transaction. Once a temporary file has been created its path is retained and
/// returned for caller-directed cleanup or recovery; it is never registered as valid output.
#[derive(Debug)]
pub struct OutputWriteError {
    pub kind: OutputFailureKind,
    pub partial_temp_path: Option<PathBuf>,
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
/// and committed at the destination. Transactions for this registry are serialized; writer
/// callbacks must not re-enter the same registry.
#[derive(Clone, Debug, Default)]
pub struct OutputRegistry {
    state: Arc<Mutex<OutputRegistryState>>,
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
    /// Relative destinations are resolved to an absolute path once at transaction start, and that
    /// captured path is used for staging, verification, commit, errors, and registration.
    /// Registration follows commit only; failures preserve the destination and report any partial.
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
        let mut registry = lock_unpoisoned(&self.state);
        let destination = canonical_output_destination(destination.as_ref()).map_err(|error| {
            OutputWriteError::new(OutputFailureKind::TemporaryCreate, None, Some(error))
        })?;
        let canonical_destination = destination.clone();
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
        if let Err(error) = file.sync_all() {
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
        if let Err(error) = replace_destination(&temp_path, &destination) {
            let kind = if is_disk_full(&error) {
                OutputFailureKind::DiskFull
            } else {
                OutputFailureKind::Commit
            };
            return Err(OutputWriteError::new(kind, Some(temp_path), Some(error)));
        }
        let committed_file_identity = output_file_identity_from_file(&file).ok().flatten();
        drop(file);
        let committed_path = destination.clone();
        let committed_canonical_destination = canonical_destination;
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

#[cfg(not(windows))]
fn same_output_destination(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn same_output_destination(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    use std::os::windows::fs::MetadataExt;

    let (Some(left_parent), Some(right_parent)) = (left.parent(), right.parent()) else {
        return false;
    };
    if left_parent != right_parent {
        return false;
    }
    let (Some(left_name), Some(right_name)) = (left.file_name(), right.file_name()) else {
        return false;
    };
    const FILE_ATTRIBUTE_CASE_SENSITIVE_DIR: u32 = 0x1000_0000;
    let Ok(parent_metadata) = fs::metadata(left_parent) else {
        return false;
    };
    if parent_metadata.file_attributes() & FILE_ATTRIBUTE_CASE_SENSITIVE_DIR != 0 {
        left_name == right_name
    } else {
        windows_filename_eq_ordinal_ignore_case(left_name, right_name)
    }
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

#[cfg(unix)]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp_path, destination)
}

#[cfg(windows)]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let existing: Vec<u16> = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let new: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH.
    let success = unsafe { MoveFileExW(existing.as_ptr(), new.as_ptr(), 0x1 | 0x8) };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_destination(temp_path: &Path, destination: &Path) -> io::Result<()> {
    // A hard link commits only if the destination is absent, so unsupported targets never clobber.
    fs::hard_link(temp_path, destination)?;
    let _ = fs::remove_file(temp_path);
    Ok(())
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
