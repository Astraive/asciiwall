use crate::determinism::MAX_NODE_ID_BYTES;
use crate::params::{
    DEFAULT_CATCH_UP_STEPS, MAX_ACCELERATION_Q16, MAX_CATCH_UP_STEPS, MAX_GRID_CELLS,
    MAX_GRID_DIMENSION, MAX_PARTICLE_LIFETIME_TICKS, MAX_PARTICLES, MAX_RUNTIME_RESOURCE_BYTES,
    MAX_SIMULATION_WORK_UNITS, MAX_VELOCITY_Q16, TIME_SCALE_ONE_Q16,
};
use crate::snapshots::SignalKind;

/// Logical allocation and bounded-work estimate for one configured runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceEstimate {
    pub(crate) character_bytes: u64,
    pub(crate) generator_scratch_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) work_units: u64,
}

/// Logical storage model used by `SceneRuntime::resource_estimate`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceEstimatePolicy {
    pub character_bytes_per_cell: u64,
    pub particle_record_bytes: u64,
    pub excludes_allocator_overhead: bool,
}

impl ResourceEstimate {
    #[must_use]
    pub const fn character_bytes(self) -> u64 {
        self.character_bytes
    }

    #[must_use]
    pub const fn generator_scratch_bytes(self) -> u64 {
        self.generator_scratch_bytes
    }

    #[must_use]
    pub const fn total_bytes(self) -> u64 {
        self.total_bytes
    }

    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    U16,
    U32,
    FiniteFloat,
    Boolean,
    ColorRgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterDefault {
    U16(u16),
    U32(u32),
    Float(f32),
    Boolean(bool),
    ColorRgba(u32),
}

/// Generator-local parameter introspection; this is not a settings registry or package schema.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterDescriptor {
    pub name: &'static str,
    pub group: &'static str,
    pub kind: ParameterKind,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub default: ParameterDefault,
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionPolicy {
    FreezeWhenBackgroundMotionDisabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresetMotion {
    Normal,
    Static,
}

/// Values for a named local preset. Preset cost is a logical estimate, not PERF evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresetDescriptor {
    pub name: &'static str,
    pub motion: PresetMotion,
    pub seed: u32,
    pub time_scale_q16: u32,
    pub max_particles: u32,
    pub simulation_hz: u16,
    pub background_motion: bool,
    pub cost_is_estimate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratorLimits {
    pub max_columns: u32,
    pub max_rows: u32,
    pub max_cells: u64,
    pub max_node_id_bytes: usize,
    pub max_particles: u32,
    pub default_catch_up_steps: u8,
    pub max_catch_up_steps: u8,
    pub max_runtime_resource_bytes: u64,
    pub max_work_units_per_advance: u64,
    pub max_velocity_q16: i32,
    pub max_acceleration_q16: i32,
    pub max_lifetime_ticks: u32,
}

/// Versioned declaration for one reviewed first-party generator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneratorDescriptor {
    pub id: &'static str,
    pub version: u32,
    pub parameter_schema_version: u32,
    pub parameters: &'static [ParameterDescriptor],
    pub limits: GeneratorLimits,
    pub supported_signals: &'static [SignalKind],
    pub reduced_motion: MotionPolicy,
    pub deterministic_export: bool,
    pub presets: &'static [PresetDescriptor],
    pub resource_estimate: ResourceEstimatePolicy,
}

const PARAMETERS: &[ParameterDescriptor] = &[
    ParameterDescriptor {
        name: "seed",
        group: "Randomness",
        kind: ParameterKind::U32,
        minimum: Some(0.0),
        maximum: Some(4_294_967_295.0),
        default: ParameterDefault::U32(1),
        description: "Unsigned scene seed; zero and the full u32 range are valid.",
    },
    ParameterDescriptor {
        name: "time_scale",
        group: "Simulation",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(4.0),
        default: ParameterDefault::Float(1.0),
        description: "Fixed-step time multiplier; zero freezes scene evolution.",
    },
    ParameterDescriptor {
        name: "max_particles",
        group: "Particles",
        kind: ParameterKind::U32,
        minimum: Some(0.0),
        maximum: Some(MAX_PARTICLES as f64),
        default: ParameterDefault::U32(2_000),
        description: "Hard per-generator particle count; zero disables particles.",
    },
    ParameterDescriptor {
        name: "simulation_hz",
        group: "Simulation",
        kind: ParameterKind::U16,
        minimum: Some(15.0),
        maximum: Some(120.0),
        default: ParameterDefault::U16(60),
        description: "Fixed simulation frequency in steps per second.",
    },
    ParameterDescriptor {
        name: "background_motion",
        group: "Motion",
        kind: ParameterKind::Boolean,
        minimum: None,
        maximum: None,
        default: ParameterDefault::Boolean(true),
        description: "False selects the static path and performs no simulation updates.",
    },
];

const PRESETS: &[PresetDescriptor] = &[
    PresetDescriptor {
        name: "baseline",
        motion: PresetMotion::Normal,
        seed: 1,
        time_scale_q16: TIME_SCALE_ONE_Q16,
        max_particles: 2_000,
        simulation_hz: 60,
        background_motion: true,
        cost_is_estimate: true,
    },
    PresetDescriptor {
        name: "static",
        motion: PresetMotion::Static,
        seed: 1,
        time_scale_q16: 0,
        max_particles: 2_000,
        simulation_hz: 60,
        background_motion: false,
        cost_is_estimate: true,
    },
];

pub const GENERATOR_DESCRIPTOR: GeneratorDescriptor = GeneratorDescriptor {
    id: "builtin.particle-field",
    version: 1,
    parameter_schema_version: 1,
    parameters: PARAMETERS,
    resource_estimate: ResourceEstimatePolicy {
        character_bytes_per_cell: core::mem::size_of::<char>() as u64,
        particle_record_bytes: crate::generator::PARTICLE_RECORD_BYTES,
        excludes_allocator_overhead: true,
    },
    limits: GeneratorLimits {
        max_columns: MAX_GRID_DIMENSION,
        max_rows: MAX_GRID_DIMENSION,
        max_cells: MAX_GRID_CELLS,
        max_node_id_bytes: MAX_NODE_ID_BYTES,
        max_particles: MAX_PARTICLES,
        default_catch_up_steps: DEFAULT_CATCH_UP_STEPS,
        max_catch_up_steps: MAX_CATCH_UP_STEPS,
        max_runtime_resource_bytes: MAX_RUNTIME_RESOURCE_BYTES,
        max_work_units_per_advance: MAX_SIMULATION_WORK_UNITS,
        max_velocity_q16: MAX_VELOCITY_Q16,
        max_acceleration_q16: MAX_ACCELERATION_Q16,
        max_lifetime_ticks: MAX_PARTICLE_LIFETIME_TICKS,
    },
    supported_signals: &[SignalKind::PointerPosition],
    reduced_motion: MotionPolicy::FreezeWhenBackgroundMotionDisabled,
    deterministic_export: true,
    presets: PRESETS,
};
