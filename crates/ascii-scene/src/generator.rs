use std::time::Duration;

use ascii_core::{Error, MalformedReason};

use crate::descriptor::{GENERATOR_DESCRIPTOR, GeneratorDescriptor, ResourceEstimate};
use crate::determinism::{MAX_NODE_ID_BYTES, RandomStream};
use crate::params::{
    MAX_ACCELERATION_Q16, MAX_PARTICLE_LIFETIME_TICKS as MAX_LIFETIME_TICKS, MAX_PARTICLES,
    MAX_VELOCITY_Q16, SceneDimensions, SceneParameters,
};
use crate::resource::{GeneratorPreparationPermit, ResourceReservation, SceneResourceBudget};
use crate::simulation::{AdvanceReport, FixedStepContext, FixedStepSimulation, SimulationLimits};
use crate::snapshots::ApprovedSnapshots;

const Q16_ONE: i32 = 1 << 16;
const INITIAL_MAX_SPEED_Q16: i32 = 4 * Q16_ONE;
const INITIAL_MAX_ACCELERATION_Q16: i32 = Q16_ONE;
const MIN_LIFETIME_TICKS: u32 = 120;

pub const STARFIELD_NODE_ID: &str = "builtin.particle-field.default";

pub(crate) mod sealed {
    pub trait Sealed {}
}
/// Sealed first-party execution contract driven by the bounded `SceneRuntime`.
#[allow(private_bounds)]
pub trait SceneGenerator: sealed::Sealed {
    fn descriptor(&self) -> &'static GeneratorDescriptor;
    fn node_id(&self) -> &str;
    /// Returns checked logical scratch storage without allocating.
    fn scratch_bytes(&self, parameters: &SceneParameters) -> Result<u64, Error>;
    /// Returns checked logical scratch storage for the selected grid without allocating.
    fn scratch_bytes_for_dimensions(
        &self,
        parameters: &SceneParameters,
        dimensions: SceneDimensions,
    ) -> Result<u64, Error> {
        let _ = dimensions;
        self.scratch_bytes(parameters)
    }
    /// Returns checked bounded work beyond the shared frame/particle estimate.
    fn additional_work_units_per_advance(
        &self,
        _dimensions: SceneDimensions,
        _parameters: &SceneParameters,
        _max_catch_up_steps: u8,
    ) -> Result<u64, Error> {
        Ok(0)
    }
    /// Prepares bounded buffers; the runtime-only permit proves resources were reserved first.
    fn prepare(
        &mut self,
        permit: &GeneratorPreparationPermit<'_>,
        parameters: &SceneParameters,
        dimensions: SceneDimensions,
    ) -> Result<(), Error>;
    /// Performs one bounded fixed-step update without allocating.
    fn step(
        &mut self,
        context: FixedStepContext,
        dimensions: SceneDimensions,
        parameters: &SceneParameters,
        snapshots: &ApprovedSnapshots<'_>,
    );
    /// Returns true when this generator has no evolving state at its current configuration.
    fn is_static(&self) -> bool {
        false
    }
    /// Writes into caller-owned output storage without allocating.
    fn render(&self, dimensions: SceneDimensions, output: &mut [char]);
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
        _permit: &GeneratorPreparationPermit<'_>,
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
        snapshots: &ApprovedSnapshots<'_>,
    ) {
        let hz = i32::from(context.simulation_hz());
        let width_q16 = dimensions.columns() as i32 * Q16_ONE;
        let height_q16 = dimensions.rows() as i32 * Q16_ONE;
        let pointer_target = snapshots.pointer_position().map(|pointer| {
            (
                i32::from(pointer.x_q16()) * dimensions.columns() as i32,
                i32::from(pointer.y_q16()) * dimensions.rows() as i32,
            )
        });
        for particle in &mut self.particles {
            let (acceleration_x_q16, acceleration_y_q16) = pointer_target.map_or(
                (particle.acceleration_x_q16, particle.acceleration_y_q16),
                |(target_x_q16, target_y_q16)| {
                    let attraction_x =
                        shortest_toroidal_delta(particle.x_q16, target_x_q16, width_q16) / 8;
                    let attraction_y =
                        shortest_toroidal_delta(particle.y_q16, target_y_q16, height_q16) / 8;
                    (
                        (particle.acceleration_x_q16 + attraction_x)
                            .clamp(-MAX_ACCELERATION_Q16, MAX_ACCELERATION_Q16),
                        (particle.acceleration_y_q16 + attraction_y)
                            .clamp(-MAX_ACCELERATION_Q16, MAX_ACCELERATION_Q16),
                    )
                },
            );
            particle.velocity_x_q16 = (particle.velocity_x_q16 + acceleration_x_q16 / hz)
                .clamp(-MAX_VELOCITY_Q16, MAX_VELOCITY_Q16);
            particle.velocity_y_q16 = (particle.velocity_y_q16 + acceleration_y_q16 / hz)
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

fn shortest_toroidal_delta(from_q16: i32, to_q16: i32, extent_q16: i32) -> i32 {
    let forward = (to_q16 - from_q16).rem_euclid(extent_q16);
    if forward > extent_q16 / 2 {
        forward - extent_q16
    } else {
        forward
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
pub struct SceneRuntime<G: SceneGenerator> {
    parameters: SceneParameters,
    dimensions: SceneDimensions,
    generator: G,
    simulation: FixedStepSimulation,
    frame: Vec<char>,
    resource_estimate: ResourceEstimate,
    _reservation: ResourceReservation,
}

impl<G: SceneGenerator> SceneRuntime<G> {
    /// Constructs a runtime with the four-step default catch-up limit.
    pub fn try_new(
        parameters: SceneParameters,
        dimensions: SceneDimensions,
        generator: G,
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
        mut generator: G,
        budget: &SceneResourceBudget,
        limits: SimulationLimits,
    ) -> Result<Self, Error> {
        if generator.node_id().is_empty() || generator.node_id().len() > MAX_NODE_ID_BYTES {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        let scratch_bytes = generator.scratch_bytes_for_dimensions(&parameters, dimensions)?;
        let additional_work_units = generator.additional_work_units_per_advance(
            dimensions,
            &parameters,
            limits.max_catch_up_steps(),
        )?;
        let resource_estimate = parameters.estimate_resources_with_additional_work(
            dimensions,
            scratch_bytes,
            limits.max_catch_up_steps(),
            additional_work_units,
        )?;
        let reservation = budget.reserve(resource_estimate.total_bytes())?;

        let mut frame = Vec::new();
        frame
            .try_reserve_exact(dimensions.cell_count())
            .map_err(|_| Error::AllocationFailed)?;
        frame.resize(dimensions.cell_count(), ' ');
        {
            let permit = GeneratorPreparationPermit::new(&reservation);
            generator.prepare(&permit, &parameters, dimensions)?;
        }
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
        let report = if self.generator.is_static() {
            self.simulation.freeze()
        } else {
            self.simulation.advance(elapsed, self.parameters)?
        };
        if !self.generator.is_static() {
            for context in self
                .simulation
                .contexts(report, self.parameters.simulation_hz())
            {
                self.generator
                    .step(context, self.dimensions, &self.parameters, snapshots);
            }
        }
        let output_refreshed = report.simulation_steps() > 0 && !self.generator.is_static();
        if output_refreshed {
            self.generator.render(self.dimensions, &mut self.frame);
        }
        Ok(report.with_output_refreshed(output_refreshed))
    }

    pub(crate) fn advance_exact_steps_with<'snapshot, F>(
        &mut self,
        requested_steps: u64,
        mut snapshots_for_step: F,
    ) -> Result<(), Error>
    where
        F: FnMut(u64) -> ApprovedSnapshots<'snapshot>,
    {
        let simulation_steps = if self.parameters.is_static() || self.generator.is_static() {
            0
        } else {
            requested_steps
        };
        for context in self
            .simulation
            .advance_exact_steps(simulation_steps, self.parameters.simulation_hz())?
        {
            let snapshots = snapshots_for_step(context.step_index());
            self.generator
                .step(context, self.dimensions, &self.parameters, &snapshots);
        }
        if simulation_steps > 0 {
            self.generator.render(self.dimensions, &mut self.frame);
        }
        Ok(())
    }

    #[must_use]
    pub fn descriptor(&self) -> &'static GeneratorDescriptor {
        self.generator.descriptor()
    }

    #[must_use]
    pub fn node_id(&self) -> &str {
        self.generator.node_id()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    struct TestGenerator<'a> {
        inner: ParticleFieldGenerator<'static>,
        step_count: &'a Cell<u64>,
        fail_prepare: bool,
    }

    impl sealed::Sealed for TestGenerator<'_> {}

    impl SceneGenerator for TestGenerator<'_> {
        fn descriptor(&self) -> &'static GeneratorDescriptor {
            self.inner.descriptor()
        }

        fn node_id(&self) -> &str {
            self.inner.node_id()
        }

        fn scratch_bytes(&self, parameters: &SceneParameters) -> Result<u64, Error> {
            self.inner.scratch_bytes(parameters)
        }

        fn prepare(
            &mut self,
            permit: &GeneratorPreparationPermit<'_>,
            parameters: &SceneParameters,
            dimensions: SceneDimensions,
        ) -> Result<(), Error> {
            if self.fail_prepare {
                return Err(Error::AllocationFailed);
            }
            self.inner.prepare(permit, parameters, dimensions)
        }

        fn step(
            &mut self,
            context: FixedStepContext,
            dimensions: SceneDimensions,
            parameters: &SceneParameters,
            snapshots: &ApprovedSnapshots<'_>,
        ) {
            self.step_count.set(self.step_count.get().saturating_add(1));
            self.inner.step(context, dimensions, parameters, snapshots);
        }

        fn render(&self, dimensions: SceneDimensions, output: &mut [char]) {
            self.inner.render(dimensions, output);
        }
    }

    #[test]
    fn runtime_dispatches_execution_through_generator_contract() {
        let parameters = SceneParameters::default();
        let dimensions = SceneDimensions::new(16, 8).unwrap();
        let budget = SceneResourceBudget::default();
        let step_count = Cell::new(0);
        let mut scene = SceneRuntime::try_new(
            parameters,
            dimensions,
            TestGenerator {
                inner: ParticleFieldGenerator::default(),
                step_count: &step_count,
                fail_prepare: false,
            },
            &budget,
        )
        .unwrap();
        assert_eq!(scene.descriptor().id, GENERATOR_DESCRIPTOR.id);
        assert_eq!(scene.node_id(), STARFIELD_NODE_ID);
        let report = scene
            .advance(Duration::from_secs(1), &ApprovedSnapshots::empty())
            .unwrap();
        assert_eq!(report.simulation_steps(), 4);
        assert_eq!(step_count.get(), 4);

        let mut direct = SceneRuntime::try_new(
            parameters,
            dimensions,
            ParticleFieldGenerator::default(),
            &SceneResourceBudget::default(),
        )
        .unwrap();
        direct
            .advance(Duration::from_secs(1), &ApprovedSnapshots::empty())
            .unwrap();
        assert_eq!(scene.frame(), direct.frame());
    }

    #[test]
    fn failed_generator_preparation_releases_its_resource_reservation() {
        let budget = SceneResourceBudget::default();
        let step_count = Cell::new(0);
        let result = SceneRuntime::try_new(
            SceneParameters::default(),
            SceneDimensions::new(16, 8).unwrap(),
            TestGenerator {
                inner: ParticleFieldGenerator::default(),
                step_count: &step_count,
                fail_prepare: true,
            },
            &budget,
        );
        assert!(matches!(result, Err(Error::AllocationFailed)));
        assert_eq!(budget.reserved_bytes(), 0);
    }
}
