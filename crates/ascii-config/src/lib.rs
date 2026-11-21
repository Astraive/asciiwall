//! Bounded, revisioned settings authority for AsciiWall.
//!
//! The crate owns descriptor metadata, layer authority, validation, schema
//! migration, and the only supported persisted writer. Domain descriptors are
//! supplied by their assigned CFG tickets; this foundation intentionally does
//! not invent settings or defaults. Applications should construct their
//! platform-specific `settings.toml` path and prepare resources only from a
//! validated immutable [`SettingsSnapshot`].
#![forbid(unsafe_code)]

mod controller;
mod document;
mod error;
mod migrate;
mod model;
mod registry;
mod resolve;
mod startup;
mod store;

pub use controller::{ControllerStatus, SettingChange, SettingsController, SettingsSnapshot};
pub use document::{
    CURRENT_SCHEMA_VERSION, DocumentError, MAX_CONFIG_BYTES, MAX_NESTING_DEPTH, MAX_SETTING_ENTRIES,
};
pub use error::ConfigError;
pub use model::{
    DescriptorError, InvalidationClass, KeyError, SemanticValidator, SettingDescriptor, SettingKey,
    SettingPhase, SettingScope, SettingType, SettingValue, TypeViolation, ValueError, ValueKind,
};
pub use registry::{Registry, RegistryError};
pub use resolve::{
    ConstraintReason, Constraints, EffectiveConstraint, EffectiveSettings, ResolveError,
    ResolvedValue, SettingLayers, SettingSource, resolve_settings,
};
pub use startup::{RestoreFailureState, RestoreGateError, RestorePolicy, RestorePolicyError};
pub use store::{PersistStage, StoreError, StoreOperation};
