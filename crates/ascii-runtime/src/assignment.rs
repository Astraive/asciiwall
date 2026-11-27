//! Portable transactional assignment lifecycle primitives.
//!
//! This module owns state and resource transitions, not native presentation APIs. Hosts implement
//! [`AssignmentHost`] so validation and hidden attachment happen before the atomic switch. The host
//! contract requires `switch` to leave `previous` presented when it returns an error.

use ascii_core::{AssignmentId, Generation};

/// Public lifecycle states exposed to command and presentation consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentState {
    Unloaded,
    Validating,
    Preparing,
    Attaching,
    Running,
    Paused,
    Throttled,
    Suspended,
    Stopping,
    Detached,
    Failed,
}

/// Sanitized, stable reason codes. These values deliberately carry no caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    InvalidAssignment,
    PreparationFailed,
    AttachmentFailed,
    SwitchFailed,
    GenerationExhausted,
    UserPause,
    SessionLocked,
    DisplayOff,
    FullscreenPolicy,
    LowBattery,
    BatterySaver,
    ThermalPolicy,
    HostUnavailable,
    ExportContention,
    StopInProgress,
    UserStop,
    SecurityStop,
    ForcedStop,
}

/// Conditions blocking or reducing active presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blocker {
    UserPause,
    SessionLocked,
    DisplayOff,
    FullscreenPolicy,
    LowBattery,
    BatterySaver,
    ThermalPolicy,
    HostUnavailable,
    ExportContention,
}

/// Presentation policy passed only for an assignment replacement, not shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchTransition {
    /// A user-requested wallpaper switch may use configured transition behavior.
    UserRequested,
    /// Startup, recovery, and other non-user changes switch without cosmetic animation.
    Immediate,
}

/// Fixed-size set of blockers, suitable for snapshots without allocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Blockers(u16);

impl Blockers {
    const fn bit(blocker: Blocker) -> u16 {
        match blocker {
            Blocker::UserPause => 1 << 0,
            Blocker::SessionLocked => 1 << 1,
            Blocker::DisplayOff => 1 << 2,
            Blocker::FullscreenPolicy => 1 << 3,
            Blocker::LowBattery => 1 << 4,
            Blocker::BatterySaver => 1 << 5,
            Blocker::ThermalPolicy => 1 << 6,
            Blocker::HostUnavailable => 1 << 7,
            Blocker::ExportContention => 1 << 8,
        }
    }

    /// Adds one independent blocker; clearing another blocker never resumes presentation early.
    pub fn insert(&mut self, blocker: Blocker) {
        self.0 |= Self::bit(blocker);
    }

    /// Removes one blocker.
    pub fn remove(&mut self, blocker: Blocker) {
        self.0 &= !Self::bit(blocker);
    }

    /// Returns true when the blocker is present.
    #[must_use]
    pub const fn contains(self, blocker: Blocker) -> bool {
        self.0 & Self::bit(blocker) != 0
    }

