use crate::{
    document::DocumentError,
    model::{SettingKey, TypeViolation},
    registry::RegistryError,
    resolve::ResolveError,
    store::StoreError,
};

/// Categorized configuration-authority failure.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigError {
    Storage(StoreError),
    Document(DocumentError),
    Registry(RegistryError),
    Resolution(ResolveError),
    Conflict {
        expected: u64,
        current: u64,
    },
    ExternalModification {
        current: u64,
    },
    ReadOnlyFutureSchema {
        schema_version: i64,
    },
    RepairRequired,
    NotRepairable,
    NoBackup,
    InvalidBackupVersion(i64),
    UnknownSetting(SettingKey),
    InvalidPersistentValue {
        key: SettingKey,
        violation: TypeViolation,
    },
    SessionSettingCannotPersist(SettingKey),
    DuplicateBatchKey(SettingKey),
    RevisionOverflow,
    LockPoisoned,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "configuration error: {self:?}")
    }
}

impl std::error::Error for ConfigError {}

impl From<StoreError> for ConfigError {
    fn from(error: StoreError) -> Self {
        Self::Storage(error)
    }
}

impl From<DocumentError> for ConfigError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

impl From<RegistryError> for ConfigError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<ResolveError> for ConfigError {
    fn from(error: ResolveError) -> Self {
        Self::Resolution(error)
    }
}
