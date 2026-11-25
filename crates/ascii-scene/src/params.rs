use ascii_core::{Budget, BudgetKind, Error, GridSize, MalformedReason};

use crate::descriptor::ResourceEstimate;

pub const MAX_GRID_DIMENSION: u32 = 1_024;
pub const MAX_GRID_CELLS: u64 = 65_536;
pub const MAX_PARTICLES: u32 = 20_000;
pub const MAX_RUNTIME_RESOURCE_BYTES: u64 = 1_048_576;
pub const MAX_SIMULATION_WORK_UNITS: u64 = 1_000_000;
pub const MAX_CATCH_UP_STEPS: u8 = 8;
pub(crate) const DEFAULT_CATCH_UP_STEPS: u8 = 4;
pub(crate) const TIME_SCALE_ONE_Q16: u32 = 1 << 16;
pub(crate) const MAX_VELOCITY_Q16: i32 = 64 << 16;
pub(crate) const MAX_ACCELERATION_Q16: i32 = 16 << 16;
pub(crate) const MAX_PARTICLE_LIFETIME_TICKS: u32 = 7_200;

/// Validated, allocation-free dimensions for one character grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneDimensions {
    size: GridSize,
    cell_count: usize,
}

impl SceneDimensions {
    /// Validates nonzero dimensions and the per-axis and total-cell ceilings.
    pub fn new(columns: u32, rows: u32) -> Result<Self, Error> {
        let size = GridSize::new(columns, rows);
        if size.is_empty() {
            return Err(Error::Malformed(MalformedReason::EmptyDimensions));
        }
        if columns > MAX_GRID_DIMENSION || rows > MAX_GRID_DIMENSION {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        let cell_count = size.cell_count();
        if cell_count > MAX_GRID_CELLS {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        let cell_count = usize::try_from(cell_count).map_err(|_| Error::SizeOverflow)?;
        Ok(Self { size, cell_count })
    }

    #[must_use]
    pub const fn columns(self) -> u32 {
        self.size.columns()
    }

    #[must_use]
    pub const fn rows(self) -> u32 {
        self.size.rows()
    }

    #[must_use]
    pub const fn cell_count(self) -> usize {
        self.cell_count
    }

    #[must_use]
    pub const fn grid_size(self) -> GridSize {
        self.size
    }
}

/// A fully validated, copyable block of generator parameters.
///
/// Time scale is stored as unsigned Q16.16 so fixed-step clock arithmetic is stable and bounded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneParameters {
    seed: u32,
    time_scale_q16: u32,
    max_particles: u32,
    simulation_hz: u16,
    background_motion: bool,
}

impl SceneParameters {
    pub const DEFAULT: Self = Self {
        seed: 1,
        time_scale_q16: TIME_SCALE_ONE_Q16,
        max_particles: 2_000,
        simulation_hz: 60,
        background_motion: true,
    };

    /// Validates every caller-provided value before any runtime allocation can occur.
    pub fn new(
        seed: u32,
        time_scale: f32,
        max_particles: u32,
        simulation_hz: u16,
        background_motion: bool,
    ) -> Result<Self, Error> {
        if !time_scale.is_finite()
            || !(0.0..=4.0).contains(&time_scale)
            || max_particles > MAX_PARTICLES
            || !(15..=120).contains(&simulation_hz)
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let time_scale_q16 = (f64::from(time_scale) * f64::from(TIME_SCALE_ONE_Q16)).round() as u32;
        Ok(Self {
            seed,
            time_scale_q16,
            max_particles,
            simulation_hz,
            background_motion,
        })
    }

    #[must_use]
    pub const fn seed(self) -> u32 {
        self.seed
    }

    #[must_use]
    pub const fn time_scale_q16(self) -> u32 {
        self.time_scale_q16
    }

    #[must_use]
    pub fn time_scale(self) -> f32 {
        self.time_scale_q16 as f32 / TIME_SCALE_ONE_Q16 as f32
    }

    #[must_use]
    pub const fn max_particles(self) -> u32 {
        self.max_particles
    }

    #[must_use]
    pub const fn simulation_hz(self) -> u16 {
        self.simulation_hz
    }

    #[must_use]
    pub const fn background_motion(self) -> bool {
        self.background_motion
    }

    /// Returns a checked logical allocation and per-advance work estimate for this runtime.
    pub fn estimate_resources(
        self,
        dimensions: SceneDimensions,
        generator_scratch_bytes: u64,
        max_catch_up_steps: u8,
    ) -> Result<ResourceEstimate, Error> {
        if !(1..=MAX_CATCH_UP_STEPS).contains(&max_catch_up_steps) {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let character_bytes = (dimensions.cell_count() as u64)
            .checked_mul(core::mem::size_of::<char>() as u64)
            .ok_or(Error::SizeOverflow)?;
        let work_units = if !self.background_motion || self.time_scale_q16 == 0 {
            0
        } else {
            let particle_passes = u64::from(max_catch_up_steps)
                .checked_add(1)
                .ok_or(Error::SizeOverflow)?;
            let particle_work = u64::from(self.max_particles)
                .checked_mul(particle_passes)
                .ok_or(Error::SizeOverflow)?;
            (dimensions.cell_count() as u64)
                .checked_add(particle_work)
                .ok_or(Error::SizeOverflow)?
        };
        let total_bytes = character_bytes
            .checked_add(generator_scratch_bytes)
            .ok_or(Error::SizeOverflow)?;
        let budget = Budget::new(MAX_RUNTIME_RESOURCE_BYTES, 0, MAX_SIMULATION_WORK_UNITS);
        if total_bytes > budget.max_frame_bytes() {
            return Err(Error::BudgetExceeded(BudgetKind::FrameBytes));
        }
        if work_units > budget.max_work_units() {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }
        Ok(ResourceEstimate {
            character_bytes,
            generator_scratch_bytes,
            total_bytes,
            work_units,
        })
    }
}

impl Default for SceneParameters {
    fn default() -> Self {
        Self::DEFAULT
    }
}
