use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::{
    document::{CURRENT_SCHEMA_VERSION, DecodedDocument, DocumentError, decode, encode},
    error::ConfigError,
    migrate::{MigrationError, migrate},
    model::{SettingKey, SettingScope, SettingValue},
    registry::Registry,
    resolve::{Constraints, EffectiveSettings, SettingLayers, SettingSource, resolve_settings},
    store::{BackupMode, ConfigStore, FileStore, StoreError},
};

/// Whether the active config may be changed by the current application.
#[derive(Clone, Debug, PartialEq)]
pub enum ControllerStatus {
    Ready,
    RepairRequired(ConfigError),
    ReadOnlyFutureSchema { schema_version: i64 },
    ReadOnlyUnclassifiableDocument(ConfigError),
}

/// A complete, immutable settings revision published by the controller.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsSnapshot {
    revision: u64,
    effective: EffectiveSettings,
    overrides: Arc<BTreeMap<SettingKey, SettingValue>>,
}

impl SettingsSnapshot {
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn effective(&self) -> &EffectiveSettings {
        &self.effective
    }

    /// Explicit persisted requests only; omitted keys continue to use the
    /// registry's source-owned defaults.
    #[must_use]
    pub fn overrides(&self) -> &BTreeMap<SettingKey, SettingValue> {
        &self.overrides
    }
}

/// One atomic persistent edit. Reset removes the override and exposes the
/// registry default again.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingChange {
    Set(SettingValue),
    Reset,
}

/// The sole writer for one active settings path. A mutex serializes revision
/// checks, validation, disk replacement, and immutable snapshot publication.
pub struct SettingsController {
    registry: Arc<Registry>,
    state: Mutex<ControllerState>,
}

struct ControllerState {
    store: Box<dyn ConfigStore>,
    status: ControllerStatus,
    snapshot: Arc<SettingsSnapshot>,
    disk_bytes: Option<Vec<u8>>,
}

struct OpenDocumentError {
    error: ConfigError,
    can_repair: bool,
    revision: Option<u64>,
}

impl SettingsController {
    /// Opens canonical TOML settings at the path selected by the platform-aware
    /// application path provider. This crate does not guess OS config paths.
    pub fn open(path: impl AsRef<Path>, registry: Arc<Registry>) -> Result<Self, ConfigError> {
        Self::open_with_store(registry, Box::new(FileStore::new(path)))
    }

