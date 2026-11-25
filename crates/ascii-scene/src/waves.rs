use ascii_convert::GlyphTable;
use ascii_core::{Error, MalformedReason};

use crate::descriptor::{
    GeneratorDescriptor, GeneratorLimits, MotionPolicy, ParameterDefault, ParameterDescriptor,
    ParameterKind, PresetMotion, ResourceEstimatePolicy,
};
use crate::determinism::MAX_NODE_ID_BYTES;
use crate::generator::SceneGenerator;
use crate::params::{
    MAX_CATCH_UP_STEPS, MAX_GRID_CELLS, MAX_GRID_DIMENSION, MAX_PARTICLES,
    MAX_RUNTIME_RESOURCE_BYTES, MAX_SIMULATION_WORK_UNITS, SceneDimensions, SceneParameters,
};
use crate::resource::GeneratorPreparationPermit;
use crate::simulation::FixedStepContext;
use crate::snapshots::ApprovedSnapshots;

const Q16: u64 = 1 << 16;
const MAX_PARAMETER_Q16: u32 = 20 << 16;
const WAVE_FIXED_BYTES: u64 = core::mem::size_of::<u16>() as u64;
const WAVE_PARAMETER_SCHEMA_VERSION: u32 = 1;
const WAVE_PARAMETERS: &[ParameterDescriptor] = &[
    ParameterDescriptor {
        name: "amplitude",
        group: "Field",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(10.0),
        default: ParameterDefault::Float(0.5),
        description: "Wave displacement in normalized glyph-scale units.",
    },
    ParameterDescriptor {
        name: "frequency",
        group: "Field",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(20.0),
        default: ParameterDefault::Float(2.0),
        description: "Spatial cycles across the field; zero yields a spatially uniform phase.",
    },
    ParameterDescriptor {
        name: "speed",
        group: "Motion",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(10.0),
        default: ParameterDefault::Float(0.5),
        description: "Temporal phase cycles per second; zero freezes the field.",
    },
    ParameterDescriptor {
        name: "color",
        group: "Appearance",
        kind: ParameterKind::ColorRgba,
        minimum: None,
        maximum: None,
        default: ParameterDefault::ColorRgba(0x66cc_ffff),
        description: "Portable #RRGGBBAA metadata; character output does not render color.",
    },
];

