//! Platform-aware storage roots and crash-safe local state.
//!
//! Directory inputs are explicit so platform mappings are deterministic in tests. Configuration,
//! install metadata, importer journals, and diagnostics are stored independently; portable exports
//! are filtered before serialization. This module performs filesystem persistence only and does not
//! claim native permission, IPC, or import-extraction behavior.
//!
//! # API and recovery semantics
//!
//! Construct [`DirectoryInputs`] explicitly (or use [`DirectoryInputs::current`]), resolve
//! [`StorageRoots`], and pass them to [`Storage`]. [`Storage::load_registry`] reads the current
//! install registry; [`Storage::save_registry`] replaces the entire document. Use
//! [`Storage::update_registry`] for serialized read-modify-write updates against the latest state.
//! [`Storage::load_config`] and [`Storage::save_config`] invoke the caller-owned schema validator.
//! Settings saves and explicit recovery require the exact current bytes previously observed by the
//! caller (`None` for absence); stale snapshots fail under the directory lock before any mutation.
//! Ordinary settings saves reject invalid or unsupported current settings before touching either the
//! destination or `settings.toml.last-good`; registry saves likewise reject invalid or unsupported
//! current registries before replacement. [`Storage::restore_config_from_backup`] is the explicit
//! settings recovery path and never replaces the last-good file. The `*_with_hook` variants expose
//! [`AtomicWriteStage`] boundaries for deterministic interruption injection; failed in-process
//! writes remove their owned temporary.
//!
//! [`ImportJournal`] accepts only staged→verified/abandoned and verified→committed/abandoned
//! transitions, serialized across callers sharing its journal directory. [`ImportJournal::recover`]
//! classifies persisted work without mutating it: staged work must be revalidated, verified work may
//! retry commit, committed work is complete, and abandoned work is eligible for cleanup.
//! [`PortableExport::from_value`] removes local paths, file URIs, network endpoints, display/monitor,
//! permission, application-rule, user/home, and history data; it rejects inputs beyond fixed traversal
//! and serialized-output limits. Local backups are explicitly labeled nonportable. [`DiagnosticLog`]
//! rotates within [`LogLimits`], coordinates writers targeting the same directory, and redacts home
//! paths, media-bearing and clipped messages by default.

#[cfg(target_os = "macos")]
use icu_casemap::CaseMapper;
#[cfg(target_os = "macos")]
use icu_normalizer::DecomposingNormalizer;
use icu_properties::{CodePointMapData, props::GeneralCategory};

use serde::de::{Error as DeError, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
#[cfg(not(windows))]
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};
use toml::Value as TomlValue;

const REGISTRY_VERSION: u32 = 1;
const CONFIG_MAX_BYTES: u64 = 8 * 1024 * 1024;
const REGISTRY_MAX_BYTES: u64 = 16 * 1024 * 1024;
const JOURNAL_MAX_BYTES: u64 = 1024 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 128;
const PORTABLE_EXPORT_MAX_BYTES: usize = 1024 * 1024;
const PORTABLE_EXPORT_MAX_NODES: usize = 16_384;
const PORTABLE_EXPORT_MAX_DEPTH: usize = 64;
const PORTABLE_EXPORT_MAX_STRING_BYTES: usize = 16 * 1024;
const PORTABLE_EXPORT_MAX_KEY_BYTES: usize = 256;
const MAX_LOG_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOG_RECORD_BYTES: usize = 64 * 1024;
static DIRECTORY_LOCKS: LazyLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Failure or durability boundaries exposed for deterministic interruption testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomicWriteStage {
    /// The exclusive sibling temporary file has been created.
    TemporaryCreated,
    /// The complete temporary file has been flushed and synced.
    TemporarySynced,
    /// The previous destination has been atomically copied to its backup, when requested.
    BackupReplaced,
    /// The new destination has been atomically installed and its parent synced where supported.
    TargetReplaced,
    /// The parent directory is about to be synced after installing the new destination.
    ParentDirectorySync,
    /// A preserved prior-state copy is about to be renamed into place during rollback.
    RecoveryRollbackRename,
    /// The previous destination has been synced to a temporary backup, before backup replacement.
    BackupTemporarySynced,
}