    fn open_with_store(
        registry: Arc<Registry>,
        mut store: Box<dyn ConfigStore>,
    ) -> Result<Self, ConfigError> {
        let defaults = build_snapshot(&registry, 0, BTreeMap::new())?;
        let (status, snapshot, disk_bytes) = match store.read_main() {
            Ok(None) => (ControllerStatus::Ready, defaults, None),
            Ok(Some(bytes)) => match decode_and_validate(&registry, &bytes) {
                Ok(document) if document.schema_version > CURRENT_SCHEMA_VERSION => {
                    let snapshot = build_snapshot(&registry, document.revision, BTreeMap::new())?;
                    (
                        ControllerStatus::ReadOnlyFutureSchema {
                            schema_version: document.schema_version,
                        },
                        snapshot,
                        Some(bytes),
                    )
                }
                Ok(document) if document.schema_version < CURRENT_SCHEMA_VERSION => {
                    let migrated = migrate(document).map_err(map_migration_error)?;
                    validate_overrides(&registry, &migrated.overrides)?;
                    let effective = resolve_overrides(&registry, &migrated.overrides)?;
                    let revision = migrated
                        .revision
                        .checked_add(1)
                        .filter(|revision| *revision <= i64::MAX as u64)
                        .ok_or(ConfigError::RevisionOverflow)?;
                    let serialized = encode(revision, &migrated.overrides)?;
                    store.commit(Some(&bytes), &serialized, BackupMode::ReplaceWithPrevious)?;
                    (
                        ControllerStatus::Ready,
                        Arc::new(SettingsSnapshot {
                            revision,
                            effective,
                            overrides: Arc::new(migrated.overrides),
                        }),
                        Some(serialized),
                    )
                }
                Ok(document) => {
                    validate_overrides(&registry, &document.overrides)?;
                    let effective = resolve_overrides(&registry, &document.overrides)?;
                    (
                        ControllerStatus::Ready,
                        Arc::new(SettingsSnapshot {
                            revision: document.revision,
                            effective,
                            overrides: Arc::new(document.overrides),
                        }),
                        Some(bytes),
                    )
                }
                Err(failure) => {
                    let snapshot = match failure.revision {
                        Some(revision) => build_snapshot(&registry, revision, BTreeMap::new())?,
                        None => defaults,
                    };
                    let status = if failure.can_repair {
                        ControllerStatus::RepairRequired(failure.error)
                    } else {
                        ControllerStatus::ReadOnlyUnclassifiableDocument(failure.error)
                    };
                    (status, snapshot, Some(bytes))
                }
            },
            Err(StoreError::TooLarge { limit, .. }) => (
                ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
                    DocumentError::TooLarge { limit },
                )),
                defaults,
                None,
            ),
            Err(error) => return Err(ConfigError::Storage(error)),
        };
        Ok(Self {
            registry,
            state: Mutex::new(ControllerState {
                store,
                status,
                snapshot,
                disk_bytes,
            }),
        })
    }

    /// Returns the currently published immutable revision.
    pub fn snapshot(&self) -> Result<Arc<SettingsSnapshot>, ConfigError> {
        Ok(self.lock()?.snapshot.clone())
    }

    /// Returns whether changes are accepted, require repair, or blocked by a
    /// future schema or unclassifiable document.
    pub fn status(&self) -> Result<ControllerStatus, ConfigError> {
        Ok(self.lock()?.status.clone())
    }

    /// Validates an entire batch before a single durable write. A conflict
    /// contains the currently observed revision; a failed write never publishes
    /// the candidate snapshot.
    pub fn apply_batch(
        &self,
        expected_revision: u64,
        changes: &[(SettingKey, SettingChange)],
    ) -> Result<Arc<SettingsSnapshot>, ConfigError> {
        let mut state = self.lock()?;
        ensure_writable(&state.status)?;
        ensure_expected(expected_revision, state.snapshot.revision)?;
        ensure_disk_matches(&mut state)?;
        if changes.is_empty() {
            return Ok(state.snapshot.clone());
        }

        let mut seen = BTreeSet::new();
        let mut overrides = (*state.snapshot.overrides).clone();
        for (key, change) in changes {
            if !seen.insert(key.clone()) {
                return Err(ConfigError::DuplicateBatchKey(key.clone()));
            }
            let Some(descriptor) = self.registry.get(key) else {
                return Err(ConfigError::UnknownSetting(key.clone()));
            };
            if descriptor.scope == SettingScope::S {
                return Err(ConfigError::SessionSettingCannotPersist(key.clone()));
            }
            match change {
                SettingChange::Set(value) => {
                    descriptor
                        .value_type
                        .validates(value)
                        .map_err(|violation| ConfigError::InvalidPersistentValue {
                            key: key.clone(),
                            violation,
                        })?;
                    overrides.insert(key.clone(), value.clone());
                }
                SettingChange::Reset => {
                    overrides.remove(key);
                }
            }
        }
        validate_overrides(&self.registry, &overrides)?;
        let effective = resolve_overrides(&self.registry, &overrides)?;
        let revision = next_revision(state.snapshot.revision)?;
        let bytes = encode(revision, &overrides)?;
        commit_state(&mut state, &bytes, BackupMode::ReplaceWithPrevious)?;
        let snapshot = Arc::new(SettingsSnapshot {
            revision,
            effective,
            overrides: Arc::new(overrides),
        });
        state.disk_bytes = Some(bytes);
        state.snapshot = snapshot.clone();
        Ok(snapshot)
    }

    /// Replaces invalid data in a fully parsed, supported-schema document with
    /// registry defaults only after explicit caller action. The file store
    /// preserves the last-good backup and archives the replaced document.
    pub fn repair(&self, expected_revision: u64) -> Result<Arc<SettingsSnapshot>, ConfigError> {
        let mut state = self.lock()?;
        if !matches!(&state.status, ControllerStatus::RepairRequired(_)) {
            return ensure_writable(&state.status).and(Err(ConfigError::NotRepairable));
        }
        ensure_expected(expected_revision, state.snapshot.revision)?;
        ensure_disk_matches_for_repair(&mut state)?;
        let overrides = BTreeMap::new();
        let revision = next_revision(state.snapshot.revision)?;
        let effective = resolve_overrides(&self.registry, &overrides)?;
        let bytes = encode(revision, &overrides)?;
        commit_state(&mut state, &bytes, BackupMode::PreserveLastGood)?;
        let snapshot = Arc::new(SettingsSnapshot {
            revision,
            effective,
            overrides: Arc::new(overrides),
        });
        state.status = ControllerStatus::Ready;
        state.disk_bytes = Some(bytes);
        state.snapshot = snapshot.clone();
        Ok(snapshot)
    }

    /// Explicitly restores the last-good `.bak`, revalidates it and publishes
    /// it as a new current-schema revision. Future-schema and unclassifiable
    /// files remain read-only and cannot be overwritten through this API.
    pub fn restore_backup(
        &self,
        expected_revision: u64,
    ) -> Result<Arc<SettingsSnapshot>, ConfigError> {
        let mut state = self.lock()?;
        if let ControllerStatus::ReadOnlyFutureSchema { schema_version } = &state.status {
            return Err(ConfigError::ReadOnlyFutureSchema {
                schema_version: *schema_version,
            });
        }
        if matches!(
            &state.status,
            ControllerStatus::ReadOnlyUnclassifiableDocument(_)
        ) {
            return Err(ConfigError::ReadOnlyUnclassifiableDocument);
        }
        ensure_expected(expected_revision, state.snapshot.revision)?;
        ensure_disk_matches_for_repair(&mut state)?;
        let backup = state.store.read_backup()?.ok_or(ConfigError::NoBackup)?;
        let mut document = decode(&backup)?;
        if document.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::InvalidBackupVersion(document.schema_version));
        }
        if document.schema_version < CURRENT_SCHEMA_VERSION {
            document = migrate(document).map_err(map_migration_error)?;
        }
        validate_overrides(&self.registry, &document.overrides)?;
        let effective = resolve_overrides(&self.registry, &document.overrides)?;
        let revision = next_revision(state.snapshot.revision)?;
        let bytes = encode(revision, &document.overrides)?;
        // Leave the selected backup available if replacing the main file fails.
        commit_state(&mut state, &bytes, BackupMode::PreserveLastGood)?;
        let snapshot = Arc::new(SettingsSnapshot {
            revision,
            effective,
            overrides: Arc::new(document.overrides),
        });
        state.status = ControllerStatus::Ready;
        state.disk_bytes = Some(bytes);
        state.snapshot = snapshot.clone();
        Ok(snapshot)
    }

    fn lock(&self) -> Result<MutexGuard<'_, ControllerState>, ConfigError> {
        self.state.lock().map_err(|_| ConfigError::LockPoisoned)
    }
}

