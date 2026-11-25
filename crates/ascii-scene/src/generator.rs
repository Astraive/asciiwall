use std::time::Duration;

use ascii_core::{Error, MalformedReason};

use crate::descriptor::{GENERATOR_DESCRIPTOR, GeneratorDescriptor, ResourceEstimate};
use crate::determinism::{MAX_NODE_ID_BYTES, RandomStream};
use crate::params::{
    MAX_PARTICLE_LIFETIME_TICKS as MAX_LIFETIME_TICKS, MAX_PARTICLES, MAX_VELOCITY_Q16,
    SceneDimensions, SceneParameters,
};
use crate::resource::{ResourceReservation, SceneResourceBudget};
use crate::simulation::{AdvanceReport, FixedStepContext, FixedStepSimulation, SimulationLimits};
use crate::snapshots::ApprovedSnapshots;

const Q16_ONE: i32 = 1 << 16;
const INITIAL_MAX_SPEED_Q16: i32 = 4 * Q16_ONE;
const INITIAL_MAX_ACCELERATION_Q16: i32 = Q16_ONE;
const MIN_LIFETIME_TICKS: u32 = 120;

pub const STARFIELD_NODE_ID: &str = "builtin.particle-field.default";

mod sealed {
    pub trait Sealed {}
}
/// Reviewed first-party generator metadata; mutable execution stays inside `SceneRuntime`.
#[allow(private_bounds)]
pub trait SceneGenerator: sealed::Sealed {
    fn descriptor(&self) -> &'static GeneratorDescriptor;
    fn node_id(&self) -> &str;
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    x_q16: i32,
    y_q16: i32,
    velocity_x_q16: i32,
    velocity_y_q16: i32,
    acceleration_x_q16: i32,
    acceleration_y_q16: i32,
    age_ticks: u32,
    lifetime_ticks: u32,
}
pub(crate) const PARTICLE_RECORD_BYTES: u64 = core::mem::size_of::<Particle>() as u64;

/// Deterministic bounded particle field used to exercise the first-party scene contract.
pub struct ParticleFieldGenerator<'a> {
    node_id: &'a str,
    particles: Vec<Particle>,
    random: RandomStream,
}

impl<'a> ParticleFieldGenerator<'a> {
    #[must_use]
    pub const fn new(node_id: &'a str) -> Self {
        Self {
            node_id,
            particles: Vec::new(),
            random: RandomStream::new(0),
        }
    }
}
impl sealed::Sealed for ParticleFieldGenerator<'_> {}

impl Default for ParticleFieldGenerator<'static> {
    fn default() -> Self {
        Self::new(STARFIELD_NODE_ID)
    }
}

impl SceneGenerator for ParticleFieldGenerator<'_> {
    fn descriptor(&self) -> &'static GeneratorDescriptor {
        &GENERATOR_DESCRIPTOR
    }

    fn node_id(&self) -> &str {
        self.node_id
    }
}

