//! Shared draft v1 package contracts. Structural validation is deliberately
//! separate from archive, graph, compatibility, and capability semantics.
mod manifest;
mod settings;

pub use manifest::*;
pub use settings::{RegistryComparisonError, RegistryDrift, compare_portable_settings};
pub use validation::{
    Error, ErrorKind, MAX_JSON_DEPTH, MAX_MANIFEST_BYTES, MAX_SCENE_BYTES, validate_manifest,
    validate_scene,
};