fn decode_and_validate(
    registry: &Registry,
    bytes: &[u8],
) -> Result<DecodedDocument, OpenDocumentError> {
    let document = decode(bytes).map_err(|error| OpenDocumentError {
        error: ConfigError::Document(error),
        can_repair: false,
        revision: None,
    })?;
    if document.schema_version > CURRENT_SCHEMA_VERSION {
        return Ok(document);
    }
    validate_overrides(registry, &document.overrides).map_err(|error| OpenDocumentError {
        revision: Some(document.revision),
        can_repair: !matches!(&error, ConfigError::UnknownSetting(_)),
        error,
    })?;
    resolve_overrides(registry, &document.overrides).map_err(|error| OpenDocumentError {
        error,
        can_repair: true,
        revision: Some(document.revision),
    })?;
    Ok(document)
}

fn build_snapshot(
    registry: &Registry,
    revision: u64,
    overrides: BTreeMap<SettingKey, SettingValue>,
) -> Result<Arc<SettingsSnapshot>, ConfigError> {
    validate_overrides(registry, &overrides)?;
    let effective = resolve_overrides(registry, &overrides)?;
    Ok(Arc::new(SettingsSnapshot {
        revision,
        effective,
        overrides: Arc::new(overrides),
    }))
}

fn resolve_overrides(
    registry: &Registry,
    overrides: &BTreeMap<SettingKey, SettingValue>,
) -> Result<EffectiveSettings, ConfigError> {
    let mut layers = SettingLayers::default();
    for (key, value) in overrides {
        let descriptor = registry
            .get(key)
            .ok_or_else(|| ConfigError::UnknownSetting(key.clone()))?;
        let source = match descriptor.scope {
            SettingScope::P | SettingScope::O => SettingSource::Assignment,
            SettingScope::A => SettingSource::Application,
            SettingScope::S => {
                return Err(ConfigError::SessionSettingCannotPersist(key.clone()));
            }
        };
        layers.set(source, key.clone(), value.clone());
    }
    Ok(resolve_settings(
        registry,
        &layers,
        &Constraints::default(),
    )?)
}