/// Filesystem error, malformed persisted state, or rejected input.
#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidApplicationId,
    InvalidDirectories(&'static str),
    InvalidRegistry(&'static str),
    InvalidJournal(&'static str),
    InvalidConfiguration(String),
    InvalidStoredState(&'static str),
    StaleConfiguration,
    RecoveryRollbackFailed {
        sync_error: io::Error,
        rollback_error: io::Error,
        retained_recovery_copy: Option<PathBuf>,
    },
    DocumentTooLarge,
    InvalidLogLimits,
    UnsupportedVersion {
        kind: &'static str,
        version: u32,
    },
    UnsupportedPlatform,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "storage I/O failed: {error}"),
            Self::Json(error) => write!(f, "stored JSON is invalid: {error}"),
            Self::InvalidApplicationId => f.write_str("application identifier is invalid"),
            Self::InvalidDirectories(reason) => {
                write!(f, "storage directories are invalid: {reason}")
            }
            Self::InvalidRegistry(reason) => write!(f, "install registry is invalid: {reason}"),
            Self::InvalidJournal(reason) => write!(f, "import journal is invalid: {reason}"),
            Self::InvalidConfiguration(_) => f.write_str("configuration is invalid"),
            Self::InvalidStoredState(_) => f.write_str("stored state is invalid"),
            Self::StaleConfiguration => f.write_str("configuration changed since it was read"),
            Self::RecoveryRollbackFailed {
                sync_error,
                rollback_error,
                retained_recovery_copy,
            } => {
                write!(
                    f,
                    "parent-directory sync failed ({sync_error}); restoring prior state also failed ({rollback_error})"
                )?;
                if let Some(path) = retained_recovery_copy {
                    write!(
                        f,
                        "; original bytes remain recoverable at {}",
                        path.display()
                    )?;
                }
                Ok(())
            }
            Self::DocumentTooLarge => f.write_str("document exceeds size limit"),
            Self::InvalidLogLimits => f.write_str("diagnostic log bounds are invalid"),
            Self::UnsupportedVersion { kind, version } => {
                write!(f, "unsupported {kind} version {version}")
            }
            Self::UnsupportedPlatform => {
                f.write_str("storage roots are unsupported on this platform")
            }
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::RecoveryRollbackFailed { rollback_error, .. } => Some(rollback_error),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Explicitly injected directory inputs used to resolve platform-specific storage classes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectoryInputs {
    /// Linux follows XDG config/data/cache/runtime directories, with the XDG-prescribed home fallbacks.
    Linux {
        home: PathBuf,
        config_home: Option<PathBuf>,
        data_home: Option<PathBuf>,
        cache_home: Option<PathBuf>,
        runtime_dir: Option<PathBuf>,
    },
    /// macOS uses `~/Library/Application Support` and `~/Library/Caches`.
    MacOs { home: PathBuf },
    /// Windows uses roaming AppData for configuration and Local AppData for other local state.
    Windows {
        roaming_app_data: PathBuf,
        local_app_data: PathBuf,
    },
}

impl DirectoryInputs {
    /// Reads the native platform's environment once. Tests should construct a variant directly.
    pub fn current() -> Result<Self, StorageError> {
        #[cfg(target_os = "linux")]
        {
            let home = required_env_path("HOME")?;
            return Ok(Self::Linux {
                home,
                config_home: env_path("XDG_CONFIG_HOME"),
                data_home: env_path("XDG_DATA_HOME"),
                cache_home: env_path("XDG_CACHE_HOME"),
                runtime_dir: env_path("XDG_RUNTIME_DIR"),
            });
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(Self::MacOs {
                home: required_env_path("HOME")?,
            });
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(Self::Windows {
                roaming_app_data: required_env_path("APPDATA")?,
                local_app_data: required_env_path("LOCALAPPDATA")?,
            });
        }
        #[allow(unreachable_code)]
        Err(StorageError::UnsupportedPlatform)
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

fn required_env_path(name: &str) -> Result<PathBuf, StorageError> {
    env_path(name).ok_or(StorageError::InvalidDirectories(
        "required environment directory is missing",
    ))
}

/// Resolved, distinct storage classes for one application identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageRoots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub library: PathBuf,
    pub projects: PathBuf,
    pub catalog: PathBuf,
    pub cache: PathBuf,
    pub staging: PathBuf,
    pub logs: PathBuf,
    pub runtime: PathBuf,
}

impl StorageRoots {
    /// Resolves roots using explicitly supplied platform directories.
    pub fn resolve(app_id: &str, inputs: &DirectoryInputs) -> Result<Self, StorageError> {
        validate_application_id(app_id)?;
        let app = Path::new(app_id);
        let (config, data, cache, runtime) = match inputs {
            DirectoryInputs::Linux {
                home,
                config_home,
                data_home,
                cache_home,
                runtime_dir,
            } => {
                require_absolute(home)?;
                let config_base = choose_xdg(config_home, home.join(".config"));
                let data_base = choose_xdg(data_home, home.join(".local/share"));
                let cache_base = choose_xdg(cache_home, home.join(".cache"));
                let config = config_base.join(app);
                let data = data_base.join(app);
                let cache = cache_base.join(app);
                let runtime = match runtime_dir {
                    Some(path) => {
                        require_absolute(path)?;
                        path.join(app)
                    }
                    None => data.join("runtime"),
                };
                (config, data, cache, runtime)
            }
            DirectoryInputs::MacOs { home } => {
                require_absolute(home)?;
                let support = home.join("Library/Application Support").join(app);
                let caches = home.join("Library/Caches").join(app);
                (
                    support.join("config"),
                    support,
                    caches,
                    home.join("Library/Application Support")
                        .join(app)
                        .join("runtime"),
                )
            }
            DirectoryInputs::Windows {
                roaming_app_data,
                local_app_data,
            } => {
                require_absolute(roaming_app_data)?;
                require_absolute(local_app_data)?;
                let local = local_app_data.join(app);
                (
                    roaming_app_data.join(app),
                    local.clone(),
                    local.join("cache"),
                    local.join("runtime"),
                )
            }
        };
        let mut roots = Self {
            config,
            data: data.clone(),
            library: data.join("library"),
            projects: data.join("projects"),
            catalog: data.join("catalogs"),
            cache,
            staging: data.join("staging"),
            logs: data.join("logs"),
            runtime,
        };
        let names = [
            "config", "data", "library", "projects", "catalog", "cache", "staging", "logs",
            "runtime",
        ];
        let mut identities = [
            normalized_path_identity(&roots.config),
            normalized_path_identity(&roots.data),
            normalized_path_identity(&roots.library),
            normalized_path_identity(&roots.projects),
            normalized_path_identity(&roots.catalog),
            normalized_path_identity(&roots.cache),
            normalized_path_identity(&roots.staging),
            normalized_path_identity(&roots.logs),
            normalized_path_identity(&roots.runtime),
        ];
        for _ in 0..names.len() {
            let mut duplicate_class = None;
            'pairs: for later in 1..identities.len() {
                for earlier in 0..later {
                    if path_identities_equal(&identities[earlier], &identities[later]) {
                        duplicate_class =
                            if matches!(names[earlier], "config" | "cache" | "runtime") {
                                Some(names[earlier])
                            } else if matches!(names[later], "config" | "cache" | "runtime") {
                                Some(names[later])
                            } else {
                                return Err(StorageError::InvalidDirectories(
                                    "storage classes resolve to the same path",
                                ));
                            };
                        break 'pairs;
                    }
                }
            }
            match duplicate_class {
                Some("config") => {
                    roots.config.push("config");
                    identities[0] = normalized_path_identity(&roots.config);
                }
                Some("cache") => {
                    roots.cache.push("cache");
                    identities[5] = normalized_path_identity(&roots.cache);
                }
                Some("runtime") => {
                    roots.runtime.push("runtime");
                    identities[8] = normalized_path_identity(&roots.runtime);
                }
                Some(_) => {
                    return Err(StorageError::InvalidDirectories(
                        "storage classes resolve to the same path",
                    ));
                }
                None => return Ok(roots),
            }
        }
        Err(StorageError::InvalidDirectories(
            "storage classes could not be separated",
        ))
    }

    /// Creates each app-owned storage root with owner-only Unix permissions.
    pub fn create_all(&self) -> Result<(), StorageError> {
        let paths = [
            self.config.as_path(),
            self.data.as_path(),
            self.library.as_path(),
            self.projects.as_path(),
            self.catalog.as_path(),
            self.cache.as_path(),
            self.staging.as_path(),
            self.logs.as_path(),
            self.runtime.as_path(),
        ];
        for path in paths.iter().copied() {
            create_directory_durable(path)?;
            let identities = paths.map(normalized_path_identity);
            for later in 1..identities.len() {
                if identities[..later].contains(&identities[later]) {
                    return Err(StorageError::InvalidDirectories(
                        "storage classes resolve to the same path",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn choose_xdg(value: &Option<PathBuf>, fallback: PathBuf) -> PathBuf {
    match value {
        Some(path) if path.is_absolute() => path.clone(),
        _ => fallback,
    }
}

fn require_absolute(path: &Path) -> Result<(), StorageError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(StorageError::InvalidDirectories(
            "directory input must be absolute",
        ))
    }
}

/// Resolves aliases without creating paths, retaining filesystem spelling for I/O.
fn normalized_path_for_io(path: &Path) -> PathBuf {
    let mut ancestor = path;
    loop {
        if let Ok(mut resolved) = fs::canonicalize(ancestor)
            && let Ok(suffix) = path.strip_prefix(ancestor)
        {
            append_normalized_components(&mut resolved, suffix);
            return resolved;
        }
        match ancestor.parent() {
            Some(parent) if parent != ancestor => ancestor = parent,
            _ => return normalize_path(path),
        }
    }
}

/// Produces a normalized path identity for comparisons without creating paths.
#[cfg(windows)]
fn normalized_path_identity(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    // Canonicalize the final-sigma alias before the OS ordinal comparison.
    normalized_path_for_io(path)
        .as_os_str()
        .encode_wide()
        .map(normalize_windows_path_alias_unit)
        .collect()
}

#[cfg(windows)]
fn normalize_windows_path_alias_unit(unit: u16) -> u16 {
    if unit == 0x03C2 { 0x03C3 } else { unit }
}

#[cfg(not(windows))]
fn normalized_path_identity(path: &Path) -> PathBuf {
    normalized_path_for_io(path)
}

#[cfg(windows)]
fn path_identities_equal(left: &[u16], right: &[u16]) -> bool {
    matches!(
        windows_path_collision_cmp(left, right),
        Some(std::cmp::Ordering::Equal) | None
    )
}

#[cfg(not(windows))]
fn path_identities_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn normalize_path_identity_case(path: PathBuf) -> PathBuf {
    if path.to_str().is_some() {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path
    }
}

#[cfg(not(windows))]
fn normalize_path_identity_case(path: PathBuf) -> PathBuf {
    path
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    append_normalized_components(&mut normalized, path);
    normalized
}

fn append_normalized_components(path: &mut PathBuf, suffix: &Path) {
    for component in suffix.components() {
        match component {
            std::path::Component::Prefix(_) => {
                if path.as_os_str().is_empty() {
                    path.push(component.as_os_str());
                }
            }
            std::path::Component::RootDir => path.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if path.file_name().is_some() {
                    path.pop();
                }
            }
            std::path::Component::Normal(value) => path.push(value),
        }
    }
}

/// Validates an application/package/transaction identifier before it can become a path component.
pub fn validate_application_id(value: &str) -> Result<(), StorageError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > MAX_IDENTIFIER_BYTES
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
        || value.split('.').any(str::is_empty)
    {
        return Err(StorageError::InvalidApplicationId);
    }
    let first_component = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(first_component.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            first_component.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
    {
        return Err(StorageError::InvalidApplicationId);
    }
    Ok(())
}

/// Version-one authoritative install registry persisted as one JSON document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRegistry {
    pub schema_version: u32,
    #[serde(deserialize_with = "deserialize_unique_packages")]
    pub packages: BTreeMap<String, InstallEntry>,
}

fn deserialize_unique_packages<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, InstallEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniquePackages;

    impl<'de> Visitor<'de> for UniquePackages {
        type Value = BTreeMap<String, InstallEntry>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with unique package identifiers")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut packages = BTreeMap::new();
            while let Some((package_id, entry)) = map.next_entry::<String, InstallEntry>()? {
                if packages.insert(package_id, entry).is_some() {
                    return Err(M::Error::custom(
                        "duplicate package identifier in install registry",
                    ));
                }
            }
            Ok(packages)
        }
    }

    deserializer.deserialize_map(UniquePackages)
}

impl Default for InstallRegistry {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_VERSION,
            packages: BTreeMap::new(),
        }
    }
}

/// Portable package identity and path relative to the application's immutable library root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallEntry {
    pub package_version: String,
    pub sha256: String,
    pub relative_path: String,
}

impl InstallRegistry {
    /// Rejects unsupported schemas, invalid identifiers, non-SHA-256 hashes, and unsafe paths.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != REGISTRY_VERSION {
            return Err(StorageError::UnsupportedVersion {
                kind: "install registry",
                version: self.schema_version,
            });
        }
        #[cfg(windows)]
        let mut relative_paths: Vec<Vec<u16>> = Vec::with_capacity(self.packages.len());
        #[cfg(target_os = "macos")]
        let mut relative_paths: HashSet<String> = HashSet::with_capacity(self.packages.len());
        #[cfg(all(not(windows), not(target_os = "macos")))]
        let mut relative_paths = HashSet::with_capacity(self.packages.len());
        for (package_id, entry) in &self.packages {
            validate_application_id(package_id)?;
            if entry.package_version.is_empty()
                || entry.package_version.len() > 128
                || !entry
                    .package_version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
            {
                return Err(StorageError::InvalidRegistry("package version is invalid"));
            }
            if entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(StorageError::InvalidRegistry(
                    "package digest must be lowercase SHA-256 hex",
                ));
            }
            if !is_safe_relative_path(&entry.relative_path) {
                return Err(StorageError::InvalidRegistry(
                    "package path must be normalized and relative",
                ));
            }
            #[cfg(windows)]
            relative_paths.push(windows_path_collision_key(&entry.relative_path).ok_or(
                StorageError::InvalidRegistry("package paths must be unique"),
            )?);
            #[cfg(target_os = "macos")]
            if !relative_paths.insert(macos_path_collision_key(&entry.relative_path)) {
                return Err(StorageError::InvalidRegistry(
                    "package paths must be unique",
                ));
            }
            #[cfg(all(not(windows), not(target_os = "macos")))]
            if !relative_paths.insert(entry.relative_path.as_str()) {
                return Err(StorageError::InvalidRegistry(
                    "package paths must be unique",
                ));
            }
        }
        #[cfg(windows)]
        {
            let mut comparison_failed = false;
            relative_paths.sort_unstable_by(|left, right| {
                match windows_path_collision_cmp(left, right) {
                    Some(ordering) => ordering,
                    None => {
                        comparison_failed = true;
                        std::cmp::Ordering::Equal
                    }
                }
            });
            let duplicate =
                relative_paths.windows(2).any(|paths| {
                    match windows_path_collision_cmp(&paths[0], &paths[1]) {
                        Some(std::cmp::Ordering::Equal) => true,
                        Some(_) => false,
                        None => {
                            comparison_failed = true;
                            false
                        }
                    }
                });
            if comparison_failed || duplicate {
                return Err(StorageError::InvalidRegistry(
                    "package paths must be unique",
                ));
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_path_collision_key(path: &str) -> String {
    let normalizer = DecomposingNormalizer::new_nfd();
    let normalized = normalizer.normalize(path);
    let folded = CaseMapper::new().fold_string(&normalized);
    normalizer.normalize(&folded).into_owned()
}

#[cfg(windows)]
fn is_reserved_windows_device_component(component: &str) -> bool {
    let basename = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(['.', ' ']);
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|reserved| basename.eq_ignore_ascii_case(reserved))
    {
        return true;
    }

    let mut chars = basename.chars();
    let suffix = chars.next_back();
    (chars.as_str().eq_ignore_ascii_case("COM") || chars.as_str().eq_ignore_ascii_case("LPT"))
        && matches!(suffix, Some('1'..='9') | Some('¹') | Some('²') | Some('³'))
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && value.split('/').all(|part| {
            #[cfg(windows)]
            let normalized = part.trim_end_matches(['.', ' ']);
            #[cfg(not(windows))]
            let normalized = part;
            #[cfg(windows)]
            let not_reserved = !is_reserved_windows_device_component(part);
            #[cfg(not(windows))]
            let not_reserved = true;

            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.chars().any(char::is_control)
                && part == normalized
                && not_reserved
        })
}

#[cfg(windows)]
fn windows_path_collision_key(path: &str) -> Option<Vec<u16>> {
    let mut key = Vec::new();
    for (index, component) in path.split('/').enumerate() {
        let component = component.trim_end_matches(['.', ' ']);
        if component.is_empty() {
            return None;
        }
        let separator_units = if index > 0 { 1 } else { 0 };
        let key_units = key
            .len()
            .checked_add(separator_units)?
            .checked_add(component.encode_utf16().count())?;
        if key_units > i32::MAX as usize {
            return None;
        }
        if index > 0 {
            key.push(b'/' as u16);
        }
        key.extend(
            component
                .encode_utf16()
                .map(normalize_windows_path_alias_unit),
        );
    }
    Some(key)
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "CompareStringOrdinal"]
    fn compare_string_ordinal(
        left: *const u16,
        left_length: i32,
        right: *const u16,
        right_length: i32,
        ignore_case: i32,
    ) -> i32;
}

