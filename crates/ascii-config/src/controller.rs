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
        let mut next_read = store.read_main();
        let mut migration_retry_used = false;
        let (status, snapshot, disk_bytes) = loop {
            match next_read {
                Ok(None) => break (ControllerStatus::Ready, defaults.clone(), None),
                Ok(Some(bytes)) => match decode_and_validate(&registry, &bytes) {
                    Ok(document) if document.schema_version > CURRENT_SCHEMA_VERSION => {
                        let snapshot =
                            build_snapshot(&registry, document.revision, BTreeMap::new())?;
                        break (
                            ControllerStatus::ReadOnlyFutureSchema {
                                schema_version: document.schema_version,
                            },
                            snapshot,
                            Some(bytes),
                        );
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
                        match store.commit(
                            Some(&bytes),
                            &serialized,
                            BackupMode::ReplaceWithPrevious,
                        ) {
                            Ok(()) => {
                                break (
                                    ControllerStatus::Ready,
                                    Arc::new(SettingsSnapshot {
                                        revision,
                                        effective,
                                        overrides: Arc::new(migrated.overrides),
                                    }),
                                    Some(serialized),
                                );
                            }
                            Err(StoreError::Stale { .. }) if !migration_retry_used => {
                                migration_retry_used = true;
                                next_read = store.read_main_locked();
                                continue;
                            }
                            Err(error) => return Err(ConfigError::Storage(error)),
                        }
                    }
                    Ok(document) => {
                        validate_overrides(&registry, &document.overrides)?;
                        let effective = resolve_overrides(&registry, &document.overrides)?;
                        break (
                            ControllerStatus::Ready,
                            Arc::new(SettingsSnapshot {
                                revision: document.revision,
                                effective,
                                overrides: Arc::new(document.overrides),
                            }),
                            Some(bytes),
                        );
                    }
                    Err(failure) => {
                        let snapshot = match failure.revision {
                            Some(revision) => build_snapshot(&registry, revision, BTreeMap::new())?,
                            None => defaults.clone(),
                        };
                        let status = if failure.can_repair {
                            ControllerStatus::RepairRequired(failure.error)
                        } else {
                            ControllerStatus::ReadOnlyUnclassifiableDocument(failure.error)
                        };
                        break (status, snapshot, Some(bytes));
                    }
                },
                Err(StoreError::TooLarge { limit, .. }) => {
                    break (
                        ControllerStatus::ReadOnlyUnclassifiableDocument(ConfigError::Document(
                            DocumentError::TooLarge { limit },
                        )),
                        defaults.clone(),
                        None,
                    );
                }
                Err(error) => return Err(ConfigError::Storage(error)),
            }
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

    /// Replaces invalid data only after recognizing a supported-schema
    /// document. This is an explicit operation that preserves the last-good
    /// backup and archives the replaced bytes.
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
    let document = match decode(bytes) {
        Ok(document) => document,
        Err(error) => {
            let recovery = match &error {
                DocumentError::InvalidToml
                | DocumentError::InvalidField("revision")
                | DocumentError::UnsupportedStructure => recovery_metadata(registry, bytes),
                _ => None,
            };
            return Err(OpenDocumentError {
                error: ConfigError::Document(error),
                can_repair: recovery.is_some(),
                revision: recovery.and_then(|metadata| metadata.revision),
            });
        }
    };
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

#[derive(Clone, Copy)]
struct RecoveryMetadata {
    revision: Option<u64>,
}

fn recovery_metadata(registry: &Registry, bytes: &[u8]) -> Option<RecoveryMetadata> {
    let text = std::str::from_utf8(bytes).ok()?;
    if let Ok(toml::Value::Table(root)) = toml::from_str::<toml::Value>(text) {
        return recovery_metadata_from_parsed(registry, text, root);
    }
    if !settings_keys_are_known(text, registry) {
        return None;
    }
    recovery_metadata_from_prefix(text)
}

fn recovery_metadata_from_parsed(
    registry: &Registry,
    text: &str,
    mut root: toml::map::Map<String, toml::Value>,
) -> Option<RecoveryMetadata> {
    let schema_version = root
        .remove("schema_version")
        .and_then(|value| value.as_integer())?;
    if schema_version != CURRENT_SCHEMA_VERSION {
        return None;
    }
    let revision = root
        .remove("revision")
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok());
    if let Some(settings) = root.remove("settings") {
        let toml::Value::Table(settings) = settings else {
            return None;
        };
        let parsed_syntax = if settings.values().any(recovery_value_contains_table) {
            Some(text.parse::<toml_edit::DocumentMut>().ok()?)
        } else {
            None
        };
        if settings.iter().any(|(key, value)| {
            !known_setting_name(registry, key)
                || (recovery_value_contains_table(value)
                    && !parsed_syntax.as_ref().is_some_and(|document| {
                        structured_setting_assignment_matches(document, key)
                    }))
        }) {
            return None;
        }
    }
    if !root.is_empty() {
        return None;
    }
    Some(RecoveryMetadata { revision })
}