fn validate_overrides(
    registry: &Registry,
    overrides: &BTreeMap<SettingKey, SettingValue>,
) -> Result<(), ConfigError> {
    let mut validation_error = None;
    for (key, value) in overrides {
        let Some(descriptor) = registry.get(key) else {
            return Err(ConfigError::UnknownSetting(key.clone()));
        };
        if validation_error.is_some() {
            continue;
        }
        validation_error = if descriptor.scope == SettingScope::S {
            Some(ConfigError::SessionSettingCannotPersist(key.clone()))
        } else {
            descriptor
                .value_type
                .validates(value)
                .err()
                .map(|violation| ConfigError::InvalidPersistentValue {
                    key: key.clone(),
                    violation,
                })
        };
    }
    validation_error.map_or(Ok(()), Err)
}

fn ensure_writable(status: &ControllerStatus) -> Result<(), ConfigError> {
    match status {
        ControllerStatus::Ready => Ok(()),
        ControllerStatus::RepairRequired(_) => Err(ConfigError::RepairRequired),
        ControllerStatus::ReadOnlyFutureSchema { schema_version } => {
            Err(ConfigError::ReadOnlyFutureSchema {
                schema_version: *schema_version,
            })
        }
        ControllerStatus::ReadOnlyUnclassifiableDocument(_) => {
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        }
    }
}

fn ensure_expected(expected: u64, current: u64) -> Result<(), ConfigError> {
    if expected == current {
        Ok(())
    } else {
        Err(ConfigError::Conflict { expected, current })
    }
}

fn ensure_disk_matches(state: &mut ControllerState) -> Result<(), ConfigError> {
    let actual = match state.store.read_main() {
        Ok(actual) => actual,
        Err(StoreError::TooLarge { .. }) => {
            return Err(ConfigError::ExternalModification {
                current: state.snapshot.revision,
            });
        }
        Err(error) => return Err(ConfigError::Storage(error)),
    };
    if actual != state.disk_bytes {
        return Err(external_change(state, actual.as_deref()));
    }
    Ok(())
}

fn ensure_disk_matches_for_repair(state: &mut ControllerState) -> Result<(), ConfigError> {
    if matches!(&state.status, ControllerStatus::Ready) {
        return ensure_disk_matches(state);
    }
    if let Some(expected) = &state.disk_bytes {
        let actual = state.store.read_main()?;
        if actual.as_deref() != Some(expected.as_slice()) {
            return Err(external_change(state, actual.as_deref()));
        }
    }
    Ok(())
}

fn external_change(state: &ControllerState, actual: Option<&[u8]>) -> ConfigError {
    let observed = actual
        .and_then(|bytes| decode(bytes).ok())
        .map(|document| document.revision);
    match observed {
        Some(current) if current != state.snapshot.revision => ConfigError::Conflict {
            expected: state.snapshot.revision,
            current,
        },
        _ => ConfigError::ExternalModification {
            current: state.snapshot.revision,
        },
    }
}
fn commit_state(
    state: &mut ControllerState,
    bytes: &[u8],
    backup_mode: BackupMode,
) -> Result<(), ConfigError> {
    match state
        .store
        .commit(state.disk_bytes.as_deref(), bytes, backup_mode)
    {
        Ok(()) => Ok(()),
        Err(StoreError::Stale { current }) => Err(external_change(state, current.as_deref())),
        Err(StoreError::TooLarge { .. }) => Err(ConfigError::ExternalModification {
            current: state.snapshot.revision,
        }),
        Err(error) => Err(ConfigError::Storage(error)),
    }
}