#[cfg(windows)]
fn windows_path_collision_cmp(left: &[u16], right: &[u16]) -> Option<std::cmp::Ordering> {
    let left_length = i32::try_from(left.len()).ok()?;
    let right_length = i32::try_from(right.len()).ok()?;
    let result = unsafe {
        compare_string_ordinal(left.as_ptr(), left_length, right.as_ptr(), right_length, 1)
    };
    match result {
        1 => Some(std::cmp::Ordering::Less),
        2 => Some(std::cmp::Ordering::Equal),
        3 => Some(std::cmp::Ordering::Greater),
        _ => None,
    }
}

/// Bounded TOML syntax tree; this type does not imply that application settings passed schema validation.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedToml {
    value: TomlValue,
    bytes: Vec<u8>,
}

impl ParsedToml {
    /// Parses a bounded TOML document without imposing settings keys or application defaults.
    pub fn from_toml(bytes: &[u8]) -> Result<Self, StorageError> {
        enforce_toml_size_limit(bytes)?;
        Self::from_owned_toml(bytes.to_vec())
    }

    fn from_owned_toml(bytes: Vec<u8>) -> Result<Self, StorageError> {
        let value = parse_toml_value(&bytes)?;
        Ok(Self { value, bytes })
    }

    /// Returns the TOML syntax tree; callers still own application-schema validation.
    pub fn value(&self) -> &TomlValue {
        &self.value
    }

    /// Returns the original bounded TOML bytes without reserialization.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn enforce_toml_size_limit(bytes: &[u8]) -> Result<(), StorageError> {
    if bytes.len() as u64 > CONFIG_MAX_BYTES {
        Err(StorageError::DocumentTooLarge)
    } else {
        Ok(())
    }
}

fn parse_toml_value(bytes: &[u8]) -> Result<TomlValue, StorageError> {
    enforce_toml_size_limit(bytes)?;
    let text = std::str::from_utf8(bytes)
        .map_err(|error| StorageError::InvalidConfiguration(error.to_string()))?;
    toml::from_str(text).map_err(|error| StorageError::InvalidConfiguration(error.to_string()))
}

fn config_schema_version(value: &TomlValue) -> Option<i64> {
    value.get("schema_version")?.as_integer()
}

fn reject_history_keys(value: &TomlValue) -> Result<(), StorageError> {
    reject_history_keys_at(value, &mut String::new())
}

