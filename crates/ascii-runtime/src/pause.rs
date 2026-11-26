//! Portable pause directives and bounded resume-clock primitives.
//!
//! This module consumes [`crate::AssignmentSnapshot`] and [`crate::Blockers`]; it does not own a
//! second assignment controller or subscribe to operating-system events. A native/event consumer
//! must reprobe topology and capabilities before calling [`ResumeClock::resume_after_reprobe`].

use core::{fmt, num::NonZeroU64};

use crate::{
    AssignmentSnapshot, AssignmentState, Blocker, ReasonCode, StopKind,
};

/// Whether a suspension keeps the surface attached or releases it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceDisposition {
    /// Keep the current surface and its approved frame attached.
    Retain,
    /// Release the surface; the consumer must attach a fresh one after reprobe.
    Release,
}

/// Resource-retention policy applied while the assignment is suspended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuspendResourcePolicy {
    /// Retain expensive resources for a faster resume when the host policy allows it.
    Retain,
    /// Release expensive resources while suspended.
    ReleaseExpensive,
}

/// Caller-owned native/resource policy for a portable suspended directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuspendPolicy {
    /// Whether expensive resources remain retained.
    pub resources: SuspendResourcePolicy,
    /// Whether the native surface remains attached.
    pub surface: SurfaceDisposition,
}

/// Whether a host may use a configured cosmetic transition for a state change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionPolicy {
    /// The host may apply its configured non-security transition.
    AllowConfigured,
    /// Skip cosmetic transitions and apply the state change immediately.
    Bypass,
}

/// Action derived from the existing transactional assignment snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PauseAction {
    /// There is no current assignment to present.
    Inactive,
    /// Advance simulation and render normally.
    Continue,
    /// Freeze simulation and retain the assignment and its approved frame.
    FreezeSimulation,
    /// Keep simulation semantics distinct from pause while limiting render work.
    ThrottleRender,
    /// Stop visual work; resource and surface release remain explicit host policy.
    Suspend {
        /// Resource policy selected by the caller.
        resources: SuspendResourcePolicy,
        /// Surface policy selected by the caller.
        surface: SurfaceDisposition,
        /// Lock, display-off, and unavailable-host transitions bypass animation.
        transition: TransitionPolicy,
    },
    /// Detach the assignment and end background work using the controller's stop path.
    Stop {
        /// Stop kind already authorized by `AssignmentController::begin_stop`.
        kind: StopKind,
    },
}

/// A snapshot cannot be translated into a pause action in its current lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PausePolicyError {
    /// `Stopping` must carry one of the controller's typed stop reason codes.
    MissingStopKind,
    /// Validation, preparation, attachment, and failure states are controlled by assignment flow.
    LifecycleStateNotControllable,
}

impl fmt::Display for PausePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingStopKind => formatter.write_str("stopping assignment has no stop kind"),
            Self::LifecycleStateNotControllable => {
                formatter.write_str("assignment lifecycle state is not pause-controllable")
            }
        }
    }
}

impl std::error::Error for PausePolicyError {}

