use ascii_core::{BudgetKind, Error, MalformedReason};

use crate::descriptor::{GENERATOR_DESCRIPTOR, ResourceEstimate};
use crate::generator::{ParticleFieldGenerator, SceneRuntime};
use crate::params::{MAX_SIMULATION_WORK_UNITS, SceneDimensions, SceneParameters};
use crate::resource::SceneResourceBudget;
use crate::snapshots::{NormalizedPoint, SignalKind};

pub const MAX_RECORDED_EVENTS: usize = 65_536;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordedEventKind {
    PointerPosition(NormalizedPoint),
    PointerButton { button: u8, pressed: bool },
}

impl RecordedEventKind {
    #[must_use]
    pub const fn signal_kind(self) -> SignalKind {
        match self {
            Self::PointerPosition(_) => SignalKind::PointerPosition,
            Self::PointerButton { .. } => SignalKind::PointerButtons,
        }
    }
}

/// One normalized synthetic event at a fixed simulation step, not a live OS event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordedEvent {
    step_index: u64,
    kind: RecordedEventKind,
}

impl RecordedEvent {
    #[must_use]
    pub const fn new(step_index: u64, kind: RecordedEventKind) -> Self {
        Self { step_index, kind }
    }

    #[must_use]
    pub const fn step_index(self) -> u64 {
        self.step_index
    }

    #[must_use]
    pub const fn kind(self) -> RecordedEventKind {
        self.kind
    }
}

/// Caller-supplied normalized events or an explicit no-input fallback.
#[derive(Clone, Copy, Debug)]
pub enum ExportSignalTrack<'a> {
    Recorded(&'a [RecordedEvent]),
    IdleFallback,
}

/// Complete caller-supplied identity and timeline inputs for deterministic scene export.
#[derive(Clone, Copy, Debug)]
pub struct ExportContext<'a> {
    parameters: SceneParameters,
    selected_start_timestamp_unix_ms: i64,
    settings_fingerprint: [u8; 32],
    font_identity: [u8; 32],
    signal_track: ExportSignalTrack<'a>,
}

/// Exact fixed-step character output paired with its caller-selected export identity.
pub struct ExportedFrame<'a> {
    selected_start_timestamp_unix_ms: i64,
    settings_fingerprint: [u8; 32],
    font_identity: [u8; 32],
    step_index: u64,
    signal_track: ExportSignalTrack<'a>,
    runtime: SceneRuntime<'static>,
}

impl<'a> ExportedFrame<'a> {
    #[must_use]
    pub const fn parameters(&self) -> SceneParameters {
        self.runtime.parameters()
    }

    #[must_use]
    pub const fn seed_le_bytes(&self) -> [u8; 4] {
        self.runtime.parameters().seed().to_le_bytes()
    }

    #[must_use]
    pub const fn selected_start_timestamp_unix_ms(&self) -> i64 {
        self.selected_start_timestamp_unix_ms
    }

    #[must_use]
    pub const fn settings_fingerprint(&self) -> [u8; 32] {
        self.settings_fingerprint
    }

    #[must_use]
    pub const fn font_identity(&self) -> [u8; 32] {
        self.font_identity
    }

    #[must_use]
    pub const fn step_index(&self) -> u64 {
        self.step_index
    }

    #[must_use]
    pub const fn signal_track(&self) -> ExportSignalTrack<'a> {
        self.signal_track
    }

    #[must_use]
    pub fn cells(&self) -> &[char] {
        self.runtime.frame()
    }

    #[must_use]
    pub const fn resource_estimate(&self) -> ResourceEstimate {
        self.runtime.resource_estimate()
    }
}