fn reject_history_keys_at(
    value: &TomlValue,
    normalized_path: &mut String,
) -> Result<(), StorageError> {
    match value {
        TomlValue::Table(table) => {
            for (key, nested) in table {
                let path_length = normalized_path.len();
                for character in key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                {
                    normalized_path.push(character.to_ascii_lowercase());
                }
                if is_normalized_history_key(normalized_path) {
                    return Err(StorageError::InvalidConfiguration(
                        "history settings are not allowed in local configuration".into(),
                    ));
                }
                reject_history_keys_at(nested, normalized_path)?;
                normalized_path.truncate(path_length);
            }
        }
        TomlValue::Array(items) => {
            for nested in items {
                reject_history_keys_at(nested, normalized_path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Authoritative local state paths and atomic registry/settings operations.
#[derive(Clone, Debug)]
pub struct Storage {
    roots: StorageRoots,
}

impl Storage {
    pub fn new(roots: StorageRoots) -> Self {
        Self { roots }
    }

    pub fn roots(&self) -> &StorageRoots {
        &self.roots
    }

    pub fn registry_path(&self) -> PathBuf {
        self.roots.data.join("install-registry.json")
    }

    pub fn config_path(&self) -> PathBuf {
        self.roots.config.join("settings.toml")
    }

    pub fn config_backup_path(&self) -> PathBuf {
        self.roots.config.join("settings.toml.last-good")
    }

    /// Reads the sole authoritative registry. A missing file means a new empty installation.
    pub fn load_registry(&self) -> Result<InstallRegistry, StorageError> {
        let bytes = match read_optional_bounded(&self.registry_path(), REGISTRY_MAX_BYTES)? {
            Some(bytes) => bytes,
            None => return Ok(InstallRegistry::default()),
        };
        let registry: InstallRegistry = serde_json::from_slice(&bytes)?;
        registry.validate()?;
        Ok(registry)
    }

    /// Atomically replaces the entire registry after existing bytes pass version and integrity checks.
    /// This is a full-document replacement; use [`Self::update_registry`] to modify the latest state.
    pub fn save_registry(&self, registry: &InstallRegistry) -> Result<(), StorageError> {
        self.save_registry_with_hook(registry, |_| Ok(()))
    }

    /// Registry counterpart of [`Self::save_registry`] with injectable interruption boundaries.
    pub fn save_registry_with_hook(
        &self,
        registry: &InstallRegistry,
        mut hook: impl FnMut(AtomicWriteStage) -> io::Result<()>,
    ) -> Result<(), StorageError> {
        registry.validate()?;
        let bytes = json_bytes_bounded(registry, REGISTRY_MAX_BYTES)?;
        let validate_existing = |current: &[u8]| {
            let current: InstallRegistry = serde_json::from_slice(current)?;
            current.validate()
        };
        atomic_replace(
            &self.registry_path(),
            &bytes,
            CurrentExpectation::Unchecked,
            Some((REGISTRY_MAX_BYTES, &validate_existing)),
            None,
            &mut hook,
        )?;
        Ok(())
    }

    /// Applies a read-modify-write closure to the latest validated registry under its directory lock.
    ///
    /// Use this method to preserve independent updates from concurrent callers. `save_registry`
    /// replaces the whole document and is appropriate only when replacing it intentionally.
    pub fn update_registry(
        &self,
        update: impl FnOnce(&mut InstallRegistry),
    ) -> Result<(), StorageError> {
        self.update_registry_with_hook(update, |_| Ok(()))
    }

    /// Hook-enabled counterpart of [`Self::update_registry`].
    ///
    /// A parent-directory-sync failure restores the previous registry bytes when rollback succeeds.
    pub fn update_registry_with_hook(
        &self,
        update: impl FnOnce(&mut InstallRegistry),
        hook: impl FnMut(AtomicWriteStage) -> io::Result<()>,
    ) -> Result<(), StorageError> {
        let destination = self.registry_path();
        let parent = destination
            .parent()
            .ok_or(StorageError::InvalidDirectories(
                "state file has no parent directory",
            ))?;
        create_directory_durable(parent)?;
        let directory_lock = DirectoryLock::new(parent);
        let _guard = directory_lock.lock()?;

        let current_bytes = read_optional_bounded(&destination, REGISTRY_MAX_BYTES)?;
        let mut registry = match current_bytes.as_deref() {
            Some(bytes) => {
                let registry: InstallRegistry = serde_json::from_slice(bytes)?;
                registry.validate()?;
                registry
            }
            None => InstallRegistry::default(),
        };
        update(&mut registry);
        registry.validate()?;
        let bytes = json_bytes_bounded(&registry, REGISTRY_MAX_BYTES)?;
        let validate_existing = |current: &[u8]| {
            let current: InstallRegistry = serde_json::from_slice(current)?;
            current.validate()
        };
        let expected_current = match current_bytes.as_deref() {
            Some(bytes) => CurrentExpectation::Bytes(bytes),
            None => CurrentExpectation::Missing,
        };
        let rollback = prepare_replace_rollback(&destination, REGISTRY_MAX_BYTES)?;
        let hook = RefCell::new(hook);
        let mut stage_hook = |stage| (hook.borrow_mut())(stage);
        let mut sync_hook = || (hook.borrow_mut())(AtomicWriteStage::ParentDirectorySync);
        let mut rollback_hook = || (hook.borrow_mut())(AtomicWriteStage::RecoveryRollbackRename);
        atomic_replace_locked(
            &destination,
            &bytes,
            expected_current,
            Some((REGISTRY_MAX_BYTES, &validate_existing)),
            None,
            AtomicReplaceHooks {
                stage: &mut stage_hook,
                before_parent_sync: &mut sync_hook,
                before_rollback_rename: &mut rollback_hook,
                rollback_on_sync_error: Some(rollback),
            },
        )?;
        Ok(())
    }

    /// Parses settings TOML and applies the caller-owned schema validator.
    pub fn load_config<E: fmt::Display>(
        &self,
        validate: impl FnOnce(&TomlValue) -> Result<(), E>,
    ) -> Result<Option<ParsedToml>, StorageError> {
        let bytes = match read_optional_bounded(&self.config_path(), CONFIG_MAX_BYTES)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        let config = ParsedToml::from_owned_toml(bytes)?;
        validate(config.value())
            .map_err(|error| StorageError::InvalidConfiguration(error.to_string()))?;
        Ok(Some(config))
    }

    /// Writes TOML only if `expected_current` still matches the exact current bytes (or absence)
    /// observed by the caller. A mismatch returns [`StorageError::StaleConfiguration`] before any
    /// temporary or `.last-good` mutation.
    pub fn save_config<E: fmt::Display>(
        &self,
        expected_current: Option<&[u8]>,
        config: &ParsedToml,
        validate: impl Fn(&TomlValue) -> Result<(), E>,
    ) -> Result<(), StorageError> {
        self.save_config_with_hook(expected_current, config, validate, |_| Ok(()))
    }

    /// TOML counterpart of [`Self::save_config`] with injectable interruption boundaries.
    /// `expected_current` is compared under the destination directory lock before any write.
    /// Returning an error from `hook` simulates failure at that boundary; failed writes remove their
    /// owned temporary sibling, and the destination is complete on either side of the atomic rename.
    pub fn save_config_with_hook<E: fmt::Display>(
        &self,
        expected_current: Option<&[u8]>,
        config: &ParsedToml,
        validate: impl Fn(&TomlValue) -> Result<(), E>,
        mut hook: impl FnMut(AtomicWriteStage) -> io::Result<()>,
    ) -> Result<(), StorageError> {
        reject_history_keys(config.value())?;
        validate(config.value())
            .map_err(|error| StorageError::InvalidConfiguration(error.to_string()))?;
        let validate_existing = |bytes: &[u8]| {
            let value = parse_toml_value(bytes)?;
            reject_history_keys(&value)?;
            validate(&value).map_err(|error| StorageError::InvalidConfiguration(error.to_string()))
        };
        let expected_current = match expected_current {
            Some(bytes) => CurrentExpectation::Bytes(bytes),
            None => CurrentExpectation::Missing,
        };
        let backup_path = self.config_backup_path();
        atomic_replace(
            &self.config_path(),
            config.as_bytes(),
            expected_current,
            Some((CONFIG_MAX_BYTES, &validate_existing)),
            Some((&backup_path, (CONFIG_MAX_BYTES, &validate_existing))),
            &mut hook,
        )?;
        Ok(())
    }

    /// Explicitly restores schema-valid last-good TOML only if `expected_current` still matches
    /// the exact current bytes (or absence) observed by the caller. Oversized current files are
    /// rejected before recovery work or mutation. A stale snapshot returns
    /// [`StorageError::StaleConfiguration`] before mutation. A lower versioned backup is refused,
    /// as is a comparison where only one parseable document is versioned.
    pub fn restore_config_from_backup<E: fmt::Display>(
        &self,
        expected_current: Option<&[u8]>,
        validate: impl FnOnce(&TomlValue) -> Result<(), E>,
    ) -> Result<(), StorageError> {
        self.restore_config_from_backup_with_hook(expected_current, validate, |_| Ok(()))
    }

    /// Sync-hook counterpart of [`Self::restore_config_from_backup`].
    ///
    /// The hook runs before parent-directory sync and, after a sync failure, before rollback rename.
    /// A rollback-rename failure retains the old bytes at a path exposed by
    /// [`StorageError::RecoveryRollbackFailed`]; other rollback failures are reported without
    /// claiming that the prior current-file state was restored.
    pub fn restore_config_from_backup_with_hook<E: fmt::Display>(
        &self,
        expected_current: Option<&[u8]>,
        validate: impl FnOnce(&TomlValue) -> Result<(), E>,
        hook: impl FnMut(AtomicWriteStage) -> io::Result<()>,
    ) -> Result<(), StorageError> {
        let destination = self.config_path();
        let parent = destination
            .parent()
            .ok_or(StorageError::InvalidDirectories(
                "state file has no parent directory",
            ))?;
        create_directory_durable(parent)?;
        let directory_lock = DirectoryLock::new(parent);
        let _guard = directory_lock.lock()?;
        let expected_current = match expected_current {
            Some(bytes) => CurrentExpectation::Bytes(bytes),
            None => CurrentExpectation::Missing,
        };
        let current_bytes = read_optional_bounded(&destination, CONFIG_MAX_BYTES)?;
        if !expected_current_matches(current_bytes.as_deref(), expected_current) {
            return Err(StorageError::StaleConfiguration);
        }
        let current = match current_bytes {
            Some(bytes) => match parse_toml_value(&bytes) {
                Ok(value) => Some(value),
                Err(StorageError::InvalidConfiguration(_))
                | Err(StorageError::DocumentTooLarge) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let backup = self.config_backup_path();
        let bytes = read_optional_bounded(&backup, CONFIG_MAX_BYTES)?.ok_or_else(|| {
            StorageError::InvalidConfiguration("last-good settings are missing".into())
        })?;
        let value = parse_toml_value(&bytes)?;
        reject_history_keys(&value)?;
        validate(&value).map_err(|error| StorageError::InvalidConfiguration(error.to_string()))?;
        let current_version = current.as_ref().and_then(config_schema_version);
        let backup_version = config_schema_version(&value);
        match (current.as_ref(), current_version, backup_version) {
            (Some(_), Some(current_version), Some(backup_version))
                if current_version > backup_version =>
            {
                let version = u32::try_from(current_version).map_err(|_| {
                    StorageError::InvalidConfiguration(format!(
                        "current settings schema version {current_version} is newer than last-good version {backup_version}"
                    ))
                })?;
                return Err(StorageError::UnsupportedVersion {
                    kind: "settings configuration",
                    version,
                });
            }
            (Some(_), Some(_), None) | (Some(_), None, Some(_)) => {
                return Err(StorageError::InvalidConfiguration(
                    "cannot compare versioned and versionless settings during recovery".into(),
                ));
            }
            _ => {}
        }
        let rollback = prepare_replace_rollback(&destination, CONFIG_MAX_BYTES)?;
        let hook = RefCell::new(hook);
        let mut rollback_hook = || (hook.borrow_mut())(AtomicWriteStage::RecoveryRollbackRename);
        let mut sync_hook = || (hook.borrow_mut())(AtomicWriteStage::ParentDirectorySync);
        atomic_replace_locked(
            &destination,
            &bytes,
            expected_current,
            None,
            None,
            AtomicReplaceHooks {
                stage: &mut |_| Ok(()),
                before_parent_sync: &mut sync_hook,
                before_rollback_rename: &mut rollback_hook,
                rollback_on_sync_error: Some(rollback),
            },
        )?;
        Ok(())
    }
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    max_bytes: usize,
    exceeded: bool,
}

impl BoundedJsonWriter {
    fn new(max_bytes: u64) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes: usize::try_from(max_bytes).unwrap_or(usize::MAX),
            exceeded: false,
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.max_bytes {
            self.exceeded = true;
            return Err(io::Error::other("serialized document exceeds size limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn json_bytes_bounded(value: &impl Serialize, max_bytes: u64) -> Result<Vec<u8>, StorageError> {
    let mut writer = BoundedJsonWriter::new(max_bytes);
    let result = value.serialize(&mut serde_json::Serializer::pretty(&mut writer));
    if writer.exceeded {
        return Err(StorageError::DocumentTooLarge);
    }
    result?;
    if writer.write_all(b"\n").is_err() {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(writer.bytes)
}

fn read_optional_bounded(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, StorageError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(StorageError::DocumentTooLarge);
    }
    Ok(Some(bytes))
}
fn expected_current_matches(
    current: Option<&[u8]>,
    expected_current: CurrentExpectation<'_>,
) -> bool {
    match expected_current {
        CurrentExpectation::Unchecked => true,
        CurrentExpectation::Missing => current.is_none(),
        CurrentExpectation::Bytes(expected) => current.is_some_and(|current| current == expected),
    }
}

fn current_matches_revision(
    path: &Path,
    expected_current: CurrentExpectation<'_>,
) -> Result<bool, StorageError> {
    let expected_bytes = match expected_current {
        CurrentExpectation::Unchecked => return Ok(true),
        CurrentExpectation::Missing => None,
        CurrentExpectation::Bytes(bytes) => Some(bytes),
    };
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(expected_bytes.is_none());
        }
        Err(error) => return Err(error.into()),
    };
    let Some(expected_bytes) = expected_bytes else {
        return Ok(false);
    };
    let mut offset = 0;
    let mut buffer = [0; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(offset == expected_bytes.len());
        }
        let Some(end) = offset.checked_add(read) else {
            return Ok(false);
        };
        let Some(expected_chunk) = expected_bytes.get(offset..end) else {
            return Ok(false);
        };
        if &buffer[..read] != expected_chunk {
            return Ok(false);
        }
        offset = end;
    }
}

type ExistingValidator<'a> = (u64, &'a dyn Fn(&[u8]) -> Result<(), StorageError>);
type AtomicBackup<'a> = (&'a Path, ExistingValidator<'a>);

#[derive(Clone, Copy)]
enum CurrentExpectation<'a> {
    Unchecked,
    Missing,
    Bytes(&'a [u8]),
}

struct AtomicReplaceHooks<'a> {
    stage: &'a mut dyn FnMut(AtomicWriteStage) -> io::Result<()>,
    before_parent_sync: &'a mut dyn FnMut() -> io::Result<()>,
    before_rollback_rename: &'a mut dyn FnMut() -> io::Result<()>,
    rollback_on_sync_error: Option<ReplaceRollback>,
}

fn atomic_replace(
    destination: &Path,
    bytes: &[u8],
    expected_current: CurrentExpectation<'_>,
    validate_existing: Option<ExistingValidator<'_>>,
    backup: Option<AtomicBackup<'_>>,
    hook: &mut dyn FnMut(AtomicWriteStage) -> io::Result<()>,
) -> Result<(), StorageError> {
    let parent = destination
        .parent()
        .ok_or(StorageError::InvalidDirectories(
            "state file has no parent directory",
        ))?;
    create_directory_durable(parent)?;
    let directory_lock = DirectoryLock::new(parent);
    let _guard = directory_lock.lock()?;
    atomic_replace_locked(
        destination,
        bytes,
        expected_current,
        validate_existing,
        backup,
        AtomicReplaceHooks {
            stage: hook,
            before_parent_sync: &mut || Ok(()),
            before_rollback_rename: &mut || Ok(()),
            rollback_on_sync_error: None,
        },
    )
}

/// Atomically replaces state while the caller holds the destination directory lock.
fn atomic_replace_locked(
    destination: &Path,
    bytes: &[u8],
    expected_current: CurrentExpectation<'_>,
    validate_existing: Option<ExistingValidator<'_>>,
    backup: Option<AtomicBackup<'_>>,
    hooks: AtomicReplaceHooks<'_>,
) -> Result<(), StorageError> {
    let AtomicReplaceHooks {
        stage,
        before_parent_sync,
        before_rollback_rename,
        rollback_on_sync_error,
    } = hooks;
    let parent = destination
        .parent()
        .ok_or(StorageError::InvalidDirectories(
            "state file has no parent directory",
        ))?;
    let previous = match validate_existing {
        Some((limit, validate)) => {
            let previous = read_optional_bounded(destination, limit)?;
            let matches_expected = expected_current_matches(previous.as_deref(), expected_current);
            if !matches_expected {
                return Err(StorageError::StaleConfiguration);
            }
            if let Some(bytes) = previous.as_deref() {
                validate(bytes)?;
            }
            previous
        }
        None => {
            if !current_matches_revision(destination, expected_current)? {
                return Err(StorageError::StaleConfiguration);
            }
            None
        }
    };
    if let Some((backup_path, (limit, validate))) = backup {
        read_guarded_state(backup_path, limit, validate)?;
    }
    let mut temporary = create_sibling_temp(destination, "state")?;
    stage(AtomicWriteStage::TemporaryCreated)?;
    temporary.file_mut().write_all(bytes)?;
    temporary.file_mut().flush()?;
    temporary.file_mut().sync_all()?;
    temporary.close();
    stage(AtomicWriteStage::TemporarySynced)?;

    if let (Some((backup_path, _)), Some(old_bytes)) = (backup, previous.as_deref()) {
        let mut backup_hook = |nested_stage| {
            if nested_stage == AtomicWriteStage::TemporarySynced {
                stage(AtomicWriteStage::BackupTemporarySynced)?;
            }
            Ok(())
        };
        atomic_replace_locked(
            backup_path,
            old_bytes,
            CurrentExpectation::Unchecked,
            None,
            None,
            AtomicReplaceHooks {
                stage: &mut backup_hook,
                before_parent_sync: &mut || Ok(()),
                before_rollback_rename: &mut || Ok(()),
                rollback_on_sync_error: None,
            },
        )?;
        stage(AtomicWriteStage::BackupReplaced)?;
    }
    #[cfg(unix)]
    preserve_existing_file_permissions(temporary.path(), destination)?;
    fs::rename(temporary.path(), destination)?;
    temporary.disarm();
    if let Err(sync_error) = before_parent_sync().and_then(|()| sync_parent_directory(parent)) {
        if let Some(rollback) = rollback_on_sync_error {
            let rollback_result = match rollback.restore(destination, before_rollback_rename) {
                Ok(()) => sync_parent_directory(parent).map_err(|error| (error, None)),
                Err(error) => Err(error),
            };
            if let Err((rollback_error, retained_recovery_copy)) = rollback_result {
                let retained_recovery_copy = retained_recovery_copy
                    .and_then(|path| mark_recovery_copy_retained(destination, &path));
                return Err(StorageError::RecoveryRollbackFailed {
                    sync_error,
                    rollback_error,
                    retained_recovery_copy,
                });
            }
        }
        return Err(sync_error.into());
    }
    stage(AtomicWriteStage::TargetReplaced)?;
    Ok(())
}

enum ReplaceRollback {
    Restore(OwnedTempFile),
    Remove,
}

impl ReplaceRollback {
    fn restore(
        self,
        destination: &Path,
        before_restore: &mut dyn FnMut() -> io::Result<()>,
    ) -> Result<(), (io::Error, Option<PathBuf>)> {
        match self {
            Self::Restore(mut original) => {
                original.close();
                let retained_copy = original.path().to_path_buf();
                original.cleanup = false;
                match before_restore().and_then(|()| fs::rename(&retained_copy, destination)) {
                    Ok(()) => Ok(()),
                    Err(error) => Err((error, Some(retained_copy))),
                }
            }
            Self::Remove => match fs::remove_file(destination) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err((error, None)),
            },
        }
    }
}

fn prepare_replace_rollback(
    destination: &Path,
    max_bytes: u64,
) -> Result<ReplaceRollback, StorageError> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    cleanup_stale_recovery_copies(destination)?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if metadata.len() > max_bytes {
                return Err(StorageError::DocumentTooLarge);
            }
            let original = File::open(destination)?;
            let mut temporary = create_unique_sibling_temp(destination, "recovery-original")?;
            let copied = io::copy(
                &mut original.take(max_bytes.saturating_add(1)),
                temporary.file_mut(),
            )?;
            if copied > max_bytes {
                return Err(StorageError::DocumentTooLarge);
            }
            temporary.file_mut().flush()?;
            temporary.file_mut().sync_all()?;
            temporary.close();
            sync_parent_directory(parent)?;
            Ok(ReplaceRollback::Restore(temporary))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ReplaceRollback::Remove),
        Err(error) => Err(error.into()),
    }
}

/// Removes crash-orphaned rollback copies before a new rollback slot is allocated.
/// Explicitly retained copies use a distinct name and are never removed here.
fn cleanup_stale_recovery_copies(destination: &Path) -> Result<(), StorageError> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name"))?
        .to_string_lossy();
    let prefix = format!(".ascii-runtime-{name}.recovery-original-");
    let mut newest_unmatched = None;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(sequence) = recovery_copy_sequence(file_name, &prefix) else {
            continue;
        };
        if !entry.file_type()?.is_file() || is_recovery_copy_retained(destination, sequence) {
            continue;
        }
        if !files_have_same_bytes(destination, &entry.path())? {
            newest_unmatched =
                Some(newest_unmatched.map_or(sequence, |current: u64| current.max(sequence)));
        }
    }

    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(sequence) = recovery_copy_sequence(file_name, &prefix) else {
            continue;
        };
        if !entry.file_type()?.is_file() || is_recovery_copy_retained(destination, sequence) {
            continue;
        }
        if newest_unmatched == Some(sequence) && !files_have_same_bytes(destination, &entry.path())?
        {
            continue;
        }
        fs::remove_file(entry.path())?;
    }
    Ok(())
}