impl ParticleFieldGenerator<'_> {
    fn scratch_bytes(&self, parameters: &SceneParameters) -> Result<u64, Error> {
        let required_particles =
            usize::try_from(parameters.max_particles()).map_err(|_| Error::SizeOverflow)?;
        u64::try_from(required_particles)
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(PARTICLE_RECORD_BYTES)
            .ok_or(Error::SizeOverflow)
    }

    fn prepare(
        &mut self,
        parameters: &SceneParameters,
        dimensions: SceneDimensions,
    ) -> Result<(), Error> {
        if self.node_id.is_empty()
            || self.node_id.len() > MAX_NODE_ID_BYTES
            || parameters.max_particles() > MAX_PARTICLES
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let particle_count =
            usize::try_from(parameters.max_particles()).map_err(|_| Error::SizeOverflow)?;
        self.particles.clear();
        self.particles
            .try_reserve_exact(particle_count)
            .map_err(|_| Error::AllocationFailed)?;
        self.random = RandomStream::for_node(parameters.seed(), self.node_id)?;
        for _ in 0..particle_count {
            self.particles
                .push(random_particle(&mut self.random, dimensions));
        }
        Ok(())
    }

    fn step(
        &mut self,
        context: FixedStepContext,
        dimensions: SceneDimensions,
        _parameters: &SceneParameters,
        _snapshots: &ApprovedSnapshots<'_>,
    ) {
        let hz = i32::from(context.simulation_hz());
        let width_q16 = dimensions.columns() as i32 * Q16_ONE;
        let height_q16 = dimensions.rows() as i32 * Q16_ONE;
        for particle in &mut self.particles {
            particle.velocity_x_q16 = (particle.velocity_x_q16 + particle.acceleration_x_q16 / hz)
                .clamp(-MAX_VELOCITY_Q16, MAX_VELOCITY_Q16);
            particle.velocity_y_q16 = (particle.velocity_y_q16 + particle.acceleration_y_q16 / hz)
                .clamp(-MAX_VELOCITY_Q16, MAX_VELOCITY_Q16);
            particle.x_q16 = (particle.x_q16 + particle.velocity_x_q16 / hz).rem_euclid(width_q16);
            particle.y_q16 = (particle.y_q16 + particle.velocity_y_q16 / hz).rem_euclid(height_q16);
            particle.age_ticks = particle.age_ticks.saturating_add(1);
            if particle.age_ticks >= particle.lifetime_ticks {
                *particle = random_particle(&mut self.random, dimensions);
            }
        }
    }

    fn render(&self, dimensions: SceneDimensions, output: &mut [char]) {
        if output.len() != dimensions.cell_count() {
            return;
        }
        output.fill(' ');
        for particle in &self.particles {
            let column = (particle.x_q16 >> 16) as usize;
            let row = (particle.y_q16 >> 16) as usize;
            let index = row * dimensions.columns() as usize + column;
            let life_fraction = particle.age_ticks.saturating_mul(3) / particle.lifetime_ticks;
            output[index] = match life_fraction {
                0 => '.',
                1 => '+',
                _ => '*',
            };
        }
    }
}

fn random_particle(random: &mut RandomStream, dimensions: SceneDimensions) -> Particle {
    let x = random.below(dimensions.columns()).unwrap_or(0) as i32 * Q16_ONE
        + random.below(Q16_ONE as u32).unwrap_or(0) as i32;
    let y = random.below(dimensions.rows()).unwrap_or(0) as i32 * Q16_ONE
        + random.below(Q16_ONE as u32).unwrap_or(0) as i32;
    let lifetime_ticks = MIN_LIFETIME_TICKS
        + random
            .below(MAX_LIFETIME_TICKS - MIN_LIFETIME_TICKS + 1)
            .unwrap_or(0);
    Particle {
        x_q16: x,
        y_q16: y,
        velocity_x_q16: random_signed(random, INITIAL_MAX_SPEED_Q16),
        velocity_y_q16: random_signed(random, INITIAL_MAX_SPEED_Q16),
        acceleration_x_q16: random_signed(random, INITIAL_MAX_ACCELERATION_Q16),
        acceleration_y_q16: random_signed(random, INITIAL_MAX_ACCELERATION_Q16),
        age_ticks: random.below(lifetime_ticks).unwrap_or(0),
        lifetime_ticks,
    }
}

fn random_signed(random: &mut RandomStream, magnitude: i32) -> i32 {
    let range = magnitude as u32 * 2 + 1;
    random.below(range).unwrap_or(0) as i32 - magnitude
}

/// Owns fixed-step state and reusable output storage for one generator node.
pub struct SceneRuntime<'a> {
    parameters: SceneParameters,
    dimensions: SceneDimensions,
    generator: ParticleFieldGenerator<'a>,
    simulation: FixedStepSimulation,
    frame: Vec<char>,
    resource_estimate: ResourceEstimate,
    _reservation: ResourceReservation,
}