    /// Returns true when any blocker remains.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

const fn state_for(blockers: Blockers) -> AssignmentState {
    if blockers.contains(Blocker::SessionLocked)
        || blockers.contains(Blocker::DisplayOff)
        || blockers.contains(Blocker::HostUnavailable)
    {
        AssignmentState::Suspended
    } else if blockers.contains(Blocker::ThermalPolicy)
        || blockers.contains(Blocker::LowBattery)
        || blockers.contains(Blocker::BatterySaver)
        || blockers.contains(Blocker::ExportContention)
    {
        AssignmentState::Throttled
    } else if !blockers.is_empty() {
        AssignmentState::Paused
    } else {
        AssignmentState::Running
    }
}

const fn reason_for(blockers: Blockers) -> Option<ReasonCode> {
    if blockers.contains(Blocker::SessionLocked) {
        Some(ReasonCode::SessionLocked)
    } else if blockers.contains(Blocker::DisplayOff) {
        Some(ReasonCode::DisplayOff)
    } else if blockers.contains(Blocker::HostUnavailable) {
        Some(ReasonCode::HostUnavailable)
    } else if blockers.contains(Blocker::ThermalPolicy) {
        Some(ReasonCode::ThermalPolicy)
    } else if blockers.contains(Blocker::LowBattery) {
        Some(ReasonCode::LowBattery)
    } else if blockers.contains(Blocker::BatterySaver) {
        Some(ReasonCode::BatterySaver)
    } else if blockers.contains(Blocker::ExportContention) {
        Some(ReasonCode::ExportContention)
    } else if blockers.contains(Blocker::FullscreenPolicy) {
        Some(ReasonCode::FullscreenPolicy)
    } else if blockers.contains(Blocker::UserPause) {
        Some(ReasonCode::UserPause)
    } else {
        None
    }
}
const fn hard_blocker_reason(blockers: Blockers) -> Option<ReasonCode> {
    if blockers.contains(Blocker::SessionLocked) {
        Some(ReasonCode::SessionLocked)
    } else if blockers.contains(Blocker::DisplayOff) {
        Some(ReasonCode::DisplayOff)
    } else if blockers.contains(Blocker::HostUnavailable) {
        Some(ReasonCode::HostUnavailable)
    } else {
        None
    }
}

/// Current observable state. No path, title, input history, or arbitrary error string is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentSnapshot {
    pub state: AssignmentState,
    pub generation: Generation,
    pub active_assignment: Option<AssignmentId>,
    pub blockers: Blockers,
    pub reason: Option<ReasonCode>,
}

/// Input carried to asynchronous preparation. Its payload belongs to the caller/host and is never
/// copied by this crate.
pub struct ApplyInput<I> {
    pub assignment: AssignmentId,
    pub transition: SwitchTransition,
    pub payload: I,
}

/// Work ticket returned after validation. Execute `prepare` away from the controller thread and
/// submit its generation-tagged result with [`AssignmentController::complete_preparation`].
pub struct PreparationJob<I> {
    generation: Generation,
    input: ApplyInput<I>,
}

impl<I> PreparationJob<I> {
    /// Generation captured when this preparation was requested.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Consumes the job in a background worker and returns an ownership-carrying completion.
    pub fn execute<H>(self, host: &mut H) -> PreparationCompletion<H::Prepared>
    where
        H: AssignmentHost<Input = I>,
    {
        let generation = self.generation;
        let result = host.prepare(self.input.payload);
        PreparationCompletion {
            generation,
            assignment: self.input.assignment,
            transition: self.input.transition,
            result,
        }
    }
}

/// Asynchronous preparation result. A stale successful result is dropped by the controller.
pub struct PreparationCompletion<P> {
    generation: Generation,
    assignment: AssignmentId,
    transition: SwitchTransition,
    result: Result<P, ReasonCode>,
}

impl<P> PreparationCompletion<P> {
    /// Generation captured by the originating job.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }
}

/// Portable host boundary for validation, preparation, hidden attachment, and the atomic switch.
///
/// `switch` must not change the visible previous assignment unless it returns `Ok`. Implementors
/// must keep all native/thread-affine operations inside their own host boundary.
pub trait AssignmentHost {
    type Input;
    type Prepared;
    type Attached;
    type Active;

    /// Performs bounded, non-heavy validation before a preparation job is issued.
    fn validate(&mut self, input: &Self::Input) -> Result<(), ReasonCode>;

    /// Heavy work, called only by `PreparationJob::execute` on the caller-selected worker.
    fn prepare(&mut self, input: Self::Input) -> Result<Self::Prepared, ReasonCode>;

    /// Attaches a prepared safe-first-frame surface in a hidden/ready state.
    fn attach_hidden(&mut self, prepared: Self::Prepared) -> Result<Self::Attached, ReasonCode>;

    /// Atomically makes `attached` active, honoring user-transition versus immediate policy.
    /// On error, the previous active assignment stays visible.
    fn switch(
        &mut self,
        previous: Option<&Self::Active>,
        attached: Self::Attached,
        transition: SwitchTransition,
    ) -> Result<Self::Active, ReasonCode>;
}