fn recovery_copy_sequence(file_name: &str, prefix: &str) -> Option<u64> {
    file_name
        .strip_prefix(prefix)?
        .strip_suffix(".tmp")?
        .parse()
        .ok()
}

fn is_recovery_copy_retained(destination: &Path, sequence: u64) -> bool {
    let retained_name = destination
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let retained_name = format!(".ascii-runtime-{retained_name}.recovery-retained-{sequence}.tmp");
    destination
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(retained_name)
        .exists()
        || retained_recovery_marker(destination, &sequence.to_string()).exists()
}

fn retained_recovery_marker(destination: &Path, sequence: &str) -> PathBuf {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    destination
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            ".ascii-runtime-{name}.recovery-retained-{sequence}.marker"
        ))
}

fn files_have_same_bytes(left: &Path, right: &Path) -> io::Result<bool> {
    let left_metadata = match fs::metadata(left) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let right_metadata = fs::metadata(right)?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0; 8192];
    let mut right_buffer = [0; 8192];
    let mut remaining = left_metadata.len();
    while remaining > 0 {
        let count = remaining.min(left_buffer.len() as u64) as usize;
        left.read_exact(&mut left_buffer[..count])?;
        right.read_exact(&mut right_buffer[..count])?;
        if left_buffer[..count] != right_buffer[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(true)
}

fn mark_recovery_copy_retained(destination: &Path, copy: &Path) -> Option<PathBuf> {
    let parent = destination.parent()?;
    mark_recovery_copy_retained_with(
        destination,
        copy,
        |source, retained| {
            fs::rename(source, retained)?;
            sync_parent_directory(parent)
        },
        |marker| {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(marker)?;
            file.sync_all()?;
            let parent = marker.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "recovery marker has no parent")
            })?;
            sync_parent_directory(parent)
        },
    )
}

fn mark_recovery_copy_retained_with(
    destination: &Path,
    copy: &Path,
    mut rename_copy: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut create_marker: impl FnMut(&Path) -> io::Result<()>,
) -> Option<PathBuf> {
    let name = destination.file_name()?;
    let copy_name = copy.file_name()?.to_str()?;
    let prefix = format!(
        ".ascii-runtime-{}.recovery-original-",
        name.to_string_lossy()
    );
    let sequence = copy_name.strip_prefix(&prefix)?.strip_suffix(".tmp")?;
    if sequence.is_empty() || !sequence.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parent = destination.parent()?;
    let retained = parent.join(format!(
        ".ascii-runtime-{}.recovery-retained-{sequence}.tmp",
        name.to_string_lossy()
    ));
    if rename_copy(copy, &retained).is_ok() {
        return Some(retained);
    }
    if !copy.exists() {
        return None;
    }
    create_marker(&retained_recovery_marker(destination, sequence)).ok()?;
    Some(copy.to_path_buf())
}

fn read_guarded_state(
    path: &Path,
    limit: u64,
    validate: &dyn Fn(&[u8]) -> Result<(), StorageError>,
) -> Result<Option<Vec<u8>>, StorageError> {
    let bytes = read_optional_bounded(path, limit)?;
    if let Some(bytes) = bytes.as_deref() {
        validate(bytes)?;
    }
    Ok(bytes)
}

struct OwnedTempFile {
    path: PathBuf,
    file: Option<File>,
    cleanup: bool,
}

impl OwnedTempFile {
    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary file is open")
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn close(&mut self) {
        self.file.take();
    }

    fn disarm(mut self) {
        self.close();
        self.cleanup = false;
    }
}

impl Drop for OwnedTempFile {
    fn drop(&mut self) {
        self.close();
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}
#[cfg(unix)]
fn preserve_existing_file_permissions(temporary: &Path, destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::set_permissions(temporary, metadata.permissions())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Uses one reserved slot per destination so retries clean crash leftovers without scanning a
/// directory or touching unrelated entries. The caller must hold the directory lock.
fn create_sibling_temp(destination: &Path, label: &str) -> io::Result<OwnedTempFile> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name"))?
        .to_string_lossy();
    let candidate = parent.join(format!(".ascii-runtime-{name}.{label}.tmp"));
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(&candidate)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "temporary state path is occupied by a non-file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    create_owned_sibling_temp(candidate)
}

fn create_unique_sibling_temp(destination: &Path, label: &str) -> io::Result<OwnedTempFile> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name"))?
        .to_string_lossy();
    static NEXT_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    if label == "recovery-original" {
        let original_prefix = format!(".ascii-runtime-{name}.recovery-original-");
        let retained_prefix = format!(".ascii-runtime-{name}.recovery-retained-");
        let mut next_sequence = 0;
        for entry in fs::read_dir(parent)? {
            let file_name = entry?.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let sequence = file_name
                .strip_prefix(&original_prefix)
                .and_then(|suffix| suffix.strip_suffix(".tmp"))
                .or_else(|| {
                    file_name
                        .strip_prefix(&retained_prefix)
                        .and_then(|suffix| suffix.strip_suffix(".tmp"))
                })
                .or_else(|| {
                    file_name
                        .strip_prefix(&retained_prefix)
                        .and_then(|suffix| suffix.strip_suffix(".marker"))
                })
                .and_then(|sequence| sequence.parse::<usize>().ok());
            if let Some(sequence) = sequence {
                next_sequence = next_sequence.max(sequence.saturating_add(1));
            }
        }
        NEXT_SEQUENCE.fetch_max(next_sequence, Ordering::Relaxed);
    }
    loop {
        let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".ascii-runtime-{name}.{label}-{sequence}.tmp"));
        match create_owned_sibling_temp(candidate) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            result => return result,
        }
    }
}