/// Derives a portable presentation action without changing controller state.
///
/// Stop is resolved before blocker policy. The assignment controller remains the sole owner of
/// generation invalidation, detach acknowledgement, and active-resource release.
pub fn pause_action(
    snapshot: AssignmentSnapshot,
    suspend_policy: SuspendPolicy,
) -> Result<PauseAction, PausePolicyError> {
    match snapshot.state {
        AssignmentState::Unloaded | AssignmentState::Detached => Ok(PauseAction::Inactive),
        AssignmentState::Running => Ok(PauseAction::Continue),
        AssignmentState::Paused => Ok(PauseAction::FreezeSimulation),
        AssignmentState::Throttled => Ok(PauseAction::ThrottleRender),
        AssignmentState::Suspended => {
            let transition = if snapshot.blockers.contains(Blocker::SessionLocked)
                || snapshot.blockers.contains(Blocker::DisplayOff)
                || snapshot.blockers.contains(Blocker::HostUnavailable)
            {
                TransitionPolicy::Bypass
            } else {
                TransitionPolicy::AllowConfigured
            };
            Ok(PauseAction::Suspend {
                resources: suspend_policy.resources,
                surface: suspend_policy.surface,
                transition,
            })
        }
        AssignmentState::Stopping => {
            let kind = match snapshot.reason {
                Some(ReasonCode::UserStop) => StopKind::User,
                Some(ReasonCode::SecurityStop) => StopKind::Security,
                Some(ReasonCode::ForcedStop) => StopKind::Forced,
                _ => return Err(PausePolicyError::MissingStopKind),
            };
            Ok(PauseAction::Stop { kind })
        }
        AssignmentState::Validating
        | AssignmentState::Preparing
        | AssignmentState::Attaching
        | AssignmentState::Failed => Err(PausePolicyError::LifecycleStateNotControllable),
    }
}

/// Playback-position policy chosen by the source settings for resume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackPositionPolicy {
    /// Resume from the last approved playback position.
    RestoreSavedPosition,
    /// Align playback to the configured external or wall-clock source.
    RealignToClock,
}

/// Portable instructions returned after the consumer has reprobed its host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResumePlan {
    /// Source-selected playback-position behavior.
    pub position: PlaybackPositionPolicy,
    /// Fresh monotonic baseline established for the resumed session.
    pub monotonic_baseline_ns: u64,
    /// Events at or below this source sequence are stale and must be discarded.
    pub discard_external_events_through: u64,
}

/// A bounded monotonic clock with stale-event filtering for a resumed session.
///
/// Monotonic timestamps are nanoseconds in the caller's clock domain. External event sequence
/// numbers must increase monotonically for the lifetime of that source; source restarts must be
/// represented by a new caller-managed sequence domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResumeClock {
    baseline_ns: u64,
    max_catch_up_ns: NonZeroU64,
    event_watermark: u64,
}

impl ResumeClock {
    /// Creates a clock with its initial baseline, event watermark, and nonzero per-tick bound.
    #[must_use]
    pub const fn new(
        baseline_ns: u64,
        event_watermark: u64,
        max_catch_up_ns: NonZeroU64,
    ) -> Self {
        Self { baseline_ns, max_catch_up_ns, event_watermark }
    }

    /// Resets time and stale-event state after fresh topology/capability probes have completed.
    ///
    /// The returned plan preserves the source's restore-versus-realign choice. No operating-system
    /// probe or event subscription occurs here; the caller owns those operations and must invoke
    /// this method only after a successful fresh probe.
    pub fn resume_after_reprobe(
        &mut self,
        now_ns: u64,
        latest_external_event: u64,
        position: PlaybackPositionPolicy,
    ) -> ResumePlan {
        self.baseline_ns = now_ns;
        self.event_watermark = latest_external_event;
        ResumePlan {
            position,
            monotonic_baseline_ns: now_ns,
            discard_external_events_through: latest_external_event,
        }
    }

    /// Returns elapsed simulation time since the previous tick, clamped to the configured bound.
    ///
    /// The baseline advances to `now_ns` even when the elapsed interval is clamped, so the
    /// discarded excess is not replayed as repeated catch-up work. A regressing clock yields zero.
    pub fn tick_delta_ns(&mut self, now_ns: u64) -> u64 {
        let elapsed = now_ns.saturating_sub(self.baseline_ns);
        self.baseline_ns = now_ns;
        elapsed.min(self.max_catch_up_ns.get())
    }

    /// Accepts a strictly newer event and advances the watermark; stale or duplicate events drop.
    pub fn accept_external_event(&mut self, sequence: u64) -> bool {
        if sequence <= self.event_watermark {
            return false;
        }
        self.event_watermark = sequence;
        true
    }
}
