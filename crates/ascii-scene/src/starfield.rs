use ascii_convert::GlyphTable;
use ascii_core::{Error, MalformedReason};

use crate::descriptor::{
    GeneratorDescriptor, GeneratorLimits, MotionPolicy, ParameterDefault, ParameterDescriptor,
    ParameterKind, PresetMotion, ResourceEstimatePolicy,
};
use crate::determinism::{MAX_NODE_ID_BYTES, RandomStream};
use crate::generator::SceneGenerator;
use crate::params::{
    MAX_CATCH_UP_STEPS, MAX_GRID_CELLS, MAX_GRID_DIMENSION, MAX_PARTICLES,
    MAX_RUNTIME_RESOURCE_BYTES, MAX_SIMULATION_WORK_UNITS, SceneDimensions, SceneParameters,
};
use crate::resource::GeneratorPreparationPermit;
use crate::simulation::FixedStepContext;
use crate::snapshots::{ApprovedSnapshots, SignalKind, MAX_APPROVED_BUTTON_EVENTS};

const Q16: i64 = 1 << 16;
const MIN_DEPTH_Q16: i32 = 6_554; // rounded representation of 0.1
const MAX_DEPTH_Q16: i32 = 100 * Q16 as i32;
const MAX_SPEED_Q16: u32 = 10 << 16;
const STARFIELD_PARAMETER_SCHEMA_VERSION: u32 = 1;
const STARFIELD_FIXED_BYTES: u64 = core::mem::size_of::<u64>() as u64;
const PARTICLE_RECORD_BYTES: u64 = core::mem::size_of::<StarParticle>() as u64;
const STARFIELD_PARAMETERS: &[ParameterDescriptor] = &[
    ParameterDescriptor {
        name: "count",
        group: "Particles",
        kind: ParameterKind::U32,
        minimum: Some(1.0),
        maximum: Some(20_000.0),
        default: ParameterDefault::U32(500),
        description: "Requested particle count; the scene particle cap limits the effective count.",
    },
    ParameterDescriptor {
        name: "speed",
        group: "Motion",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.0),
        maximum: Some(10.0),
        default: ParameterDefault::Float(0.5),
        description: "Depth travel speed in units per second; zero freezes the field.",
    },
    ParameterDescriptor {
        name: "depth",
        group: "Projection",
        kind: ParameterKind::FiniteFloat,
        minimum: Some(0.1),
        maximum: Some(100.0),
        default: ParameterDefault::Float(20.0),
        description: "Far plane for depth projection; the near plane is fixed at 0.1.",
    },
    ParameterDescriptor {
        name: "color",
        group: "Appearance",
        kind: ParameterKind::ColorRgba,
        minimum: None,
        maximum: None,
        default: ParameterDefault::ColorRgba(0xd8e8_ffff),
        description: "Portable #RRGGBBAA color metadata for host renderers.",
    },
];

pub const STARFIELD_PRESETS: &[StarfieldPresetDescriptor] = &[
    StarfieldPresetDescriptor {
        name: "ambient",
        config: StarfieldConfig::ambient(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    StarfieldPresetDescriptor {
        name: "warp",
        config: StarfieldConfig::warp(),
        motion: PresetMotion::Normal,
        cost_is_estimate: true,
    },
    StarfieldPresetDescriptor {
        name: "still",
        config: StarfieldConfig::still(),
        motion: PresetMotion::Static,
        cost_is_estimate: true,
    },
];

/// Generator-local declaration only; no package schema or settings-registry support is implied.
pub const STARFIELD_DESCRIPTOR: GeneratorDescriptor = GeneratorDescriptor {
    id: "builtin.starfield",
    version: 1,
    parameter_schema_version: STARFIELD_PARAMETER_SCHEMA_VERSION,
    parameters: STARFIELD_PARAMETERS,
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
        max_velocity_q16: 10 << 16,
        max_acceleration_q16: 0,
        max_lifetime_ticks: 0,
    },
    supported_signals: &[SignalKind::PointerPosition, SignalKind::PointerButtons],
    reduced_motion: MotionPolicy::FreezeWhenBackgroundMotionDisabled,
    deterministic_export: false,
    presets: &[],
    resource_estimate: ResourceEstimatePolicy {
        character_bytes_per_cell: core::mem::size_of::<char>() as u64,
        particle_record_bytes: PARTICLE_RECORD_BYTES,
        excludes_allocator_overhead: true,
    },
};

/// Validated artist-facing values. Requested count is retained when the scene cap is lower.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StarfieldConfig {
    requested_count: u32,
    speed_q16: u32,
    depth_q16: i32,
    color_rgba: u32,
}

