use ascii_core::{BudgetKind, Error, MalformedReason};

use crate::params::SceneParameters;
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

/// Explicit deterministic feed policy: replay normalized data or use the declared idle fallback.
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
}