fn next_revision(current: u64) -> Result<u64, ConfigError> {
    current
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or(ConfigError::RevisionOverflow)
}

fn map_migration_error(error: MigrationError) -> ConfigError {
    match error {
        MigrationError::UnsupportedVersion(version) => ConfigError::InvalidBackupVersion(version),
    }
}

#[cfg(test)]
impl SettingsController {
    fn open_for_test(
        registry: Arc<Registry>,
        store: Box<dyn ConfigStore>,
    ) -> Result<Self, ConfigError> {
        Self::open_with_store(registry, store)
    }
}
#[cfg(test)]
mod persistence_tests {
    use super::{SettingChange, SettingsController};
    use crate::{
        error::ConfigError,
        model::{
            InvalidationClass, SemanticValidator, SettingDescriptor, SettingKey, SettingPhase,
            SettingScope, SettingType, SettingValue,
        },
        registry::Registry,
        store::{FileStore, PersistStage, StoreError},
    };
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ascii-config-controller-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn registry() -> std::sync::Arc<Registry> {
        let key = SettingKey::new("application.enabled").unwrap();
        let descriptor = SettingDescriptor::new(
            key,
            SettingValue::Boolean(false),
            SettingType::Boolean,
            SettingScope::A,
            SettingPhase::V1,
            None,
            InvalidationClass::Controller,
            SemanticValidator::None,
        )
        .unwrap();
        std::sync::Arc::new(Registry::new([descriptor]).unwrap())
    }

    #[test]
    fn interrupted_controller_commit_keeps_disk_and_snapshot_on_old_revision() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original = b"schema_version = 2\nrevision = 0\n[settings]\n";
        fs::write(&path, original).unwrap();
        let mut store = FileStore::new(path.clone());
        store.inject_failure(PersistStage::MainReplace);
        let controller = SettingsController::open_for_test(registry(), Box::new(store)).unwrap();

        let result = controller.apply_batch(
            0,
            &[(
                SettingKey::new("application.enabled").unwrap(),
                SettingChange::Set(SettingValue::Boolean(true)),
            )],
        );
        assert!(matches!(
            result,
            Err(ConfigError::Storage(StoreError::Injected(
                PersistStage::MainReplace
            )))
        ));
        assert_eq!(controller.snapshot().unwrap().revision(), 0);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn failed_migration_keeps_pre_migration_bytes_usable() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original =
            b"schema_version = 1\nrevision = 3\n[settings]\n\"application.enabled\" = true\n";
        fs::write(&path, original).unwrap();
        let mut store = FileStore::new(path.clone());
        store.inject_failure(PersistStage::BackupReplace);

        let result = SettingsController::open_for_test(registry(), Box::new(store));
        assert!(matches!(
            result,
            Err(ConfigError::Storage(StoreError::Injected(
                PersistStage::BackupReplace
            )))
        ));
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn failed_backup_restore_keeps_selected_backup_usable() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let backup_path = directory.0.join("settings.toml.bak");
        let original = b"schema_version = 2\nrevision = 5\n[settings]\n\"application.enabled\" = true\n";
        let backup = b"schema_version = 2\nrevision = 4\n[settings]\n\"application.enabled\" = false\n";
        fs::write(&path, original).unwrap();
        fs::write(&backup_path, backup).unwrap();
        let mut store = FileStore::new(path.clone());
        store.inject_failure(PersistStage::MainReplace);
        let controller = SettingsController::open_for_test(registry(), Box::new(store)).unwrap();

        assert!(matches!(
            controller.restore_backup(5),
            Err(ConfigError::Storage(StoreError::Injected(
                PersistStage::MainReplace
            )))
        ));
        assert_eq!(controller.snapshot().unwrap().revision(), 5);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&backup_path).unwrap(), backup);
    }
}