/// Result of submitting a completion to the serialized controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionOutcome {
    Committed,
    Failed(ReasonCode),
    /// Successful stale output was discarded and its owned resources released.
    Stale,
}

/// Stop path. Security and forced paths are distinct from a user-requested transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopKind {
    User,
    Security,
    Forced,
}

/// Token proving which stop request is being acknowledged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopToken {
    generation: Generation,
    kind: StopKind,
}

impl StopToken {
    /// Stop kind for this request.
    #[must_use]
    pub const fn kind(self) -> StopKind {
        self.kind
    }
}

/// Single-owner lifecycle controller. All state transitions require exclusive mutable access.
pub struct AssignmentController<A> {
    generation: Generation,
    state: AssignmentState,
    active_assignment: Option<AssignmentId>,
    active: Option<A>,
    blockers: Blockers,
    reason: Option<ReasonCode>,
}

impl<A> Default for AssignmentController<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> AssignmentController<A> {
    /// Creates an unloaded controller.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            generation: Generation::INITIAL,
            state: AssignmentState::Unloaded,
            active_assignment: None,
            active: None,
            blockers: Blockers(0),
            reason: None,
        }
    }

    /// Copies only the bounded public state needed by UI or CLI consumers.
    #[must_use]
    pub const fn snapshot(&self) -> AssignmentSnapshot {
        AssignmentSnapshot {
            state: self.state,
            generation: self.generation,
            active_assignment: self.active_assignment,
            blockers: self.blockers,
            reason: self.reason,
        }
    }

    /// Returns a reference to the currently owned host resource, if any.
    #[must_use]
    pub fn active_resource(&self) -> Option<&A> {
        self.active.as_ref()
    }

    /// Invalidates older work, validates input, and returns a generation-tagged preparation job.
    /// A failed validation leaves the existing active assignment and its resource untouched.
    pub fn request<H>(
        &mut self,
        input: ApplyInput<H::Input>,
        host: &mut H,
    ) -> Result<PreparationJob<H::Input>, ReasonCode>
    where
        H: AssignmentHost<Active = A>,
    {
        if self.state == AssignmentState::Stopping {
            return Err(ReasonCode::StopInProgress);
        }
        if let Some(reason) = hard_blocker_reason(self.blockers) {
            return Err(reason);
        }
        let Some(generation) = self.generation.checked_next() else {
            self.state = AssignmentState::Failed;
            self.reason = Some(ReasonCode::GenerationExhausted);
            return Err(ReasonCode::GenerationExhausted);
        };
        self.generation = generation;
        self.state = AssignmentState::Validating;
        self.reason = None;
        if let Err(reason) = host.validate(&input.payload) {
            self.state = AssignmentState::Failed;
            self.reason = Some(reason);
            return Err(reason);
        }
        self.state = AssignmentState::Preparing;
        Ok(PreparationJob { generation, input })
    }

    /// Completes preparation, attaches hidden-ready, then atomically commits if still current.
    /// Any precommit failure preserves the prior active resource and assignment identity.
    pub fn complete_preparation<H>(
        &mut self,
        completion: PreparationCompletion<H::Prepared>,
        host: &mut H,
    ) -> CompletionOutcome
    where
        H: AssignmentHost<Active = A>,
    {
        if completion.generation != self.generation || self.state != AssignmentState::Preparing {
            return CompletionOutcome::Stale;
        }
        if let Some(reason) = hard_blocker_reason(self.blockers) {
            self.generation = self.generation.checked_next().unwrap_or(self.generation);
            if self.active.is_some() {
                self.state = AssignmentState::Suspended;
                self.reason = Some(reason);
            } else {
                self.state = AssignmentState::Unloaded;
                self.reason = None;
            }
            return CompletionOutcome::Stale;
        }
        let prepared = match completion.result {
            Ok(prepared) => prepared,
            Err(reason) => {
                self.state = AssignmentState::Failed;
                self.reason = Some(reason);
                return CompletionOutcome::Failed(reason);
            }
        };
        self.state = AssignmentState::Attaching;
        let attached = match host.attach_hidden(prepared) {
            Ok(attached) => attached,
            Err(reason) => {
                self.state = AssignmentState::Failed;
                self.reason = Some(reason);
                return CompletionOutcome::Failed(reason);
            }
        };
        if let Some(reason) = hard_blocker_reason(self.blockers) {
            self.state = if self.active.is_some() {
                state_for(self.blockers)
            } else {
                AssignmentState::Unloaded
            };
            self.reason = Some(reason);
            return CompletionOutcome::Failed(reason);
        }

        match host.switch(self.active.as_ref(), attached, completion.transition) {
            Ok(active) => {
                self.active = Some(active);
                self.active_assignment = Some(completion.assignment);
                self.state = state_for(self.blockers);
                self.reason = reason_for(self.blockers);
                CompletionOutcome::Committed
            }
            Err(reason) => {
                self.state = AssignmentState::Failed;
                self.reason = Some(reason);
                CompletionOutcome::Failed(reason)
            }
        }
    }

    /// Adds/removes runtime blockers, immediately invalidating hard-blocked preparation while
    /// preserving any active assignment and keeping unloaded blocker reasons observable.
    pub fn set_blocker(&mut self, blocker: Blocker, present: bool) {
        if matches!(
            self.state,
            AssignmentState::Stopping | AssignmentState::Detached
        ) {
            return;
        }
        if present {
            self.blockers.insert(blocker);
        } else {
            self.blockers.remove(blocker);
        }
        let hard_reason = hard_blocker_reason(self.blockers);
        if self.active.is_none() {
            if self.state == AssignmentState::Preparing && hard_reason.is_some() {
                self.generation = self.generation.checked_next().unwrap_or(self.generation);
                self.state = AssignmentState::Unloaded;
            }
            if self.state == AssignmentState::Unloaded {
                self.reason = reason_for(self.blockers);
            }
            return;
        }
        match self.state {
            AssignmentState::Preparing => {
                if let Some(reason) = hard_reason {
                    self.generation = self.generation.checked_next().unwrap_or(self.generation);
                    self.state = AssignmentState::Suspended;
                    self.reason = Some(reason);
                }
            }
            AssignmentState::Failed => {
                if let Some(reason) = hard_reason {
                    self.state = AssignmentState::Suspended;
                    self.reason = Some(reason);
                }
            }
            AssignmentState::Running
            | AssignmentState::Paused
            | AssignmentState::Throttled
            | AssignmentState::Suspended => {
                self.state = state_for(self.blockers);
                self.reason = reason_for(self.blockers);
            }
            AssignmentState::Unloaded
            | AssignmentState::Validating
            | AssignmentState::Attaching
            | AssignmentState::Stopping
            | AssignmentState::Detached => {}
        }
    }

    /// Begins an explicit stop, invalidating all outstanding preparations immediately.
    pub fn begin_stop(&mut self, kind: StopKind) -> StopToken {
        self.generation = self.generation.checked_next().unwrap_or(self.generation);
        self.state = AssignmentState::Stopping;
        self.blockers = Blockers::default();
        self.reason = Some(match kind {
            StopKind::User => ReasonCode::UserStop,
            StopKind::Security => ReasonCode::SecurityStop,
            StopKind::Forced => ReasonCode::ForcedStop,
        });
        StopToken {
            generation: self.generation,
            kind,
        }
    }

    /// Completes detach only for the current stop token; owned active resources are then released.
    pub fn complete_stop(&mut self, token: StopToken) -> bool {
        if token.generation != self.generation || self.state != AssignmentState::Stopping {
            return false;
        }
        self.active = None;
        self.active_assignment = None;
        self.state = AssignmentState::Detached;
        true
    }

    /// Returns a fully detached controller to its initial empty state.
    pub fn reset_unloaded(&mut self) -> bool {
        if self.state != AssignmentState::Detached || self.active.is_some() {
            return false;
        }
        self.state = AssignmentState::Unloaded;
        self.reason = None;
        self.blockers = Blockers::default();
        true
    }
}