fn recovery_value_contains_table(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(_) => true,
        toml::Value::Array(values) => values.iter().any(recovery_value_contains_table),
        _ => false,
    }
}

// Read the TOML tree so strings/comments cannot masquerade as assignments;
// direct-child lookup also rejects ambiguous dotted-key paths.
fn structured_setting_assignment_matches(
    document: &toml_edit::DocumentMut,
    expected_key: &str,
) -> bool {
    let Some(settings) = document.get("settings") else {
        return false;
    };
    match settings {
        toml_edit::Item::Table(settings) => {
            let key_is_quoted = table_key_is_quoted(settings, expected_key);
            settings.get(expected_key).is_some_and(|item| match item {
                toml_edit::Item::Table(table) => {
                    key_is_quoted && !table.is_implicit() && !table.is_dotted()
                }
                toml_edit::Item::ArrayOfTables(_) => key_is_quoted,
                _ => item
                    .as_value()
                    .is_some_and(structured_toml_value_is_unambiguous),
            })
        }
        toml_edit::Item::Value(toml_edit::Value::InlineTable(settings)) => {
            !settings.is_dotted()
                && settings
                    .get(expected_key)
                    .is_some_and(structured_toml_value_is_unambiguous)
        }
        _ => false,
    }
}

fn table_key_is_quoted(table: &toml_edit::Table, expected_key: &str) -> bool {
    table
        .key(expected_key)
        .and_then(|key| key.as_repr())
        .and_then(|repr| repr.as_raw().as_str())
        .is_some_and(|repr| repr.starts_with('"') || repr.starts_with('\''))
}

fn structured_toml_value_is_unambiguous(value: &toml_edit::Value) -> bool {
    match value {
        toml_edit::Value::Array(_) => true,
        toml_edit::Value::InlineTable(table) => !table.is_dotted(),
        _ => false,
    }
}

fn recovery_metadata_from_prefix(text: &str) -> Option<RecoveryMetadata> {
    let mut schema_version = None;
    let mut saw_schema_version = false;
    let mut revision = None;
    let mut saw_revision = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[settings]" || is_settings_assignment(line) {
            break;
        }
        if line.starts_with('[') {
            return None;
        }
        let parsed = toml::from_str::<toml::Value>(line);
        let (key, value) = match parsed {
            Ok(toml::Value::Table(mut entries)) if entries.len() == 1 => {
                let key = entries.keys().next()?.clone();
                let value = entries.remove(&key)?;
                (key, value)
            }
            _ if !saw_revision && is_revision_assignment(line) => {
                saw_revision = true;
                continue;
            }
            _ => return None,
        };
        match key.as_str() {
            "schema_version" if !saw_schema_version => {
                saw_schema_version = true;
                schema_version = Some(value.as_integer()?);
                if schema_version != Some(CURRENT_SCHEMA_VERSION) {
                    return None;
                }
            }
            "revision" if !saw_revision => {
                saw_revision = true;
                revision = value
                    .as_integer()
                    .and_then(|value| u64::try_from(value).ok());
            }
            _ => return None,
        }
    }
    (schema_version == Some(CURRENT_SCHEMA_VERSION)).then_some(RecoveryMetadata { revision })
}

fn is_revision_assignment(line: &str) -> bool {
    let mut saw_key = false;
    let mut is_revision = false;
    assignment_keys_are_unambiguous(line, |raw_key| {
        if saw_key {
            return false;
        }
        saw_key = true;
        is_revision = raw_key.trim() == "revision";
        true
    }) && is_revision
}

fn assignment_keys_are_unambiguous(
    line: &str,
    mut key_is_allowed: impl FnMut(&str) -> bool,
) -> bool {
    let bytes = line.as_bytes();
    let mut segment_start = 0;
    let mut assignment = false;
    let mut quote = None;
    let mut escaped = false;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(quote_byte) = quote {
            if quote_byte == b'"' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote_byte {
                    quote = None;
                }
            } else if byte == quote_byte {
                quote = None;
            }
            continue;
        }

        match byte {
            b'"' | b'\'' => {
                if bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte) {
                    return false;
                }
                quote = Some(byte);
            }
            b'#' => return assignment,
            b'=' if !assignment => {
                if !key_is_allowed(&line[segment_start..index]) {
                    return false;
                }
                assignment = true;
            }
            b'=' => return false,
            b',' if assignment => {
                segment_start = index + 1;
                assignment = false;
            }
            b',' => return false,
            b'[' | b'{' | b']' | b'}' => return false,
            _ => {}
        }
    }

    quote.is_none() && assignment
}