fn create_owned_sibling_temp(candidate: PathBuf) -> io::Result<OwnedTempFile> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&candidate)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let temporary = OwnedTempFile {
        path: candidate,
        file: Some(file),
        cleanup: true,
    };
    Ok(temporary)
}

fn create_directory_durable(path: &Path) -> io::Result<()> {
    let directory = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    let mut missing = Vec::new();
    let mut current = directory;
    loop {
        match fs::metadata(current) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "storage parent is not a directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
            }
            Err(error) => return Err(error),
        }
    }
    for created in missing.iter().rev() {
        create_storage_directory(created)?;
        let parent = created
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        sync_parent_directory(parent)?;
    }
    if missing.is_empty() {
        restrict_storage_directory(directory)?;
    }
    Ok(())
}

fn create_storage_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::metadata(path)?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "storage parent is not a directory",
                ));
            }
        }
        Err(error) => return Err(error),
    }
    restrict_storage_directory(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_storage_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_storage_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[derive(Clone, Debug)]
struct DirectoryLock {
    lock_path: PathBuf,
    local: Arc<Mutex<()>>,
}

impl DirectoryLock {
    fn new(directory: &Path) -> Self {
        let directory = normalized_path_for_io(directory);
        let identity = normalize_path_identity_case(directory.clone());
        let mut registry = DIRECTORY_LOCKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.retain(|_, lock| lock.strong_count() != 0);
        let local = match registry.get(&identity).and_then(Weak::upgrade) {
            Some(local) => local,
            None => {
                let local = Arc::new(Mutex::new(()));
                registry.insert(identity, Arc::downgrade(&local));
                local
            }
        };
        Self {
            lock_path: directory.join(".ascii-runtime-storage.lock"),
            local,
        }
    }

    fn lock(&self) -> io::Result<DirectoryLockGuard<'_>> {
        let local = self
            .local
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Keep a stable hidden file so separate processes serialize the same directory operations.
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock_file = options.open(&self.lock_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lock_file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        lock_file.lock()?;
        Ok(DirectoryLockGuard {
            _file: lock_file,
            _local: local,
        })
    }
}

struct DirectoryLockGuard<'a> {
    _file: File,
    _local: MutexGuard<'a, ()>,
}

#[cfg(unix)]
pub(crate) fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

/// One importer-owned transaction state, persisted under the staging root.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalState {
    Staged,
    Verified,
    Committed,
    Abandoned,
}

/// Versioned record sufficient to classify an interrupted package import.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalRecord {
    pub schema_version: u32,
    pub transaction_id: String,
    pub package_id: String,
    pub staging_path: String,
    pub state: JournalState,
}

/// Action required when a process resumes after a transaction interruption.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    ValidateStaged,
    CommitVerified,
    AlreadyCommitted,
    CleanupAbandoned,
}

/// Durable importer journal with constrained, serialized state transitions.
#[derive(Clone, Debug)]
pub struct ImportJournal {
    staging_root: PathBuf,
    directory_lock: DirectoryLock,
}

impl ImportJournal {
    pub fn new(staging_root: impl Into<PathBuf>) -> Self {
        let staging_root = staging_root.into().join("import-journal");
        let directory_lock = DirectoryLock::new(&staging_root);
        Self {
            staging_root,
            directory_lock,
        }
    }

    /// Starts a transaction in `staged` before extraction/validation proceeds.
    pub fn begin(
        &self,
        transaction_id: &str,
        package_id: &str,
        staging_path: &str,
    ) -> Result<JournalRecord, StorageError> {
        validate_application_id(transaction_id)?;
        validate_application_id(package_id)?;
        if !is_safe_relative_path(staging_path) {
            return Err(StorageError::InvalidJournal(
                "staging path must be normalized and relative",
            ));
        }
        // A serialized path includes its UTF-8 bytes, so longer inputs cannot fit the journal.
        // Reject them before cloning; the bounded writer still checks the complete record.
        if staging_path.len() > usize::try_from(JOURNAL_MAX_BYTES).unwrap_or(usize::MAX) {
            return Err(StorageError::DocumentTooLarge);
        }

        let path = self.record_path(transaction_id)?;
        create_directory_durable(&self.staging_root)?;
        let _guard = self.directory_lock.lock()?;
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                return Err(StorageError::InvalidJournal("transaction already exists"));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let record = JournalRecord {
            schema_version: 1,
            transaction_id: transaction_id.to_owned(),
            package_id: package_id.to_owned(),
            staging_path: staging_path.to_owned(),
            state: JournalState::Staged,
        };
        self.write_locked(&path, &record)?;
        Ok(record)
    }

    pub fn load(&self, transaction_id: &str) -> Result<Option<JournalRecord>, StorageError> {
        let path = self.record_path(transaction_id)?;
        let bytes = match read_optional_bounded(&path, JOURNAL_MAX_BYTES)? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        let record: JournalRecord = serde_json::from_slice(&bytes)?;
        self.validate_record(transaction_id, &record)?;
        Ok(Some(record))
    }

    /// Persists only staged→verified/abandoned and verified→committed/abandoned transitions.
    /// Concurrent transitions targeting the same journal directory are serialized.
    pub fn transition(
        &self,
        transaction_id: &str,
        next: JournalState,
    ) -> Result<JournalRecord, StorageError> {
        let path = self.record_path(transaction_id)?;
        match fs::metadata(&self.staging_root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(StorageError::Io(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "journal root is not a directory",
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StorageError::InvalidJournal("transaction does not exist"));
            }
            Err(error) => return Err(error.into()),
        }
        let _guard = self.directory_lock.lock()?;
        let mut record = self
            .load(transaction_id)?
            .ok_or(StorageError::InvalidJournal("transaction does not exist"))?;
        let allowed = matches!(
            (record.state, next),
            (
                JournalState::Staged,
                JournalState::Verified | JournalState::Abandoned
            ) | (
                JournalState::Verified,
                JournalState::Committed | JournalState::Abandoned
            )
        );
        if !allowed {
            return Err(StorageError::InvalidJournal(
                "journal transition is not allowed",
            ));
        }
        record.state = next;
        self.write_locked(&path, &record)?;
        Ok(record)
    }

    /// Classifies persisted work without silently changing or deleting an interrupted transaction.
    pub fn recover(&self, transaction_id: &str) -> Result<Option<RecoveryAction>, StorageError> {
        Ok(self.load(transaction_id)?.map(|record| match record.state {
            JournalState::Staged => RecoveryAction::ValidateStaged,
            JournalState::Verified => RecoveryAction::CommitVerified,
            JournalState::Committed => RecoveryAction::AlreadyCommitted,
            JournalState::Abandoned => RecoveryAction::CleanupAbandoned,
        }))
    }

    fn record_path(&self, transaction_id: &str) -> Result<PathBuf, StorageError> {
        validate_application_id(transaction_id)?;
        Ok(self
            .staging_root
            .join(format!("import-{transaction_id}.json")))
    }

    fn validate_record(
        &self,
        transaction_id: &str,
        record: &JournalRecord,
    ) -> Result<(), StorageError> {
        if record.schema_version != 1 {
            return Err(StorageError::UnsupportedVersion {
                kind: "import journal",
                version: record.schema_version,
            });
        }
        if record.transaction_id != transaction_id {
            return Err(StorageError::InvalidJournal(
                "record identifier does not match its file",
            ));
        }
        validate_application_id(&record.package_id)?;
        if !is_safe_relative_path(&record.staging_path) {
            return Err(StorageError::InvalidJournal(
                "staging path is not normalized and relative",
            ));
        }
        Ok(())
    }

    fn write_locked(&self, path: &Path, record: &JournalRecord) -> Result<(), StorageError> {
        self.validate_record(&record.transaction_id, record)?;
        atomic_replace_locked(
            path,
            &json_bytes_bounded(record, JOURNAL_MAX_BYTES)?,
            CurrentExpectation::Unchecked,
            None,
            None,
            AtomicReplaceHooks {
                stage: &mut |_| Ok(()),
                before_parent_sync: &mut || Ok(()),
                before_rollback_rename: &mut || Ok(()),
                rollback_on_sync_error: None,
            },
        )?;
        Ok(())
    }
}

/// Portable or explicitly labeled local-backup export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportMode {
    Portable,
    NonportableLocalBackup,
}

/// Filtered export payload; local backups are visibly distinguished from portable exports.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PortableExport {
    schema_version: u32,
    mode: &'static str,
    content: Value,
}