impl<'a> ExportContext<'a> {
    /// Requires a selected timestamp and fixed identities; no clock or asset lookup is performed.
    pub fn new(
        parameters: SceneParameters,
        selected_start_timestamp_unix_ms: i64,
        settings_fingerprint: [u8; 32],
        font_identity: [u8; 32],
        signal_track: ExportSignalTrack<'a>,
    ) -> Result<Self, Error> {
        if let ExportSignalTrack::Recorded(events) = signal_track {
            if events.len() > MAX_RECORDED_EVENTS {
                return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
            }
            if events
                .windows(2)
                .any(|pair| pair[0].step_index > pair[1].step_index)
            {
                return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
            }
        }
        Ok(Self {
            parameters,
            selected_start_timestamp_unix_ms,
            settings_fingerprint,
            font_identity,
            signal_track,
        })
    }

    #[must_use]
    pub const fn parameters(self) -> SceneParameters {
        self.parameters
    }

    #[must_use]
    pub const fn seed_le_bytes(self) -> [u8; 4] {
        self.parameters.seed().to_le_bytes()
    }

    #[must_use]
    pub const fn selected_start_timestamp_unix_ms(self) -> i64 {
        self.selected_start_timestamp_unix_ms
    }

    #[must_use]
    pub const fn simulation_hz(self) -> u16 {
        self.parameters.simulation_hz()
    }

    #[must_use]
    pub const fn settings_fingerprint(self) -> [u8; 32] {
        self.settings_fingerprint
    }

    #[must_use]
    pub const fn font_identity(self) -> [u8; 32] {
        self.font_identity
    }

    #[must_use]
    pub const fn signal_track(self) -> ExportSignalTrack<'a> {
        self.signal_track
    }
    /// Executes the exact requested fixed-step frame without clocks or live snapshots.
    ///
    /// The built-in descriptor has no supported signals and no event replay. An explicit idle
    /// fallback or an empty recorded track is accepted; nonempty tracks are rejected.
    pub fn execute_frame(
        self,
        dimensions: SceneDimensions,
        step_index: u64,
        budget: &SceneResourceBudget,
    ) -> Result<ExportedFrame<'a>, Error> {
        validate_signal_track(self.signal_track)?;

        let simulation_steps = if self.parameters.is_static() {
            0
        } else {
            step_index
        };
        let particle_count = u64::from(self.parameters.max_particles());
        let render_passes = if simulation_steps == 0 { 1_u64 } else { 2 };
        let particle_passes = render_passes.checked_add(1).ok_or(Error::SizeOverflow)?;
        let render_work = u64::try_from(dimensions.cell_count())
            .map_err(|_| Error::SizeOverflow)?
            .checked_mul(render_passes)
            .ok_or(Error::SizeOverflow)?;
        let particle_work = particle_count
            .checked_mul(particle_passes)
            .ok_or(Error::SizeOverflow)?;
        let step_work = simulation_steps
            .checked_mul(particle_count.max(1))
            .ok_or(Error::SizeOverflow)?;
        let work_units = render_work
            .checked_add(particle_work)
            .and_then(|work| work.checked_add(step_work))
            .ok_or(Error::SizeOverflow)?;
        if work_units > MAX_SIMULATION_WORK_UNITS {
            return Err(Error::BudgetExceeded(BudgetKind::WorkUnits));
        }

        let mut runtime = SceneRuntime::try_new(
            self.parameters,
            dimensions,
            ParticleFieldGenerator::default(),
            budget,
        )?;
        runtime.advance_exact_steps(step_index)?;
        Ok(ExportedFrame {
            selected_start_timestamp_unix_ms: self.selected_start_timestamp_unix_ms,
            settings_fingerprint: self.settings_fingerprint,
            font_identity: self.font_identity,
            step_index,
            signal_track: self.signal_track,
            runtime,
        })
    }
}

fn validate_signal_track(track: ExportSignalTrack<'_>) -> Result<(), Error> {
    let ExportSignalTrack::Recorded(events) = track else {
        return Ok(());
    };
    for event in events {
        if !GENERATOR_DESCRIPTOR
            .supported_signals
            .contains(&event.kind.signal_kind())
        {
            return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
        }
    }
    // A descriptor declaration alone does not imply this executor can replay that signal yet.
    if !events.is_empty() {
        return Err(Error::Malformed(MalformedReason::InvalidConfiguration));
    }
    Ok(())
}