fn assignment_keys_are_known(line: &str, registry: &Registry) -> bool {
    assignment_keys_are_unambiguous(line, |key| known_setting_syntax(registry, key))
}

fn is_settings_assignment(line: &str) -> bool {
    matches!(assignment_key(line), Some(key) if key.trim() == "settings")
}

fn assignment_key(line: &str) -> Option<&str> {
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in line.bytes().enumerate() {
        if let Some(quote_byte) = quote {
            if quote_byte == b'"' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote_byte {
                    quote = None;
                }
            } else if byte == quote_byte {
                quote = None;
            }
        } else {
            match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'#' => return None,
                b'=' => return Some(&line[..index]),
                _ => {}
            }
        }
    }
    None
}

fn settings_keys_are_known(text: &str, registry: &Registry) -> bool {
    let mut in_settings_table = false;
    let mut settings_seen = false;
    let mut inline_settings_seen = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if inline_settings_seen {
            return false;
        }
        if line.starts_with('[') {
            if line == "[settings]" && !settings_seen {
                settings_seen = true;
                in_settings_table = true;
                continue;
            }
            return false;
        }
        if !in_settings_table && is_settings_assignment(line) {
            if settings_seen || !inline_settings_keys_are_known(line, registry) {
                return false;
            }
            settings_seen = true;
            inline_settings_seen = true;
            continue;
        }
        if in_settings_table && !assignment_keys_are_known(line, registry) {
            return false;
        }
    }
    true
}

fn inline_settings_keys_are_known(line: &str, registry: &Registry) -> bool {
    let Some((_, value)) = line.split_once('=') else {
        return false;
    };
    let value = value.trim_start();
    let Some(open_brace) = value.find('{') else {
        return false;
    };
    let bytes = value.as_bytes();
    let mut segment_start = open_brace + 1;
    let mut assignment = None;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(segment_start) {
        if let Some(quote_byte) = quote {
            if quote_byte == b'"' {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote_byte {
                    quote = None;
                }
            } else if byte == quote_byte {
                quote = None;
            }
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'[' | b'{' => return false,
            b'=' if assignment.is_none() => {
                if !known_setting_syntax(registry, &value[segment_start..index]) {
                    return false;
                }
                assignment = Some(index);
            }
            b',' => {
                if assignment.is_none() {
                    return false;
                }
                segment_start = index + 1;
                assignment = None;
            }
            b'}' => {
                let segment = value[segment_start..index].trim();
                if !segment.is_empty() && assignment.is_none() {
                    return false;
                }
                let trailing = value[index + 1..].trim();
                return trailing.is_empty() || trailing.starts_with('#');
            }
            _ => {}
        }
    }
    false
}

fn known_setting_syntax(registry: &Registry, raw_key: &str) -> bool {
    setting_key_from_syntax(registry, raw_key).is_some()
}

fn setting_key_from_syntax<'a>(registry: &Registry, raw_key: &'a str) -> Option<&'a str> {
    let raw_key = raw_key.trim();
    let (key, quoted) = match raw_key
        .strip_prefix('"')
        .and_then(|key| key.strip_suffix('"'))
    {
        Some(key) => (key, true),
        None => match raw_key
            .strip_prefix('\'')
            .and_then(|key| key.strip_suffix('\''))
        {
            Some(key) => (key, true),
            None => (raw_key, false),
        },
    };
    if !quoted && key.contains('.') {
        return None;
    }
    known_setting_name(registry, key).then_some(key)
}

