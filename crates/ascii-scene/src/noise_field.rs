use ascii_convert::GlyphTable;
use ascii_core::{Error, MalformedReason};

use crate::descriptor::{
    GeneratorDescriptor, GeneratorLimits, MotionPolicy, ParameterDefault, ParameterDescriptor,
    ParameterKind, PresetDescriptor, PresetMotion, ResourceEstimatePolicy,
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

const Q16: u64 = 65_536;
const PARAMETERS: &[ParameterDescriptor] = &[
    ParameterDescriptor {
        name: "scale",
        group: "Field",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.01),
        maximum: Some(100.0),
        default: ParameterDefault::Float(2.0),
        description: "Spatial noise scale.",
    },
    ParameterDescriptor {
        name: "speed",
        group: "Motion",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(10.0),
        default: ParameterDefault::Float(0.2),
        description: "Temporal offset speed; zero freezes the field.",
    },
    ParameterDescriptor {
        name: "color",
        group: "Appearance",
        kind: ParameterKind::ColorRgba,
        minimum: None,
        maximum: None,
        default: ParameterDefault::ColorRgba(0xcccc_ffff),
        description: "Portable #RRGGBBAA metadata; character output does not render color.",
    },
];
const PRESETS: &[NoiseFieldPresetDescriptor] = &[
    NoiseFieldPresetDescriptor {
        name: "cloud",
        config: NoiseFieldConfig::cloud(),
        terrain: None,
        motion: PresetMotion::Normal,
    },
    NoiseFieldPresetDescriptor {
        name: "fine-noise",
        config: NoiseFieldConfig::fine_noise(),
        terrain: None,
        motion: PresetMotion::Normal,
    },
    NoiseFieldPresetDescriptor {
        name: "terrain",
        config: NoiseFieldConfig::terrain(),
        terrain: Some(TerrainControls::default()),
        motion: PresetMotion::Static,
    },
];
const DESCRIPTOR_PRESETS: &[PresetDescriptor] = &[
    PresetDescriptor {
        name: "cloud",
        motion: PresetMotion::Normal,
        seed: 1,
        time_scale_q16: 65_536,
        max_particles: 0,
        simulation_hz: 60,
        background_motion: true,
        cost_is_estimate: true,
    },
    PresetDescriptor {
        name: "fine-noise",
        motion: PresetMotion::Normal,
        seed: 1,
        time_scale_q16: 65_536,
        max_particles: 0,
        simulation_hz: 60,
        background_motion: true,
        cost_is_estimate: true,
    },
    PresetDescriptor {
        name: "terrain",
        motion: PresetMotion::Static,
        seed: 1,
        time_scale_q16: 0,
        max_particles: 0,
        simulation_hz: 60,
        background_motion: false,
        cost_is_estimate: true,
    },
];
pub const NOISE_FIELD_NODE_ID: &str = "builtin.noise-field.default";
pub const NOISE_ALGORITHM: &str =
    "value-noise-q16-v1: SplitMix64 lattice hashing, fixed-point smoothstep interpolation";