impl<'a> SceneRuntime<'a> {
    /// Constructs a runtime with the four-step default catch-up limit.
    pub fn try_new(
        parameters: SceneParameters,
        dimensions: SceneDimensions,
        generator: ParticleFieldGenerator<'a>,
        budget: &SceneResourceBudget,
    ) -> Result<Self, Error> {
        Self::try_new_with_limits(
            parameters,
            dimensions,
            generator,
            budget,
            SimulationLimits::default(),
        )
    }

    /// Validates and reserves all logical storage before allocating either reusable buffer.
    pub fn try_new_with_limits(
        parameters: SceneParameters,
        dimensions: SceneDimensions,
        mut generator: ParticleFieldGenerator<'a>,
        budget: &SceneResourceBudget,
        limits: SimulationLimits,
    ) -> Result<Self, Error> {
        if generator.node_id().is_empty() || generator.node_id().len() > MAX_NODE_ID_BYTES {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let scratch_bytes = generator.scratch_bytes(&parameters)?;
        let resource_estimate = parameters.estimate_resources(
            dimensions,
            scratch_bytes,
            limits.max_catch_up_steps(),
        )?;
        let reservation = budget.reserve(resource_estimate.total_bytes())?;

        let mut frame = Vec::new();
        frame
            .try_reserve_exact(dimensions.cell_count())
            .map_err(|_| Error::AllocationFailed)?;
        frame.resize(dimensions.cell_count(), ' ');
        generator.prepare(&parameters, dimensions)?;
        generator.render(dimensions, &mut frame);

        Ok(Self {
            parameters,
            dimensions,
            generator,
            simulation: FixedStepSimulation::new(limits),
            frame,
            resource_estimate,
            _reservation: reservation,
        })
    }

    /// Advances at most the configured fixed-step budget and reuses the existing frame buffer.
    pub fn advance(
        &mut self,
        elapsed: Duration,
        snapshots: &ApprovedSnapshots<'_>,
    ) -> Result<AdvanceReport, Error> {
        let report = self.simulation.advance(elapsed, self.parameters)?;
        for context in self
            .simulation
            .contexts(report, self.parameters.simulation_hz())
        {
            self.generator
                .step(context, self.dimensions, &self.parameters, snapshots);
        }
        let output_refreshed = report.simulation_steps() > 0;
        if output_refreshed {
            self.generator.render(self.dimensions, &mut self.frame);
        }
        Ok(report.with_output_refreshed(output_refreshed))
    }

    pub(crate) fn advance_exact_steps(&mut self, requested_steps: u64) -> Result<(), Error> {
        let simulation_steps = if self.parameters.is_static() {
            0
        } else {
            requested_steps
        };
        let snapshots = ApprovedSnapshots::empty();
        for context in self
            .simulation
            .advance_exact_steps(simulation_steps, self.parameters.simulation_hz())?
        {
            self.generator
                .step(context, self.dimensions, &self.parameters, &snapshots);
        }
        if simulation_steps > 0 {
            self.generator.render(self.dimensions, &mut self.frame);
        }
        Ok(())
    }

    #[must_use]
    pub const fn parameters(&self) -> SceneParameters {
        self.parameters
    }

    #[must_use]
    pub const fn dimensions(&self) -> SceneDimensions {
        self.dimensions
    }

    #[must_use]
    pub fn frame(&self) -> &[char] {
        &self.frame
    }

    #[must_use]
    pub const fn resource_estimate(&self) -> ResourceEstimate {
        self.resource_estimate
    }

    #[must_use]
    pub const fn simulation_step_count(&self) -> u64 {
        self.simulation.step_index()
    }

    #[must_use]
    pub fn frame_storage_capacity(&self) -> usize {
        self.frame.capacity()
    }
}
