use std::time::Duration;

use ascii_core::{Error, MalformedReason};

use crate::params::{
    DEFAULT_CATCH_UP_STEPS, MAX_CATCH_UP_STEPS, SceneParameters, TIME_SCALE_ONE_Q16,
};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const CLOCK_UNITS_PER_STEP: u128 = NANOS_PER_SECOND * TIME_SCALE_ONE_Q16 as u128;

/// Bounded fixed-step policy. The default is four updates per host advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimulationLimits {
    max_catch_up_steps: u8,
}

impl SimulationLimits {
    pub fn new(max_catch_up_steps: u8) -> Result<Self, Error> {
        if !(1..=MAX_CATCH_UP_STEPS).contains(&max_catch_up_steps) {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
        Ok(Self { max_catch_up_steps })
    }

    #[must_use]
    pub const fn max_catch_up_steps(self) -> u8 {
        self.max_catch_up_steps
    }
}

impl Default for SimulationLimits {
    fn default() -> Self {
        Self {
            max_catch_up_steps: DEFAULT_CATCH_UP_STEPS,
        }
    }
}

/// Context supplied to one deterministic fixed-step generator update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedStepContext {
    step_index: u64,
    simulation_hz: u16,
}

impl FixedStepContext {
    #[must_use]
    pub const fn step_index(self) -> u64 {
        self.step_index
    }

    #[must_use]
    pub const fn simulation_hz(self) -> u16 {
        self.simulation_hz
    }
}

/// Per-host-advance simulation work and resume-clamp evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdvanceReport {
    simulation_steps: u8,
    elapsed_clamped: bool,
    final_step_index: u64,
    output_refreshed: bool,
}

impl AdvanceReport {
    #[must_use]
    pub const fn simulation_steps(self) -> u8 {
        self.simulation_steps
    }

    #[must_use]
    pub const fn elapsed_clamped(self) -> bool {
        self.elapsed_clamped
    }

    #[must_use]
    pub const fn final_step_index(self) -> u64 {
        self.final_step_index
    }

    #[must_use]
    pub const fn output_refreshed(self) -> bool {
        self.output_refreshed
    }

    pub(crate) const fn with_output_refreshed(mut self, output_refreshed: bool) -> Self {
        self.output_refreshed = output_refreshed;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FixedStepSimulation {
    limits: SimulationLimits,
    accumulator_units: u128,
    step_index: u64,
}

impl FixedStepSimulation {
    pub(crate) const fn new(limits: SimulationLimits) -> Self {
        Self {
            limits,
            accumulator_units: 0,
            step_index: 0,
        }
    }

    pub(crate) const fn step_index(self) -> u64 {
        self.step_index
    }

    pub(crate) fn advance(
        &mut self,
        elapsed: Duration,
        parameters: SceneParameters,
    ) -> Result<AdvanceReport, Error> {
        if parameters.is_static() {
            return Ok(self.freeze());
        }

        let elapsed_units = elapsed
            .as_nanos()
            .checked_mul(u128::from(parameters.simulation_hz()))
            .and_then(|units| units.checked_mul(u128::from(parameters.time_scale_q16())))
            .ok_or(Error::SizeOverflow)?;
        let maximum_units = u128::from(self.limits.max_catch_up_steps) * CLOCK_UNITS_PER_STEP;
        let elapsed_clamped = elapsed_units > maximum_units;
        let accepted_units = elapsed_units.min(maximum_units);
        let total_units = self
            .accumulator_units
            .checked_add(accepted_units)
            .ok_or(Error::SizeOverflow)?;
        let available_steps = total_units / CLOCK_UNITS_PER_STEP;
        let simulation_steps =
            u8::try_from(available_steps.min(u128::from(self.limits.max_catch_up_steps)))
                .map_err(|_| Error::SizeOverflow)?;
        let consumed_units = u128::from(simulation_steps) * CLOCK_UNITS_PER_STEP;
        let mut next_accumulator_units = total_units - consumed_units;
        if next_accumulator_units >= CLOCK_UNITS_PER_STEP {
            next_accumulator_units %= CLOCK_UNITS_PER_STEP;
        }
        let final_step_index = self
            .step_index
            .checked_add(u64::from(simulation_steps))
            .ok_or(Error::SizeOverflow)?;
        self.accumulator_units = next_accumulator_units;
        self.step_index = final_step_index;
        Ok(AdvanceReport {
            simulation_steps,
            elapsed_clamped,
            final_step_index,
            output_refreshed: simulation_steps > 0,
        })
    }

    pub(crate) fn freeze(&mut self) -> AdvanceReport {
        self.accumulator_units = 0;
        AdvanceReport {
            simulation_steps: 0,
            elapsed_clamped: false,
            final_step_index: self.step_index,
            output_refreshed: false,
        }
    }

    pub(crate) fn advance_exact_steps(
        &mut self,
        steps: u64,
        simulation_hz: u16,
    ) -> Result<impl Iterator<Item = FixedStepContext>, Error> {
        let final_step_index = self
            .step_index
            .checked_add(steps)
            .ok_or(Error::SizeOverflow)?;
        let first_step_index = self.step_index;
        self.accumulator_units = 0;
        self.step_index = final_step_index;
        Ok((1..=steps).map(move |offset| FixedStepContext {
            step_index: first_step_index + offset,
            simulation_hz,
        }))
    }
    pub(crate) fn contexts(
        self,
        report: AdvanceReport,
        hz: u16,
    ) -> impl Iterator<Item = FixedStepContext> {
        let first_index = report.final_step_index - u64::from(report.simulation_steps);
        (1..=report.simulation_steps).map(move |offset| FixedStepContext {
            step_index: first_index + u64::from(offset),
            simulation_hz: hz,
        })
    }
}