impl StarfieldConfig {
    pub fn new(count: u32, speed: f32, depth: f32, color_rgba: u32) -> Result<Self, Error> {
        if !(1..=MAX_PARTICLES).contains(&count)
            || !speed.is_finite()
            || !(0.0..=10.0).contains(&speed)
            || !depth.is_finite()
            || !(0.1..=100.0).contains(&depth)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self {
            requested_count: count,
            speed_q16: (f64::from(speed) * 65_536.0).round() as u32,
            depth_q16: (f64::from(depth) * 65_536.0).round() as i32,
            color_rgba,
        })
    }

    pub fn from_hex_rgba(count: u32, speed: f32, depth: f32, color: &str) -> Result<Self, Error> {
        let digits = color
            .strip_prefix('#')
            .filter(|digits| digits.len() == 8)
            .ok_or(Error::Malformed(MalformedReason::InvalidConfiguration))?;
        let rgba = u32::from_str_radix(digits, 16)
            .map_err(|_| Error::Malformed(MalformedReason::InvalidConfiguration))?;
        Self::new(count, speed, depth, rgba)
    }

    #[must_use]
    pub const fn ambient() -> Self {
        Self {
            requested_count: 500,
            speed_q16: 32_768,
            depth_q16: 20 * Q16 as i32,
            color_rgba: 0xd8e8_ffff,
        }
    }
    #[must_use]
    pub const fn warp() -> Self {
        Self {
            requested_count: 2_000,
            speed_q16: 3 * 65_536,
            depth_q16: 50 * Q16 as i32,
            color_rgba: 0xffff_ffff,
        }
    }
    #[must_use]
    pub const fn still() -> Self {
        Self {
            requested_count: 500,
            speed_q16: 0,
            depth_q16: 20 * Q16 as i32,
            color_rgba: 0xd8e8_ffff,
        }
    }
    #[must_use]
    pub const fn requested_count(self) -> u32 {
        self.requested_count
    }
    #[must_use]
    pub const fn speed_q16(self) -> u32 {
        self.speed_q16
    }
    #[must_use]
    pub const fn depth_q16(self) -> i32 {
        self.depth_q16
    }
    #[must_use]
    pub const fn color_rgba(self) -> u32 {
        self.color_rgba
    }
    #[must_use]
    pub fn speed(self) -> f32 {
        self.speed_q16 as f32 / 65_536.0
    }
    #[must_use]
    pub fn depth(self) -> f32 {
        self.depth_q16 as f32 / 65_536.0
    }
    #[must_use]
    pub const fn effective_count(self, scene_cap: u32) -> u32 {
        if self.requested_count < scene_cap {
            self.requested_count
        } else {
            scene_cap
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StarfieldPresetDescriptor {
    pub name: &'static str,
    pub config: StarfieldConfig,
    pub motion: PresetMotion,
    pub cost_is_estimate: bool,
}

#[derive(Clone, Copy, Debug)]
struct StarParticle {
    x_q16: i32,
    y_q16: i32,
    depth_q16: i32,
    random: RandomStream,
}

/// Deterministic perspective starfield using only a borrowed immutable glyph table.
pub struct StarfieldGenerator<'a> {
    node_id: &'a str,
    config: StarfieldConfig,
    glyph_table: &'a GlyphTable,
    particles: Vec<StarParticle>,
    random: RandomStream,
    effective_count: u32,
    depth_remainder: u64,
}

impl<'a> StarfieldGenerator<'a> {
    #[must_use]
    pub const fn new(
        node_id: &'a str,
        config: StarfieldConfig,
        glyph_table: &'a GlyphTable,
    ) -> Self {
        Self {
            node_id,
            config,
            glyph_table,
            particles: Vec::new(),
            random: RandomStream::new(0),
            effective_count: 0,
            depth_remainder: 0,
        }
    }
    #[must_use]
    pub const fn config(&self) -> StarfieldConfig {
        self.config
    }
    #[must_use]
    pub const fn requested_count(&self) -> u32 {
        self.config.requested_count
    }
    #[must_use]
    pub const fn effective_count(&self) -> u32 {
        self.effective_count
    }

    fn glyphs(&self) -> Result<&[ascii_convert::GlyphRecord], Error> {
        let glyphs = self.glyph_table.glyphs();
        if glyphs.is_empty() || glyphs.len() > u32::MAX as usize {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(glyphs)
    }

    fn recycle(particle: &mut StarParticle, far_depth_q16: i32) {
        particle.depth_q16 = MIN_DEPTH_Q16
            + particle
                .random
                .below((far_depth_q16 - MIN_DEPTH_Q16) as u32)
                .unwrap_or(0) as i32;
        particle.x_q16 = random_signed(&mut particle.random, particle.depth_q16);
        particle.y_q16 = random_signed(&mut particle.random, particle.depth_q16);
    }
}

impl crate::generator::sealed::Sealed for StarfieldGenerator<'_> {}

impl SceneGenerator for StarfieldGenerator<'_> {
    fn descriptor(&self) -> &'static GeneratorDescriptor {
        &STARFIELD_DESCRIPTOR
    }
    fn node_id(&self) -> &str {
        self.node_id
    }
    fn scratch_bytes(&self, parameters: &SceneParameters) -> Result<u64, Error> {
        u64::from(self.config.effective_count(parameters.max_particles()))
            .checked_mul(PARTICLE_RECORD_BYTES)
            .and_then(|bytes| bytes.checked_add(STARFIELD_FIXED_BYTES))
            .ok_or(Error::SizeOverflow)
    }
    fn scratch_bytes_for_dimensions(
        &self,
        parameters: &SceneParameters,
        _dimensions: SceneDimensions,
    ) -> Result<u64, Error> {
        self.scratch_bytes(parameters)
    }
    fn additional_work_units_per_advance(
        &self,
        _dimensions: SceneDimensions,
        parameters: &SceneParameters,
        max_catch_up_steps: u8,
    ) -> Result<u64, Error> {
        if self.config.speed_q16 == 0 || parameters.is_static() {
            return Ok(0);
        }
        // Each admitted event may be examined by every fixed step in a catch-up advance.
        u64::try_from(MAX_APPROVED_BUTTON_EVENTS)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(u64::from(max_catch_up_steps))
            .ok_or(Error::SizeOverflow)
    }
    fn prepare(
        &mut self,
        _permit: &GeneratorPreparationPermit<'_>,
        parameters: &SceneParameters,
        _dimensions: SceneDimensions,
    ) -> Result<(), Error> {
        if self.node_id.is_empty()
            || self.node_id.len() > MAX_NODE_ID_BYTES
            || self.config.requested_count == 0
            || self.config.requested_count > MAX_PARTICLES
            || self.config.speed_q16 > MAX_SPEED_Q16
            || !(MIN_DEPTH_Q16..=MAX_DEPTH_Q16).contains(&self.config.depth_q16)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        self.glyphs()?;
        self.effective_count = self.config.effective_count(parameters.max_particles());
        let count = usize::try_from(self.effective_count).map_err(|_| Error::SizeOverflow)?;
        self.particles.clear();
        self.particles
            .try_reserve_exact(count)
            .map_err(|_| Error::AllocationFailed)?;
        self.random = RandomStream::for_node(parameters.seed(), self.node_id)?;
        self.depth_remainder = 0;
        let far = self.config.depth_q16;
        for _ in 0..count {
            let mut particle = StarParticle {
                x_q16: 0,
                y_q16: 0,
                depth_q16: MIN_DEPTH_Q16,
                random: RandomStream::new(self.random.next_u64()),
            };
            Self::recycle(&mut particle, far);
            self.particles.push(particle);
        }
        Ok(())
    }
    fn step(
        &mut self,
        context: FixedStepContext,
        dimensions: SceneDimensions,
        parameters: &SceneParameters,
        snapshots: &ApprovedSnapshots<'_>,
    ) {
        if self.config.speed_q16 == 0
            || parameters.time_scale_q16() == 0
            || !parameters.background_motion()
        {
            return;
        }
        let button_warp = snapshots.pointer_buttons().is_some_and(|events| {
            events
                .iter()
                .any(|event| event.button() == 0 && event.pressed())
        });
        let pointer = snapshots.pointer_position();
        let focal_q16 = i64::from(dimensions.columns().min(dimensions.rows())) * (Q16 / 2);
        let denominator = u64::from(context.simulation_hz()) * 65_536;
        let warp_multiplier = if button_warp { 4 } else { 1 };
        let numerator = u64::from(self.config.speed_q16)
            * u64::from(parameters.time_scale_q16())
            * warp_multiplier
            + self.depth_remainder;
        let delta = (numerator / denominator) as i32;
        self.depth_remainder = numerator % denominator;
        for particle in &mut self.particles {
            particle.depth_q16 = particle.depth_q16.saturating_sub(delta);
            if let Some(point) = pointer {
                let target_x = ((i64::from(point.x_q16()) * 2 - 65_535)
                    * i64::from(particle.depth_q16))
                    / 65_535;
                let target_y = ((65_535 - i64::from(point.y_q16()) * 2)
                    * i64::from(particle.depth_q16))
                    / 65_535;
                particle.x_q16 =
                    (i64::from(particle.x_q16) + (target_x - i64::from(particle.x_q16)) / 128)
                        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                particle.y_q16 =
                    (i64::from(particle.y_q16) + (target_y - i64::from(particle.y_q16)) / 128)
                        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            }
            if particle.depth_q16 <= MIN_DEPTH_Q16 {
                Self::recycle(particle, self.config.depth_q16);
            } else {
                let z = i64::from(particle.depth_q16.max(MIN_DEPTH_Q16));
                let screen_x = i64::from(dimensions.columns()) * Q16 / 2
                    + i64::from(particle.x_q16) * focal_q16 / z;
                let screen_y = i64::from(dimensions.rows()) * Q16 / 2
                    - i64::from(particle.y_q16) * focal_q16 / z;
                if screen_x < 0
                    || screen_y < 0
                    || screen_x >= i64::from(dimensions.columns()) * Q16
                    || screen_y >= i64::from(dimensions.rows()) * Q16
                {
                    Self::recycle(particle, self.config.depth_q16);
                }
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
        if glyphs.is_empty() {
            return;
        }
        let focal_q16 = i64::from(dimensions.columns().min(dimensions.rows())) * (Q16 / 2);
        for particle in &self.particles {
            let z = i64::from(particle.depth_q16.max(MIN_DEPTH_Q16));
            let x = i64::from(dimensions.columns()) * Q16 / 2
                + i64::from(particle.x_q16) * focal_q16 / z;
            let y =
                i64::from(dimensions.rows()) * Q16 / 2 - i64::from(particle.y_q16) * focal_q16 / z;
            let column = x.div_euclid(Q16);
            let row = y.div_euclid(Q16);
            if column < 0
                || row < 0
                || column >= i64::from(dimensions.columns())
                || row >= i64::from(dimensions.rows())
            {
                continue;
            }
            let depth_range = i64::from(self.config.depth_q16 - MIN_DEPTH_Q16).max(1);
            let brightness = ((depth_range - i64::from(particle.depth_q16 - MIN_DEPTH_Q16))
                * (glyphs.len() as i64 - 1)
                / depth_range)
                .clamp(0, glyphs.len() as i64 - 1) as usize;
            let idx = row as usize * dimensions.columns() as usize + column as usize;
            output[idx] = glyphs[brightness].character();
        }
    }
}

fn random_signed(random: &mut RandomStream, magnitude: i32) -> i32 {
    let range = magnitude.unsigned_abs().saturating_mul(2).saturating_add(1);
    random.below(range).unwrap_or(0) as i32 - magnitude.abs()
}