impl PortableExport {
    /// Filters an export within fixed work and output bounds.
    ///
    /// Returns [`StorageError::DocumentTooLarge`] if the input exceeds its traversal, string, key,
    /// or serialized-output budget; it never returns a partial export.
    pub fn from_value(value: &Value, mode: ExportMode) -> Result<Self, StorageError> {
        let label = match mode {
            ExportMode::Portable => "portable",
            ExportMode::NonportableLocalBackup => "nonportable-local-backup",
        };
        let mut budget = ExportBudget::new();
        let mut key_path = String::new();
        let content =
            filter_export(value, mode, &mut budget, 0, &mut key_path)?.unwrap_or(Value::Null);
        Ok(Self {
            schema_version: 1,
            mode: label,
            content,
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn mode(&self) -> &str {
        self.mode
    }

    pub fn content(&self) -> &Value {
        &self.content
    }

    pub fn to_json(&self) -> Result<Vec<u8>, StorageError> {
        json_bytes_bounded(self, PORTABLE_EXPORT_MAX_BYTES as u64)
    }
}

struct ExportBudget {
    visited_nodes: usize,
    estimated_output_bytes: usize,
}

impl ExportBudget {
    fn new() -> Self {
        Self {
            visited_nodes: 0,
            estimated_output_bytes: 256,
        }
    }

    fn visit_node(&mut self, depth: usize) -> Result<(), StorageError> {
        if depth > PORTABLE_EXPORT_MAX_DEPTH || self.visited_nodes >= PORTABLE_EXPORT_MAX_NODES {
            return Err(StorageError::DocumentTooLarge);
        }
        self.visited_nodes += 1;
        let indentation = depth.checked_mul(2).ok_or(StorageError::DocumentTooLarge)?;
        self.account(32usize.saturating_add(indentation))
    }

    fn visit_entry(&mut self) -> Result<(), StorageError> {
        if self.visited_nodes >= PORTABLE_EXPORT_MAX_NODES {
            return Err(StorageError::DocumentTooLarge);
        }
        self.visited_nodes += 1;
        Ok(())
    }

    fn account_string(&mut self, bytes: usize) -> Result<(), StorageError> {
        let escaped = bytes
            .checked_mul(6)
            .and_then(|size| size.checked_add(2))
            .ok_or(StorageError::DocumentTooLarge)?;
        self.account(escaped)
    }

    fn account_key(&mut self, bytes: usize, depth: usize) -> Result<(), StorageError> {
        let indentation = depth.checked_mul(2).ok_or(StorageError::DocumentTooLarge)?;
        let escaped = bytes
            .checked_mul(6)
            .and_then(|size| size.checked_add(16))
            .and_then(|size| size.checked_add(indentation))
            .ok_or(StorageError::DocumentTooLarge)?;
        self.account(escaped)
    }

    fn account(&mut self, bytes: usize) -> Result<(), StorageError> {
        self.estimated_output_bytes = self
            .estimated_output_bytes
            .checked_add(bytes)
            .filter(|total| *total <= PORTABLE_EXPORT_MAX_BYTES)
            .ok_or(StorageError::DocumentTooLarge)?;
        Ok(())
    }
}

fn filter_export(
    value: &Value,
    mode: ExportMode,
    budget: &mut ExportBudget,
    depth: usize,
    key_path: &mut String,
) -> Result<Option<Value>, StorageError> {
    budget.visit_node(depth)?;
    match value {
        Value::Object(object) => {
            let mut filtered = serde_json::Map::new();
            for (key, nested) in object {
                budget.visit_entry()?;
                if key.len() > PORTABLE_EXPORT_MAX_KEY_BYTES {
                    return Err(StorageError::DocumentTooLarge);
                }
                let key_path_length = key_path.len();
                append_normalized_key(key_path, key);
                if is_normalized_history_key(key_path)
                    || (mode == ExportMode::Portable && is_nonportable_key(key))
                {
                    key_path.truncate(key_path_length);
                    continue;
                }
                let nested = filter_export(nested, mode, budget, depth + 1, key_path)?;
                key_path.truncate(key_path_length);
                if let Some(nested) = nested {
                    budget.account_key(key.len(), depth)?;
                    filtered.insert(key.clone(), nested);
                }
            }
            Ok(Some(Value::Object(filtered)))
        }
        Value::Array(items) => {
            let mut filtered = Vec::new();
            for item in items {
                if let Some(item) = filter_export(item, mode, budget, depth + 1, key_path)? {
                    filtered.push(item);
                }
            }
            Ok(Some(Value::Array(filtered)))
        }
        Value::String(text) => {
            if text.len() > PORTABLE_EXPORT_MAX_STRING_BYTES {
                return Err(StorageError::DocumentTooLarge);
            }
            budget.account_string(text.len())?;
            if mode == ExportMode::Portable && is_machine_local_string(text) {
                Ok(None)
            } else {
                Ok(Some(Value::String(text.clone())))
            }
        }
        Value::Number(number) => Ok(Some(Value::Number(number.clone()))),
        Value::Bool(value) => Ok(Some(Value::Bool(*value))),
        Value::Null => Ok(Some(Value::Null)),
    }
}

fn append_normalized_key(path: &mut String, key: &str) {
    path.extend(
        key.chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase),
    );
}

fn normalize_key(key: &str) -> String {
    let mut normalized = String::with_capacity(key.len());
    append_normalized_key(&mut normalized, key);
    normalized
}

fn is_normalized_history_key(key: &str) -> bool {
    (key.contains("cursor") && key.contains("history"))
        || (key.contains("window") && key.contains("geometry") && key.contains("history"))
}

fn has_host_component(key: &str) -> bool {
    let mut component_start = None;
    let mut previous = None;
    let mut characters = key.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if !character.is_ascii_alphanumeric() {
            if let Some(start) = component_start.take()
                && matches_host_component(&key[start..index])
            {
                return true;
            }
            previous = None;
            continue;
        }

        let next_character = characters.peek().map(|(_, character)| *character);
        let camel_case_boundary = previous.is_some_and(|previous: char| {
            ((previous.is_ascii_lowercase() || previous.is_ascii_digit())
                && character.is_ascii_uppercase())
                || (previous.is_ascii_alphabetic() && character.is_ascii_digit())
                || (previous.is_ascii_uppercase()
                    && character.is_ascii_uppercase()
                    && next_character.is_some_and(|character| character.is_ascii_lowercase()))
        });
        if camel_case_boundary {
            if let Some(start) = component_start
                && matches_host_component(&key[start..index])
            {
                return true;
            }
            component_start = Some(index);
        } else if component_start.is_none() {
            component_start = Some(index);
        }
        previous = Some(character);
    }

    component_start.is_some_and(|start| matches_host_component(&key[start..]))
}

fn matches_host_component(component: &str) -> bool {
    component.eq_ignore_ascii_case("host") || component.eq_ignore_ascii_case("hostname")
}

fn is_nonportable_key(key: &str) -> bool {
    let has_host_component = has_host_component(key);
    let key = normalize_key(key);
    key.contains("path")
        || key.contains("monitor")
        || key.contains("display")
        || key.contains("screen")
        || key.contains("permission")
        || key.contains("apprule")
        || key.contains("applicationrule")
        || key.contains("endpoint")
        || key.contains("socket")
        || key.contains("ipc")
        || key.contains("user")
        || key.contains("home")
        || key.contains("address")
        || has_host_component
        || matches!(key.as_str(), "host" | "hostname")
        || key.contains("server")
        || key.contains("connection")
        || key == "port"
}

fn is_machine_local_string(value: &str) -> bool {
    if value.contains("://") || contains_file_uri(value) {
        return true;
    }
    value.split_whitespace().any(|token| {
        let token = trim_export_punctuation(token);
        contains_windows_drive_relative_path(token)
            || token.split([':', '=']).any(|part| {
                let part = trim_export_punctuation(part);
                part.starts_with("~/")
                    || looks_like_user_home_path(part)
                    || part.starts_with('/')
                    || part.starts_with("\\\\")
                    || part.contains('\\')
                    || looks_like_relative_local_path(part)
            })
            || looks_like_network_endpoint(token)
    })
}

fn contains_windows_drive_relative_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut previous_character = None;
    for (index, character) in value.char_indices() {
        let next_byte = bytes.get(index + 1);
        let path_byte = bytes.get(index + 2);
        if character.is_ascii_alphabetic()
            && next_byte == Some(&b':')
            && path_byte.is_some_and(|byte| {
                *byte != b'/' && *byte != b'\\' && *byte != b':' && !byte.is_ascii_whitespace()
            })
            && previous_character
                .is_none_or(|previous: char| !previous.is_alphanumeric() && previous != '_')
        {
            return true;
        }
        previous_character = Some(character);
    }
    false
}

fn looks_like_relative_local_path(value: &str) -> bool {
    let mut components = value.split(['/', '\\']);
    let Some(first) = components.next() else {
        return false;
    };
    let Some(second) = components.next() else {
        return false;
    };
    if first.is_empty() || second.is_empty() || components.next().is_some() {
        return true;
    }

    let benign_input_output =
        first.eq_ignore_ascii_case("input") && second.eq_ignore_ascii_case("output");
    let numeric_ratio = is_numeric_component(first) && is_numeric_component(second);
    let version_pair = is_version_component(first) && is_version_component(second);
    !(benign_input_output || numeric_ratio || version_pair)
}

fn is_numeric_component(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_version_component(value: &str) -> bool {
    let Some(version) = value.strip_prefix('v').or_else(|| value.strip_prefix('V')) else {
        return false;
    };
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    !major.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_unicode_quote_boundary(character: char) -> bool {
    matches!(
        CodePointMapData::<GeneralCategory>::new().get(character),
        GeneralCategory::InitialPunctuation | GeneralCategory::FinalPunctuation
    )
}

fn trim_export_punctuation(value: &str) -> &str {
    value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\''
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '='
                | ','
                | ';'
                | '.'
                | '!'
                | '<'
                | '>'
                | '「'
                | '」'
                | '『'
                | '』'
                | '〝'
                | '〞'
                | '〟'
                | '﹁'
                | '﹂'
                | '﹃'
                | '﹄'
                | '＂'
                | '＇'
                | '⹂'
                | '❛'
                | '❜'
                | '❝'
                | '❞'
                | '❮'
                | '❯'
                | '｢'
                | '｣'
        ) || is_unicode_quote_boundary(character)
    })
}

fn looks_like_user_home_path(token: &str) -> bool {
    let Some((username, _)) = token
        .strip_prefix('~')
        .and_then(|path| path.split_once(['/', '\\']))
    else {
        return false;
    };

    !username.is_empty()
        && username.chars().any(char::is_alphanumeric)
        && username
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

fn contains_file_uri(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut previous_is_boundary = true;
    for (index, character) in value.char_indices() {
        if previous_is_boundary
            && index + 5 <= bytes.len()
            && bytes[index..index + 5].eq_ignore_ascii_case(b"file:")
        {
            return true;
        }
        previous_is_boundary = character.is_whitespace()
            || matches!(
                character,
                '=' | ':' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '.'
            );
    }
    false
}

fn looks_like_network_endpoint(token: &str) -> bool {
    let candidate = token.split(['/', '\\', '?', '#']).next().unwrap_or(token);
    let candidate = candidate.strip_suffix(':').unwrap_or(candidate);
    if candidate
        .trim_matches(|character| matches!(character, '[' | ']'))
        .parse::<std::net::IpAddr>()
        .is_ok()
        || candidate.eq_ignore_ascii_case("localhost")
    {
        return true;
    }
    let Some((host, port)) = candidate.rsplit_once(':') else {
        return false;
    };
    if port.parse::<u16>().is_err() {
        return false;
    }
    !host
        .trim_matches(|character| matches!(character, '[' | ']'))
        .is_empty()
}

/// Hard caps for the diagnostic log and each individual record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogLimits {
    max_file_bytes: usize,
    max_files: usize,
    max_record_bytes: usize,
}

