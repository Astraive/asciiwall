//! Shared draft v1 package contracts. Structural validation is deliberately
//! separate from archive, graph, compatibility, and capability semantics.
mod manifest;
mod scene_registry;
mod semantic;
mod settings;
mod unique_json;
mod validation;

pub use manifest::*;
pub use scene_registry::{
    SCENE_NODE_DESCRIPTORS, SCENE_REGISTRY_VERSION, SceneArity, SceneNodeDescriptor, ScenePortType,
    descriptor_for_node, parameter_schema,
};
pub use semantic::{
    MAX_DECODED_IMAGE_BYTES, MAX_DECODED_IMAGE_PIXELS, MAX_SCENE_EDGES, MAX_SCENE_NODES,
    SceneCapabilityPlan, SceneFeedDecision, SceneFeedReason, SceneFeedState, SceneFeedStatus,
    SceneFeeds, ScenePolicy, SceneResourceEstimate, SceneResourceInputs, SceneResourceLimits,
    SceneSemanticError, SceneValidationPlan, SourceResource, estimate_scene_resources,
    validate_graph, validate_scene_capabilities, validate_scene_semantics,
    validate_scene_source_refs,
};
pub use settings::{RegistryComparisonError, RegistryDrift, compare_portable_settings};
pub use validation::{
    Error, ErrorKind, MAX_JSON_DEPTH, MAX_MANIFEST_BYTES, MAX_SCENE_BYTES, validate_manifest,
    validate_scene,
};
