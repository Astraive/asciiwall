use ascii_convert::GlyphTable;
use ascii_core::{Error, MalformedReason};

use crate::descriptor::{
    GeneratorDescriptor, GeneratorLimits, MotionPolicy, ParameterDefault, ParameterDescriptor,
    ParameterKind, PresetMotion, ResourceEstimatePolicy,
};
use crate::determinism::{MAX_NODE_ID_BYTES, RandomStream};
use crate::generator::SceneGenerator;
use crate::params::{
    MAX_CATCH_UP_STEPS, MAX_GRID_CELLS, MAX_GRID_DIMENSION, MAX_RUNTIME_RESOURCE_BYTES,
    MAX_SIMULATION_WORK_UNITS, SceneDimensions, SceneParameters,
};
use crate::resource::GeneratorPreparationPermit;
use crate::simulation::FixedStepContext;
use crate::snapshots::ApprovedSnapshots;

const Q16_ONE: i64 = 1 << 16;
const MAX_TRAIL_CELLS: u16 = 200;
const MAX_SPEED_Q16: u32 = 10 << 16;
const MAX_DENSITY_Q16: u32 = 1 << 16;
const MATRIX_PARAMETER_SCHEMA_VERSION: u32 = 1;
const COLUMN_RECORD_BYTES: u64 = core::mem::size_of::<ColumnStream>() as u64;

const MATRIX_PARAMETERS: &[ParameterDescriptor] = &[
    ParameterDescriptor {
        name: "density",
        group: "Streams",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(1.0),
        default: ParameterDefault::Float(0.2),
        description: "Fraction of grid columns with a stream head.",
    },
    ParameterDescriptor {
        name: "speed",
        group: "Motion",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(10.0),
        default: ParameterDefault::Float(0.5),
        description: "Stream speed in cells per second; zero holds stream positions.",
    },
    ParameterDescriptor {
        name: "trail_cells",
        group: "Appearance",
        kind: ParameterKind::U16,
        minimum: Some(1.0),
        maximum: Some(200.0),
        default: ParameterDefault::U16(20),
        description: "Maximum rendered trail length, clipped to the grid.",
    },
    ParameterDescriptor {
        name: "color",
        group: "Appearance",
        kind: ParameterKind::ColorRgba,
        minimum: None,
        maximum: None,
        default: ParameterDefault::ColorRgba(0x00ff_66ff),
        description: "Portable #RRGGBBAA stream color.",
    },
];