fn known_setting_name(registry: &Registry, raw_key: &str) -> bool {
    let Ok(key) = SettingKey::new(raw_key) else {
        return false;
    };
    registry.get(&key).is_some()
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
    use super::{ControllerStatus, SettingChange, SettingsController};
    use crate::{
        error::ConfigError,
        model::{
            InvalidationClass, SemanticValidator, SettingDescriptor, SettingKey, SettingPhase,
            SettingScope, SettingType, SettingValue,
        },
        registry::Registry,
        store::{BackupMode, ConfigStore, FileStore, PersistStage, StoreError},
    };
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
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

    fn registry_with_parent_and_note() -> Arc<Registry> {
        let boolean_descriptor = |name: &str| {
            SettingDescriptor::new(
                SettingKey::new(name).unwrap(),
                SettingValue::Boolean(false),
                SettingType::Boolean,
                SettingScope::A,
                SettingPhase::V1,
                None,
                InvalidationClass::Controller,
                SemanticValidator::None,
            )
            .unwrap()
        };
        let note = SettingDescriptor::new(
            SettingKey::new("note").unwrap(),
            SettingValue::text(""),
            SettingType::String { max_chars: 128 },
            SettingScope::A,
            SettingPhase::V1,
            None,
            InvalidationClass::Controller,
            SemanticValidator::None,
        )
        .unwrap();
        Arc::new(
            Registry::new([
                boolean_descriptor("application"),
                boolean_descriptor("application.enabled"),
                note,
            ])
            .unwrap(),
        )
    }
    struct ConcurrentMigrationStore {
        primary: FileStore,
        competitor: FileStore,
        concurrent_document: Vec<u8>,
        interleaved: bool,
        locked_rechecks: Arc<AtomicU64>,
    }

    impl ConfigStore for ConcurrentMigrationStore {
        fn read_main(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
            self.primary.read_main()
        }

        fn read_main_locked(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
            self.locked_rechecks.fetch_add(1, Ordering::SeqCst);
            self.primary.read_main_locked()
        }

        fn read_backup(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
            self.primary.read_backup()
        }

        fn commit(
            &mut self,
            expected_current: Option<&[u8]>,
            contents: &[u8],
            backup_mode: BackupMode,
        ) -> Result<(), StoreError> {
            if !self.interleaved {
                self.interleaved = true;
                self.competitor.commit(
                    expected_current,
                    &self.concurrent_document,
                    BackupMode::ReplaceWithPrevious,
                )?;
            }
            self.primary.commit(expected_current, contents, backup_mode)
        }
    }

    #[test]
    fn stale_migration_cas_reopens_a_competing_current_schema_document() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let legacy =
            b"schema_version = 1\nrevision = 3\n[settings]\n\"application.enabled\" = true\n";
        let concurrent =
            b"schema_version = 2\nrevision = 4\n[settings]\n\"application.enabled\" = true\n";
        fs::write(&path, legacy).unwrap();
        let locked_rechecks = Arc::new(AtomicU64::new(0));
        let store = ConcurrentMigrationStore {
            primary: FileStore::new(path.clone()),
            competitor: FileStore::new(path.clone()),
            concurrent_document: concurrent.to_vec(),
            interleaved: false,
            locked_rechecks: locked_rechecks.clone(),
        };

        let controller = SettingsController::open_for_test(registry(), Box::new(store)).unwrap();

        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::Ready
        ));
        let snapshot = controller.snapshot().unwrap();
        assert_eq!(snapshot.revision(), 4);
        assert_eq!(
            snapshot
                .overrides()
                .get(&SettingKey::new("application.enabled").unwrap()),
            Some(&SettingValue::Boolean(true))
        );
        assert_eq!(fs::read(path).unwrap(), concurrent);
        assert_eq!(locked_rechecks.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn recognized_structured_setting_requires_explicit_repair() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original =
            b"schema_version = 2\nrevision = 5\n[settings]\n\"application.enabled\" = [240]\n";
        fs::write(&path, original).unwrap();

        let controller =
            SettingsController::open_for_test(registry(), Box::new(FileStore::new(&path))).unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::RepairRequired(_)
        ));
        assert_eq!(controller.snapshot().unwrap().revision(), 5);
        assert_eq!(fs::read(&path).unwrap(), original);

        let repaired = controller.repair(5).unwrap();
        assert_eq!(repaired.revision(), 6);
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::Ready
        ));
        assert!(repaired.overrides().is_empty());
        let archived = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("repaired bytes archived");
        assert_eq!(fs::read(archived.path()).unwrap(), original);
    }

    #[test]
    fn recognized_inline_table_setting_requires_explicit_repair() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original =
            b"schema_version = 2\nrevision = 5\n[settings]\n\"application.enabled\" = { extra = 1 }\n";
        fs::write(&path, original).unwrap();

        let controller =
            SettingsController::open_for_test(registry(), Box::new(FileStore::new(&path))).unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::RepairRequired(_)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);

        let repaired = controller.repair(5).unwrap();
        assert_eq!(repaired.revision(), 6);
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::Ready
        ));
        assert!(repaired.overrides().is_empty());
        let archived = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("repaired bytes archived");
        assert_eq!(fs::read(archived.path()).unwrap(), original);
    }

    #[test]
    fn recognized_canonical_inline_table_setting_requires_explicit_repair() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original = b"schema_version = 2\nrevision = 5\nsettings = { \"application.enabled\" = { extra = 1 } }\n";
        fs::write(&path, original).unwrap();

        let controller =
            SettingsController::open_for_test(registry(), Box::new(FileStore::new(&path))).unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::RepairRequired(_)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);

        let repaired = controller.repair(5).unwrap();
        assert_eq!(repaired.revision(), 6);
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::Ready
        ));
        assert!(repaired.overrides().is_empty());
        let archived = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("repaired bytes archived");
        assert_eq!(fs::read(archived.path()).unwrap(), original);
    }

    #[test]
    fn recognized_quoted_table_and_array_headers_require_explicit_repair() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original = b"schema_version = 2\nrevision = 5\n[settings.\"application.enabled\"]\nextra = 1\n[[settings.\"note\"]]\nextra = 2\n";
        fs::write(&path, original).unwrap();

        let controller = SettingsController::open_for_test(
            registry_with_parent_and_note(),
            Box::new(FileStore::new(&path)),
        )
        .unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::RepairRequired(_)
        ));
        assert_eq!(fs::read(&path).unwrap(), original);

        let repaired = controller.repair(5).unwrap();
        assert_eq!(repaired.revision(), 6);
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::Ready
        ));
        let archived = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.toml.corrupt-")
            })
            .expect("repaired bytes archived");
        assert_eq!(fs::read(archived.path()).unwrap(), original);
    }

    #[test]
    fn unquoted_dotted_inline_prefix_with_registered_child_remains_read_only() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original = b"schema_version = 2\nrevision = 5\nsettings = { application.enabled = { extra = 1 } }\n";
        fs::write(&path, original).unwrap();

        let controller = SettingsController::open_for_test(
            registry_with_parent_and_note(),
            Box::new(FileStore::new(&path)),
        )
        .unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::ReadOnlyUnclassifiableDocument(_)
        ));
        assert!(matches!(
            controller.repair(5),
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        ));
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn unknown_and_unquoted_dotted_structures_remain_read_only() {
        let documents: [&[u8]; 6] = [
            b"schema_version = 2\nrevision = 5\n[settings]\n\"unknown.setting\" = [240]\n",
            b"schema_version = 2\nrevision = 5\n[settings]\napplication.enabled = [240]\n",
            b"schema_version = 2\nrevision = 5\n[settings]\n\"unknown.setting\" = { extra = 1 }\n",
            b"schema_version = 2\nrevision = 5\n[settings]\napplication.enabled = { extra = 1 }\n",
            b"schema_version = 2\nrevision = 5\nsettings = { \"unknown.setting\" = { extra = 1 } }\n",
            b"schema_version = 2\nrevision = 5\nsettings = { application.enabled = { extra = 1 } }\n",
        ];
        for (index, original) in documents.into_iter().enumerate() {
            let directory = TestDirectory::new();
            let path = directory.0.join(format!("settings-{index}.toml"));
            fs::write(&path, original).unwrap();

            let controller =
                SettingsController::open_for_test(registry(), Box::new(FileStore::new(&path)))
                    .unwrap();
            assert!(matches!(
                controller.status().unwrap(),
                ControllerStatus::ReadOnlyUnclassifiableDocument(_)
            ));
            assert!(matches!(
                controller.repair(0),
                Err(ConfigError::ReadOnlyUnclassifiableDocument)
            ));
            assert_eq!(fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn multiline_string_assignments_cannot_classify_dotted_tables_as_repairable() {
        let directory = TestDirectory::new();
        let path = directory.0.join("settings.toml");
        let original = b"schema_version = 2\nrevision = 5\n[settings]\nnote = \"\"\"\napplication = { nested = 1 }\n\"\"\"\napplication.enabled = { extra = 1 }\n";
        fs::write(&path, original).unwrap();

        let controller = SettingsController::open_for_test(
            registry_with_parent_and_note(),
            Box::new(FileStore::new(&path)),
        )
        .unwrap();
        assert!(matches!(
            controller.status().unwrap(),
            ControllerStatus::ReadOnlyUnclassifiableDocument(_)
        ));
        assert!(matches!(
            controller.repair(5),
            Err(ConfigError::ReadOnlyUnclassifiableDocument)
        ));
        assert_eq!(fs::read(path).unwrap(), original);
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
        let original =
            b"schema_version = 2\nrevision = 5\n[settings]\n\"application.enabled\" = true\n";
        let backup =
            b"schema_version = 2\nrevision = 4\n[settings]\n\"application.enabled\" = false\n";
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