pub const WAVE_FIELD_PRESETS: &[WaveFieldPresetDescriptor] = &[
    WaveFieldPresetDescriptor {
        name: "calm",
        config: WaveFieldConfig::calm(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    WaveFieldPresetDescriptor {
        name: "contour",
        config: WaveFieldConfig::contour(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    WaveFieldPresetDescriptor {
        name: "still",
        config: WaveFieldConfig::still(),
        motion: PresetMotion::Static,
        cost_is_estimate: true,
    },
];

/// Generator-local metadata only; it is not a package or application schema.
pub const WAVE_FIELD_DESCRIPTOR: GeneratorDescriptor = GeneratorDescriptor {
    id: "builtin.wave-field",
    version: 1,
    parameter_schema_version: WAVE_PARAMETER_SCHEMA_VERSION,
    parameters: WAVE_PARAMETERS,
    limits: GeneratorLimits {
        max_columns: MAX_GRID_DIMENSION,
        max_rows: MAX_GRID_DIMENSION,
        max_cells: MAX_GRID_CELLS,
        max_node_id_bytes: MAX_NODE_ID_BYTES,
        max_particles: MAX_PARTICLES,
        default_catch_up_steps: 4,
        max_catch_up_steps: MAX_CATCH_UP_STEPS,
        max_runtime_resource_bytes: MAX_RUNTIME_RESOURCE_BYTES,
        max_work_units_per_advance: MAX_SIMULATION_WORK_UNITS,
        max_velocity_q16: 0,
        max_acceleration_q16: 0,
        max_lifetime_ticks: 0,
    },
    supported_signals: &[],
    reduced_motion: MotionPolicy::FreezeWhenBackgroundMotionDisabled,
    deterministic_export: false,
    presets: &[],
    resource_estimate: ResourceEstimatePolicy {
        character_bytes_per_cell: core::mem::size_of::<char>() as u64,
        particle_record_bytes: 0,
        excludes_allocator_overhead: true,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveFieldConfig {
    amplitude_q16: u32,
    frequency_q16: u32,
    speed_q16: u32,
    color_rgba: u32,
}

impl WaveFieldConfig {
    pub fn new(amplitude: f32, frequency: f32, speed: f32, color_rgba: u32) -> Result<Self, Error> {
        if !amplitude.is_finite()
            || !(0.0..=10.0).contains(&amplitude)
            || !frequency.is_finite()
            || !(0.0..=20.0).contains(&frequency)
            || !speed.is_finite()
            || !(0.0..=10.0).contains(&speed)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            amplitude_q16: (f64::from(amplitude) * Q16 as f64).round() as u32,
            frequency_q16: (f64::from(frequency) * Q16 as f64).round() as u32,
            speed_q16: (f64::from(speed) * Q16 as f64).round() as u32,
            color_rgba,
        })
    }

    pub fn from_hex_rgba(
        amplitude: f32,
        frequency: f32,
        speed: f32,
        color: &str,
    ) -> Result<Self, Error> {
        let digits = color
            .strip_prefix('#')
            .filter(|digits| digits.len() == 8)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        let color_rgba = u32::from_str_radix(digits, 16)
            .map_err(|_| Error::Malformed(MalformedReason::InvalidConfiguration))?;
        Self::new(amplitude, frequency, speed, color_rgba)
    }

    #[must_use]
    pub const fn calm() -> Self {
        Self {
            amplitude_q16: 32_768,
            frequency_q16: 2 * 65_536,
            speed_q16: 32_768,
            color_rgba: 0x66cc_ffff,
        }
    }

    #[must_use]
    pub const fn contour() -> Self {
        Self {
            amplitude_q16: 2 * 65_536,
            frequency_q16: 5 * 65_536,
            speed_q16: 65_536,
            color_rgba: 0xffff_ffff,
        }
    }

    #[must_use]
    pub const fn still() -> Self {
        Self {
            speed_q16: 0,
            ..Self::calm()
        }
    }

    #[must_use]
    pub const fn amplitude_q16(self) -> u32 {
        self.amplitude_q16
    }
    #[must_use]
    pub const fn frequency_q16(self) -> u32 {
        self.frequency_q16
    }
    #[must_use]
    pub const fn speed_q16(self) -> u32 {
        self.speed_q16
    }
    #[must_use]
    pub const fn color_rgba(self) -> u32 {
        self.color_rgba
    }
    #[must_use]
    pub fn amplitude(self) -> f32 {
        self.amplitude_q16 as f32 / Q16 as f32
    }
    #[must_use]
    pub fn frequency(self) -> f32 {
        self.frequency_q16 as f32 / Q16 as f32
    }
    #[must_use]
    pub fn speed(self) -> f32 {
        self.speed_q16 as f32 / Q16 as f32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaveFieldPresetDescriptor {
    pub name: &'static str,
    pub config: WaveFieldConfig,
    pub motion: PresetMotion,
    pub cost_is_estimate: bool,
}

pub const WAVE_FIELD_NODE_ID: &str = "builtin.wave-field.default";

/// Deterministic fixed-point scalar wave field borrowing the immutable glyph table.
pub struct WaveFieldGenerator<'a> {
    glyph_table: &'a GlyphTable,
    config: WaveFieldConfig,
    node_id: &'a str,
    phase_q16: u16,
}

impl<'a> WaveFieldGenerator<'a> {
    #[must_use]
    pub const fn new(glyph_table: &'a GlyphTable, config: WaveFieldConfig) -> Self {
        Self {
            glyph_table,
            config,
            node_id: WAVE_FIELD_NODE_ID,
            phase_q16: 0,
        }
    }

    #[must_use]
    pub const fn with_node_id(mut self, node_id: &'a str) -> Self {
        self.node_id = node_id;
        self
    }
}

impl crate::generator::sealed::Sealed for WaveFieldGenerator<'_> {}

impl SceneGenerator for WaveFieldGenerator<'_> {
    fn descriptor(&self) -> &'static GeneratorDescriptor {
        &WAVE_FIELD_DESCRIPTOR
    }
    fn node_id(&self) -> &str {
        self.node_id
    }

    fn scratch_bytes(&self, _parameters: &SceneParameters) -> Result<u64, Error> {
        Ok(WAVE_FIXED_BYTES)
    }

    fn scratch_bytes_for_dimensions(
        &self,
        _parameters: &SceneParameters,
        _dimensions: SceneDimensions,
    ) -> Result<u64, Error> {
        Ok(WAVE_FIXED_BYTES)
    }

    fn prepare(
        &mut self,
        _permit: &GeneratorPreparationPermit<'_>,
        _parameters: &SceneParameters,
        _dimensions: SceneDimensions,
    ) -> Result<(), Error> {
        if self.node_id.is_empty()
            || self.node_id.len() > MAX_NODE_ID_BYTES
            || self.config.amplitude_q16 > 10 * 65_536
            || self.config.frequency_q16 > MAX_PARAMETER_Q16
            || self.config.speed_q16 > 10 * 65_536
            || self.glyph_table.glyphs().is_empty()
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        self.phase_q16 = 0;
        Ok(())
    }

    fn step(
        &mut self,
        context: FixedStepContext,
        _dimensions: SceneDimensions,
        parameters: &SceneParameters,
        _snapshots: &ApprovedSnapshots<'_>,
    ) {
        if self.config.speed_q16 == 0
            || parameters.time_scale_q16() == 0
            || !parameters.background_motion()
        {
            return;
        }
        let phase = u128::from(context.step_index()) * u128::from(self.config.speed_q16)
            / u128::from(context.simulation_hz());
        self.phase_q16 = phase as u16;
    }

    fn is_static(&self) -> bool {
        self.config.speed_q16 == 0
    }

    fn render(&self, dimensions: SceneDimensions, output: &mut [char]) {
        if output.len() != dimensions.cell_count() {
            return;
        }
        let glyphs = self.glyph_table.glyphs();
        if glyphs.is_empty() {
            output.fill(' ');
            return;
        }
        let columns = u64::from(dimensions.columns());
        let rows = u64::from(dimensions.rows());
        let frequency = u64::from(self.config.frequency_q16);
        let phase_time = u32::from(self.phase_q16);
        let amplitude = u64::from(self.config.amplitude_q16);
        let max_index = (glyphs.len() - 1) as i64;
        let denom = 20_u64 * Q16;
        for row in 0..dimensions.rows() {
            for column in 0..dimensions.columns() {
                let horizontal = u64::from(column) * frequency / columns;
                let vertical = u64::from(row) * frequency / rows / 2;
                let phase = (horizontal + vertical + u64::from(phase_time)) as u16;
                let wave = i64::from(sine_q15(phase));
                let displacement = wave * amplitude as i64 * max_index / (32_767 * denom as i64);
                let center = max_index / 2;
                let cell_index = row as usize * columns as usize + column as usize;
                output[cell_index] =
                    glyphs[((center + displacement).clamp(0, max_index)) as usize].character();
            }
        }
    }
}

/// Integer Bhaskara sine approximation, returning signed Q1.15 in [-32767, 32767].
fn sine_q15(phase: u16) -> i32 {
    let phase = u32::from(phase);
    let negative = phase >= 32_768;
    let half = if negative { phase - 32_768 } else { phase };
    let x = if negative { 32_768 - half } else { half };
    let x_q16 = u64::from(x) * 2;
    let product = x_q16 * (65_536 - x_q16) / 65_536;
    let numerator = 16 * product * 32_767;
    let denominator = 5 * 65_536 - 4 * product;
    let value = (numerator / denominator) as i32;
    if negative { -value } else { value }
}