pub const MATRIX_RAIN_PRESETS: &[MatrixRainPresetDescriptor] = &[
    MatrixRainPresetDescriptor {
        name: "ambient",
        config: MatrixRainConfig::ambient(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    MatrixRainPresetDescriptor {
        name: "dense",
        config: MatrixRainConfig::dense(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    MatrixRainPresetDescriptor {
        name: "static",
        config: MatrixRainConfig::static_motion(),
        motion: PresetMotion::Static,
        cost_is_estimate: true,
    },
];

/// Immutable local matrix-rain contract. Cost fields are limits/estimates until measured by a host.
pub const MATRIX_RAIN_DESCRIPTOR: GeneratorDescriptor = GeneratorDescriptor {
    id: "builtin.matrix-rain",
    version: 1,
    parameter_schema_version: MATRIX_PARAMETER_SCHEMA_VERSION,
    parameters: MATRIX_PARAMETERS,
    limits: GeneratorLimits {
        max_columns: MAX_GRID_DIMENSION,
        max_rows: MAX_GRID_DIMENSION,
        max_cells: MAX_GRID_CELLS,
        max_node_id_bytes: MAX_NODE_ID_BYTES,
        max_particles: 0,
        default_catch_up_steps: 4,
        max_catch_up_steps: MAX_CATCH_UP_STEPS,
        max_runtime_resource_bytes: MAX_RUNTIME_RESOURCE_BYTES,
        max_work_units_per_advance: MAX_SIMULATION_WORK_UNITS,
        max_velocity_q16: 10 << 16,
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

/// Validated exact portable values for the matrix-rain node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatrixRainConfig {
    density_q16: u32,
    speed_q16: u32,
    trail_cells: u16,
    color_rgba: u32,
}

impl MatrixRainConfig {
    /// Rejects non-finite and out-of-range values before runtime resource reservation.
    pub fn new(density: f32, speed: f32, trail_cells: u16, color_rgba: u32) -> Result<Self, Error> {
        if !density.is_finite()
            || !(0.0..=1.0).contains(&density)
            || !speed.is_finite()
            || !(0.0..=10.0).contains(&speed)
            || !(1..=MAX_TRAIL_CELLS).contains(&trail_cells)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            density_q16: (f64::from(density) * f64::from(1 << 16)).round() as u32,
            speed_q16: (f64::from(speed) * f64::from(1 << 16)).round() as u32,
            trail_cells,
            color_rgba,
        })
    }
    /// Parses the exact portable hexadecimal spelling `#RRGGBBAA`.
    pub fn from_hex_rgba(
        density: f32,
        speed: f32,
        trail_cells: u16,
        color: &str,
    ) -> Result<Self, Error> {
        let digits = color
            .strip_prefix('#')
            .filter(|digits| digits.len() == 8)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        let color_rgba = u32::from_str_radix(digits, 16)
            .map_err(|_| Error::Malformed(MalformedReason::InvalidConfiguration))?;
        Self::new(density, speed, trail_cells, color_rgba)
    }

    #[must_use]
    pub const fn ambient() -> Self {
        Self {
            density_q16: 13_107,
            speed_q16: 32_768,
            trail_cells: 20,
            color_rgba: 0x00ff_66ff,
        }
    }

    #[must_use]
    pub const fn dense() -> Self {
        Self {
            density_q16: 52_429,
            speed_q16: 131_072,
            trail_cells: 60,
            color_rgba: 0x00ff_66ff,
        }
    }

    #[must_use]
    pub const fn static_motion() -> Self {
        Self {
            density_q16: 13_107,
            speed_q16: 0,
            trail_cells: 20,
            color_rgba: 0x00ff_66ff,
        }
    }

    #[must_use]
    pub const fn density_q16(self) -> u32 {
        self.density_q16
    }

    #[must_use]
    pub const fn speed_q16(self) -> u32 {
        self.speed_q16
    }

    #[must_use]
    pub const fn trail_cells(self) -> u16 {
        self.trail_cells
    }

    #[must_use]
    pub const fn color_rgba(self) -> u32 {
        self.color_rgba
    }
}

/// Matrix-specific preset parameters without changing SCN-001's generic preset contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatrixRainPresetDescriptor {
    pub name: &'static str,
    pub config: MatrixRainConfig,
    pub motion: PresetMotion,
    pub cost_is_estimate: bool,
}

#[derive(Clone, Copy, Debug)]
struct ColumnStream {
    head_q16: i64,
    active: bool,
    glyph_offset: u32,
    random: RandomStream,
}

impl Default for ColumnStream {
    fn default() -> Self {
        Self {
            head_q16: 0,
            active: false,
            glyph_offset: 0,
            random: RandomStream::new(0),
        }
    }
}

/// Deterministic bounded column streams backed by an immutable FNT glyph table.
pub struct MatrixRainGenerator<'a> {
    node_id: &'a str,
    config: MatrixRainConfig,
    glyph_table: &'a GlyphTable,
    columns: Vec<ColumnStream>,
    random: RandomStream,
}

impl<'a> MatrixRainGenerator<'a> {
    /// Creates a generator without allocating; the borrowed glyph table remains immutable.
    #[must_use]
    pub const fn new(
        node_id: &'a str,
        config: MatrixRainConfig,
        glyph_table: &'a GlyphTable,
    ) -> Self {
        Self {
            node_id,
            config,
            glyph_table,
            columns: Vec::new(),
            random: RandomStream::new(0),
        }
    }

    #[must_use]
    pub const fn config(&self) -> MatrixRainConfig {
        self.config
    }

    fn checked_glyphs(&self) -> Result<&[ascii_convert::GlyphRecord], Error> {
        let glyphs = self.glyph_table.glyphs();
        if glyphs.is_empty() || glyphs.len() > u32::MAX as usize {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(glyphs)
    }
}

impl crate::generator::sealed::Sealed for MatrixRainGenerator<'_> {}

impl SceneGenerator for MatrixRainGenerator<'_> {
    fn descriptor(&self) -> &'static GeneratorDescriptor {
        &MATRIX_RAIN_DESCRIPTOR
    }

    fn node_id(&self) -> &str {
        self.node_id
    }

    fn scratch_bytes(&self, _parameters: &SceneParameters) -> Result<u64, Error> {
        u64::from(MAX_GRID_DIMENSION)
            .checked_mul(COLUMN_RECORD_BYTES)
            .ok_or(Error::SizeOverflow)
    }

    fn scratch_bytes_for_dimensions(
        &self,
        _parameters: &SceneParameters,
        dimensions: SceneDimensions,
    ) -> Result<u64, Error> {
        u64::from(dimensions.columns())
            .checked_mul(COLUMN_RECORD_BYTES)
            .ok_or(Error::SizeOverflow)
    }

    fn additional_work_units_per_advance(
        &self,
        dimensions: SceneDimensions,
        parameters: &SceneParameters,
        max_catch_up_steps: u8,
    ) -> Result<u64, Error> {
        if self.config.speed_q16 == 0
            || parameters.time_scale_q16() == 0
            || !parameters.background_motion()
        {
            return Ok(0);
        }
        let columns = u64::from(dimensions.columns());
        let step_work = columns
            .checked_mul(u64::from(max_catch_up_steps))
            .ok_or(Error::SizeOverflow)?;
        let render_work = columns
            .checked_mul(u64::from(self.config.trail_cells))
            .ok_or(Error::SizeOverflow)?;
        step_work
            .checked_add(render_work)
            .ok_or(Error::SizeOverflow)
    }

    fn prepare(
        &mut self,
        _permit: &GeneratorPreparationPermit<'_>,
        parameters: &SceneParameters,
        dimensions: SceneDimensions,
    ) -> Result<(), Error> {
        if self.node_id.is_empty()
            || self.node_id.len() > MAX_NODE_ID_BYTES
            || self.config.density_q16 > MAX_DENSITY_Q16
            || self.config.speed_q16 > MAX_SPEED_Q16
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        self.checked_glyphs()?;
        let column_count =
            usize::try_from(dimensions.columns()).map_err(|_| Error::SizeOverflow)?;
        self.columns.clear();
        self.columns
            .try_reserve_exact(column_count)
            .map_err(|_| Error::AllocationFailed)?;
        self.columns.resize(column_count, ColumnStream::default());
        self.random = RandomStream::for_node(parameters.seed(), self.node_id)?;
        let glyph_count = self.glyph_table.glyphs().len() as u32;
        for stream in &mut self.columns {
            stream.random = RandomStream::new(self.random.next_u64());
            stream.active =
                stream.random.below(MAX_DENSITY_Q16).unwrap_or(0) < self.config.density_q16;
            stream.head_q16 = -i64::from(
                stream
                    .random
                    .below(dimensions.rows().saturating_add(1))
                    .unwrap_or(0),
            ) * Q16_ONE;
            stream.glyph_offset = stream.random.below(glyph_count).unwrap_or(0);
        }
        Ok(())
    }

    fn step(
        &mut self,
        context: FixedStepContext,
        dimensions: SceneDimensions,
        parameters: &SceneParameters,
        _snapshots: &ApprovedSnapshots<'_>,
    ) {
        let speed =
            u64::from(self.config.speed_q16).saturating_mul(u64::from(parameters.time_scale_q16()));
        let denominator = u64::from(context.simulation_hz()) * (1_u64 << 16);
        let delta = i64::try_from(speed / denominator).unwrap_or(i64::MAX);
        let bottom = i64::from(dimensions.rows()) * Q16_ONE;
        let recycle_at = bottom + i64::from(self.config.trail_cells) * Q16_ONE;
        let glyph_count = self.glyph_table.glyphs().len() as u32;
        for stream in &mut self.columns {
            if !stream.active {
                continue;
            }
            stream.head_q16 = stream.head_q16.saturating_add(delta);
            if stream.head_q16 > recycle_at {
                stream.head_q16 = -i64::from(
                    stream
                        .random
                        .below(dimensions.rows().saturating_add(1))
                        .unwrap_or(0),
                ) * Q16_ONE;
                stream.glyph_offset = stream.random.below(glyph_count).unwrap_or(0);
            }
        }
    }

    fn is_static(&self) -> bool {
        self.config.speed_q16 == 0
    }

    fn render(&self, dimensions: SceneDimensions, output: &mut [char]) {
        if output.len() != dimensions.cell_count() {
            return;
        }
        output.fill(' ');
        let glyphs = self.glyph_table.glyphs();
        let glyph_count = glyphs.len() as u32;
        let columns = dimensions.columns() as usize;
        let row_count = dimensions.rows() as i64;
        for (column_index, stream) in self.columns.iter().enumerate() {
            if !stream.active {
                continue;
            }
            let head_row = stream.head_q16.div_euclid(Q16_ONE);
            for offset in 0..i64::from(self.config.trail_cells) {
                let row = head_row - offset;
                if !(0..row_count).contains(&row) {
                    continue;
                }
                let row_index = row as usize;
                let Some(cell_index) = row_index
                    .checked_mul(columns)
                    .and_then(|base| base.checked_add(column_index))
                    .filter(|index| *index < output.len())
                else {
                    continue;
                };
                let glyph_index = (stream.glyph_offset.wrapping_add(row as u32)) % glyph_count;
                output[cell_index] = glyphs[glyph_index as usize].character();
            }
        }
    }
}