impl LogLimits {
    pub fn new(
        max_file_bytes: usize,
        max_files: usize,
        max_record_bytes: usize,
    ) -> Result<Self, StorageError> {
        if max_file_bytes == 0
            || max_file_bytes > MAX_LOG_FILE_BYTES
            || max_files == 0
            || max_files > 16
            || max_record_bytes == 0
            || max_record_bytes > max_file_bytes
            || max_record_bytes > MAX_LOG_RECORD_BYTES
        {
            return Err(StorageError::InvalidLogLimits);
        }
        Ok(Self {
            max_file_bytes,
            max_files,
            max_record_bytes,
        })
    }
}

/// Bounded rotating text log that redacts home paths, media filenames, and clipped messages.
#[derive(Debug)]
pub struct DiagnosticLog {
    directory: PathBuf,
    home: PathBuf,
    limits: LogLimits,
    directory_lock: DirectoryLock,
}

impl DiagnosticLog {
    pub fn new(directory: impl Into<PathBuf>, home: impl Into<PathBuf>, limits: LogLimits) -> Self {
        let directory = directory.into();
        let directory_lock = DirectoryLock::new(&directory);
        Self {
            directory,
            home: home.into(),
            limits,
            directory_lock,
        }
    }
    pub fn append(&self, message: &str) -> Result<(), StorageError> {
        create_directory_durable(&self.directory)?;
        let _guard = self.directory_lock.lock()?;
        self.prune_rotation_files()?;

        let (redaction_input, clipped) = bounded_log_input(message, self.limits.max_record_bytes);
        let redacted = if clipped {
            "[REDACTED_TRUNCATED_EVENT]".to_owned()
        } else {
            redact_log_message(redaction_input, &self.home)
        };
        let max_payload = self.limits.max_record_bytes.saturating_sub(1);
        let redacted = truncate_utf8(&redacted, max_payload);
        let mut record = redacted.as_bytes().to_vec();
        record.push(b'\n');
        if record.len() > self.limits.max_file_bytes {
            record.truncate(self.limits.max_file_bytes);
            if let Some(last) = record.last_mut() {
                *last = b'\n';
            }
        }
        let path = self.directory.join("runtime.log");
        let existing_len = match fs::metadata(&path) {
            Ok(metadata) => usize::try_from(metadata.len()).unwrap_or(usize::MAX),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        if existing_len.saturating_add(record.len()) > self.limits.max_file_bytes {
            self.rotate(&path)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&record)?;
        file.flush()?;
        Ok(())
    }

    pub fn current_path(&self) -> PathBuf {
        self.directory.join("runtime.log")
    }

    fn rotate(&self, current: &Path) -> Result<(), StorageError> {
        if self.limits.max_files == 1 {
            remove_if_present(current)?;
            return Ok(());
        }
        let oldest = self
            .directory
            .join(format!("runtime.log.{}", self.limits.max_files - 1));
        remove_if_present(&oldest)?;
        for index in (2..self.limits.max_files).rev() {
            let source = self.directory.join(format!("runtime.log.{}", index - 1));
            let destination = self.directory.join(format!("runtime.log.{index}"));
            rename_if_present_bounded(&source, &destination, self.limits.max_file_bytes)?;
        }
        rename_if_present_bounded(
            current,
            &self.directory.join("runtime.log.1"),
            self.limits.max_file_bytes,
        )?;
        Ok(())
    }
    fn prune_rotation_files(&self) -> Result<(), StorageError> {
        for index in self.limits.max_files..16 {
            remove_if_present(&self.directory.join(format!("runtime.log.{index}")))?;
        }
        Ok(())
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rename_if_present_bounded(source: &Path, destination: &Path, limit: usize) -> io::Result<()> {
    match fs::metadata(source) {
        Ok(metadata) if metadata.len() > limit as u64 => remove_if_present(source),
        Ok(_) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(source, fs::Permissions::from_mode(0o600))?;
            }
            fs::rename(source, destination)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn bounded_log_input(message: &str, max_bytes: usize) -> (&str, bool) {
    let input = truncate_utf8(message, max_bytes);
    (input, input.len() != message.len())
}

fn redact_log_message(message: &str, home: &Path) -> String {
    let mut redacted = message.replace(&['\r', '\n'][..], " ");
    let home_text = home.to_string_lossy();
    if !home_text.is_empty() {
        let slash = home_text.replace('\\', "/");
        let backslash = home_text.replace('/', "\\");
        for variant in [home_text.as_ref(), slash.as_str(), backslash.as_str()] {
            redacted = replace_ascii_case_insensitive(&redacted, variant, "[REDACTED_HOME]");
        }
    }
    if contains_media_extension(&redacted) {
        return "[REDACTED_MEDIA_EVENT]".to_owned();
    }
    let mut output = String::with_capacity(redacted.len());
    let mut cursor = 0;
    while cursor < redacted.len() {
        let next_space = redacted[cursor..]
            .find(char::is_whitespace)
            .map(|offset| cursor + offset)
            .unwrap_or(redacted.len());
        let token = &redacted[cursor..next_space];
        if token.contains('/') || token.contains('\\') {
            output.push_str("[REDACTED_PATH]");
            return output;
        }
        if contains_windows_drive_relative_path(token) {
            output.push_str("[REDACTED_PATH]");
            let mut token_chars = token.chars();
            let bounded_by_quotes = matches!(
                (token_chars.next(), token_chars.next_back()),
                (Some('"'), Some('"'))
                    | (Some('\''), Some('\''))
                    | (Some('“'), Some('”'))
                    | (Some('‘'), Some('’'))
            );
            if !bounded_by_quotes {
                return output;
            }
        } else {
            output.push_str(token);
        }
        if next_space < redacted.len() {
            let whitespace = redacted[next_space..].chars().next().unwrap_or(' ');
            output.push(whitespace);
            cursor = next_space + whitespace.len_utf8();
        } else {
            cursor = redacted.len();
        }
    }
    output
}

fn replace_ascii_case_insensitive(input: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > input.len() {
        return input.to_owned();
    }
    let bytes = input.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut copied = 0;
    for (index, _) in input.char_indices() {
        let end = index + needle_bytes.len();
        if index >= copied
            && end <= bytes.len()
            && bytes[index..end].eq_ignore_ascii_case(needle_bytes)
        {
            output.push_str(&input[copied..index]);
            output.push_str(replacement);
            copied = end;
        }
    }
    output.push_str(&input[copied..]);
    output
}

const MEDIA_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff", ".avif", ".heic", ".heif",
    ".svg", ".psd", ".dng", ".raw", ".nef", ".cr2", ".arw", ".mp4", ".mov", ".mkv", ".webm",
    ".avi", ".m4v", ".mpg", ".mpeg", ".wmv", ".3gp", ".mts", ".m2ts", ".ts", ".mp3", ".m4a",
    ".wav", ".flac", ".ogg", ".aac", ".aiff", ".opus",
];

fn contains_media_extension(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    MEDIA_EXTENSIONS
        .iter()
        .any(|extension| lower.contains(extension))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
#[cfg(test)]
mod tests {
    use super::{
        bounded_log_input, cleanup_stale_recovery_copies, mark_recovery_copy_retained_with,
        redact_log_message,
    };
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    fn temporary_storage_test_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ascii-runtime-storage-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("create storage test directory");
        root
    }

    #[test]
    fn failed_retention_mark_never_reports_a_copy_cleanup_can_delete() {
        let root = temporary_storage_test_root();
        let destination = root.join("state.json");
        fs::write(&destination, b"new state").expect("write current destination");
        for sequence in 1..=64 {
            fs::write(
                root.join(format!(
                    ".ascii-runtime-state.json.recovery-original-{sequence}.tmp"
                )),
                b"older crash orphan",
            )
            .expect("seed crash orphan");
        }
        let copy = root.join(".ascii-runtime-state.json.recovery-original-65.tmp");
        fs::write(&copy, b"recoverable original").expect("write recovery copy");

        let reported = mark_recovery_copy_retained_with(
            &destination,
            &copy,
            |_, _| Err(io::Error::other("injected retained rename failure")),
            |_| Err(io::Error::other("injected marker creation failure")),
        );
        assert!(
            reported.is_none(),
            "unmarked copies must not be reported retained"
        );

        cleanup_stale_recovery_copies(&destination).expect("clean stale recovery files");
        assert!(
            reported.as_ref().is_none_or(|path| path.exists()),
            "cleanup must not delete a reported recoverable copy"
        );
        assert!(copy.exists(), "preserve the newest unmarked original");
        let leftovers = fs::read_dir(&root)
            .expect("read recovery directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".recovery-original-")
            })
            .count();
        assert_eq!(leftovers, 1, "repeated leftovers remain bounded");
        fs::remove_dir_all(root).expect("remove storage test directory");
    }

    #[test]
    fn diagnostic_redaction_receives_bounded_input_and_hides_partial_path_values() {
        let message = format!("path={}", "private".repeat(1024 * 1024));
        let (input, clipped) = bounded_log_input(&message, 32);
        assert_eq!(input.len(), 32);
        assert!(clipped);

        let redacted = if clipped {
            "[REDACTED_TRUNCATED_EVENT]".to_owned()
        } else {
            redact_log_message(input, Path::new("/home/user"))
        };
        assert!(!redacted.contains("private"));
    }

    #[test]
    fn diagnostic_redaction_hides_windows_drive_relative_paths() {
        for input in ["C:private.txt", "note=C:private.txt"] {
            let redacted = redact_log_message(input, Path::new("/home/user"));
            assert!(redacted.contains("[REDACTED_PATH]"));
            assert!(!redacted.contains("private.txt"));
        }
    }
}