pub const NOISE_FIELD_DESCRIPTOR: GeneratorDescriptor = GeneratorDescriptor {
    id: "builtin.noise-field",
    version: 1,
    parameter_schema_version: 1,
    parameters: PARAMETERS,
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
    deterministic_export: true,
    presets: DESCRIPTOR_PRESETS,
    resource_estimate: ResourceEstimatePolicy {
        character_bytes_per_cell: core::mem::size_of::<char>() as u64,
        particle_record_bytes: 0,
        excludes_allocator_overhead: true,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoiseFieldConfig {
    scale_q16: u32,
    speed_q16: u32,
    color_rgba: u32,
}
impl NoiseFieldConfig {
    pub fn new(scale: f32, speed: f32, color_rgba: u32) -> Result<Self, Error> {
        if !scale.is_finite()
            || !(0.01..=100.0).contains(&scale)
            || !speed.is_finite()
            || !(0.0..=10.0).contains(&speed)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            scale_q16: (f64::from(scale) * Q16 as f64).round() as u32,
            speed_q16: (f64::from(speed) * Q16 as f64).round() as u32,
            color_rgba,
        })
    }
    pub fn from_hex_rgba(scale: f32, speed: f32, color: &str) -> Result<Self, Error> {
        let digits = color
            .strip_prefix('#')
            .filter(|s| s.len() == 8)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        let rgba = u32::from_str_radix(digits, 16)
            .map_err(|_| Error::Malformed(MalformedReason::InvalidConfiguration))?;
        Self::new(scale, speed, rgba)
    }
    #[must_use]
    pub const fn cloud() -> Self {
        Self {
            scale_q16: 2 * 65_536,
            speed_q16: 13_107,
            color_rgba: 0xcccc_ffff,
        }
    }
    #[must_use]
    pub const fn fine_noise() -> Self {
        Self {
            scale_q16: 20 * 65_536,
            speed_q16: 32_768,
            color_rgba: 0x88ff_ccff,
        }
    }
    #[must_use]
    pub const fn terrain() -> Self {
        Self {
            scale_q16: 4 * 65_536,
            speed_q16: 0,
            color_rgba: 0xcccc_ffff,
        }
    }
    #[must_use]
    pub const fn scale_q16(self) -> u32 {
        self.scale_q16
    }
    #[must_use]
    pub const fn speed_q16(self) -> u32 {
        self.speed_q16
    }
    #[must_use]
    pub const fn color_rgba(self) -> u32 {
        self.color_rgba
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerrainControls {
    contour_spacing_q16: u32,
    seed: u32,
}
impl TerrainControls {
    pub fn new(contour_spacing: f32, seed: u32) -> Result<Self, Error> {
        if !contour_spacing.is_finite() || !(0.01..=1.0).contains(&contour_spacing) {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            contour_spacing_q16: (f64::from(contour_spacing) * Q16 as f64).round() as u32,
            seed,
        })
    }
    #[must_use]
    pub const fn default() -> Self {
        Self {
            contour_spacing_q16: 6_554,
            seed: 1,
        }
    }
    #[must_use]
    pub const fn contour_spacing_q16(self) -> u32 {
        self.contour_spacing_q16
    }
    #[must_use]
    pub const fn seed(self) -> u32 {
        self.seed
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoiseFieldPresetDescriptor {
    pub name: &'static str,
    pub config: NoiseFieldConfig,
    pub terrain: Option<TerrainControls>,
    pub motion: PresetMotion,
}
pub const NOISE_FIELD_PRESETS: &[NoiseFieldPresetDescriptor] = PRESETS;

pub struct NoiseFieldGenerator<'a> {
    glyph_table: &'a GlyphTable,
    config: NoiseFieldConfig,
    terrain: Option<TerrainControls>,
    node_id: &'a str,
    phase_q16: u32,
    seed: u64,
    static_motion: bool,
}
impl<'a> NoiseFieldGenerator<'a> {
    #[must_use]
    pub const fn new(glyph_table: &'a GlyphTable, config: NoiseFieldConfig) -> Self {
        Self {
            glyph_table,
            config,
            terrain: None,
            node_id: NOISE_FIELD_NODE_ID,
            phase_q16: 0,
            seed: 0,
            static_motion: false,
        }
    }
    #[must_use]
    pub const fn with_node_id(mut self, node_id: &'a str) -> Self {
        self.node_id = node_id;
        self
    }
    #[must_use]
    pub const fn with_terrain_controls(mut self, controls: TerrainControls) -> Self {
        self.terrain = Some(controls);
        self
    }
}
impl crate::generator::sealed::Sealed for NoiseFieldGenerator<'_> {}
impl SceneGenerator for NoiseFieldGenerator<'_> {
    fn descriptor(&self) -> &'static GeneratorDescriptor {
        &NOISE_FIELD_DESCRIPTOR
    }
    fn node_id(&self) -> &str {
        self.node_id
    }
    fn scratch_bytes(&self, _: &SceneParameters) -> Result<u64, Error> {
        Ok(16)
    }
    fn scratch_bytes_for_dimensions(
        &self,
        _: &SceneParameters,
        _: SceneDimensions,
    ) -> Result<u64, Error> {
        Ok(16)
    }
    fn prepare(
        &mut self,
        _: &GeneratorPreparationPermit<'_>,
        parameters: &SceneParameters,
        _: SceneDimensions,
    ) -> Result<(), Error> {
        if self.node_id.is_empty()
            || self.node_id.len() > MAX_NODE_ID_BYTES
            || self.glyph_table.glyphs().is_empty()
            || !(65..=6_553_600).contains(&self.config.scale_q16)
            || self.config.speed_q16 > 655_360
            || self
                .terrain
                .is_some_and(|t| t.contour_spacing_q16 == 0 || t.contour_spacing_q16 > 65_536)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let seed = crate::derive_node_seed(parameters.seed(), self.node_id)?;
        self.seed = if let Some(t) = self.terrain {
            crate::derive_effect_seed(parameters.seed(), self.node_id, "terrain")?
                ^ u64::from(t.seed)
        } else {
            seed
        };
        self.phase_q16 = 0;
        self.static_motion = self.config.speed_q16 == 0 || parameters.is_static();
        Ok(())
    }
    fn step(
        &mut self,
        context: FixedStepContext,
        _: SceneDimensions,
        parameters: &SceneParameters,
        _: &ApprovedSnapshots<'_>,
    ) {
        if self.config.speed_q16 == 0
            || parameters.time_scale_q16() == 0
            || !parameters.background_motion()
        {
            return;
        }
        self.phase_q16 = ((u128::from(context.step_index()) * u128::from(self.config.speed_q16)
            / u128::from(context.simulation_hz())) as u32)
            & 0xffff;
    }
    fn is_static(&self) -> bool {
        self.static_motion
    }
    fn render(&self, dims: SceneDimensions, output: &mut [char]) {
        if output.len() != dims.cell_count() {
            return;
        }
        let glyphs = self.glyph_table.glyphs();
        if glyphs.is_empty() {
            output.fill(' ');
            return;
        }
        let scale = u64::from(self.config.scale_q16);
        let phase = self.phase_q16;
        let spacing = self.terrain.map_or(0, |t| u64::from(t.contour_spacing_q16));
        for y in 0..dims.rows() {
            for x in 0..dims.columns() {
                let px = (u64::from(x) * Q16 / scale).min(u64::from(u32::MAX)) as u32;
                let py = (u64::from(y) * Q16 / scale).min(u64::from(u32::MAX)) as u32;
                let value = sample(self.seed, px, py.wrapping_add(phase));
                let index = if spacing > 0 {
                    let band = u64::from(value) % spacing;
                    if band < spacing.min(1_311) {
                        0
                    } else {
                        (u64::from(value) * (glyphs.len() - 1) as u64 / 65_535) as usize
                    }
                } else {
                    (u64::from(value) * (glyphs.len() - 1) as u64 / 65_535) as usize
                };
                output[y as usize * dims.columns() as usize + x as usize] =
                    glyphs[index].character();
            }
        }
    }
}
fn sample(seed: u64, x: u32, y: u32) -> u16 {
    let ix = x >> 16;
    let iy = y >> 16;
    let fx = fade(x as u16);
    let fy = fade(y as u16);
    let a = lattice(seed, ix, iy);
    let b = lattice(seed, ix.wrapping_add(1), iy);
    let c = lattice(seed, ix, iy.wrapping_add(1));
    let d = lattice(seed, ix.wrapping_add(1), iy.wrapping_add(1));
    let top = lerp(a, b, fx);
    let bottom = lerp(c, d, fx);
    lerp(top, bottom, fy)
}
fn lattice(seed: u64, x: u32, y: u32) -> u16 {
    let mut z = seed
        ^ (u64::from(x).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        ^ (u64::from(y).wrapping_mul(0xbf58_476d_1ce4_e5b9));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 48) as u16
}
fn fade(t: u16) -> u32 {
    let t = u64::from(t);
    let t2 = t * t / 65_535;
    let t3 = t2 * t / 65_535;
    (t3 * (3 * 65_535 - 2 * t) / 65_535) as u32
}
fn lerp(a: u16, b: u16, t: u32) -> u16 {
    let delta = i64::from(b) - i64::from(a);
    (i64::from(a) + delta * i64::from(t) / 65_535).clamp(0, 65_535) as u16
}
