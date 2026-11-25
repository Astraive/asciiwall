//! First-party deterministic scene generation and bounded fixed-step simulation.
//!
//! The crate owns no files, native handles, network clients, package evaluators, or operating
//! system queries. Host adapters provide already-normalized snapshots after capability filtering.
#![forbid(unsafe_code)]

mod descriptor;
mod determinism;
mod export;
mod generator;
mod params;
mod resource;
mod simulation;
mod snapshots;

pub use descriptor::{
    GENERATOR_DESCRIPTOR, GeneratorDescriptor, GeneratorLimits, MotionPolicy, ParameterDefault,
    ParameterDescriptor, ParameterKind, PresetDescriptor, PresetMotion, ResourceEstimate,
    ResourceEstimatePolicy,
};
pub use determinism::{
    MAX_NODE_ID_BYTES, PRNG_ALGORITHM, RandomStream, SEED_DERIVATION, derive_effect_seed,
    derive_node_seed,
};
pub use export::{
    ExportContext, ExportSignalTrack, ExportedFrame, MAX_RECORDED_EVENTS, RecordedEvent,
    RecordedEventKind,
};
pub use generator::{ParticleFieldGenerator, STARFIELD_NODE_ID, SceneGenerator, SceneRuntime};
pub use params::{
    MAX_CATCH_UP_STEPS, MAX_GRID_CELLS, MAX_GRID_DIMENSION, MAX_PARTICLES,
    MAX_RUNTIME_RESOURCE_BYTES, MAX_SIMULATION_WORK_UNITS, SceneDimensions, SceneParameters,
};
pub use resource::{GeneratorPreparationPermit, SceneResourceBudget};
pub use simulation::{AdvanceReport, FixedStepContext, SimulationLimits};
pub use snapshots::{
    ApprovedSnapshots, BatterySnapshot, NormalizedButtonEvent, NormalizedPoint, NormalizedRect,
    SignalKind, SnapshotCandidates, SnapshotCapabilities,
};

pub use ascii_core::Error;
