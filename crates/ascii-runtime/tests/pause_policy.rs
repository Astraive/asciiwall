use std::num::NonZeroU64;

use ascii_runtime::{
    AssignmentController, AssignmentSnapshot, AssignmentState, Blocker, Blockers, PauseAction,
    PausePolicyError, PlaybackPositionPolicy, ReasonCode, ResumeClock, StopKind,
    SurfaceDisposition, SuspendPolicy, SuspendResourcePolicy, TransitionPolicy, pause_action,
};

const PAUSE_REASONS: [Blocker; 9] = [
    Blocker::UserPause,
    Blocker::SessionLocked,
    Blocker::DisplayOff,
    Blocker::FullscreenPolicy,
    Blocker::LowBattery,
    Blocker::BatterySaver,
    Blocker::ThermalPolicy,
    Blocker::HostUnavailable,
    Blocker::ExportContention,
];

fn blockers(reasons: &[Blocker]) -> Blockers {
    let mut blockers = Blockers::default();
    for reason in reasons {
        blockers.insert(*reason);
    }
    blockers
}

fn snapshot(
    state: AssignmentState,
    blockers: Blockers,
    reason: Option<ReasonCode>,
) -> AssignmentSnapshot {
    let mut snapshot = AssignmentController::<()>::new().snapshot();
    snapshot.state = state;
    snapshot.blockers = blockers;
    snapshot.reason = reason;
    snapshot
}

fn suspend_policy() -> SuspendPolicy {
    SuspendPolicy {
        resources: SuspendResourcePolicy::ReleaseExpensive,
        surface: SurfaceDisposition::Release,
    }
}

#[test]
fn clearing_user_pause_preserves_each_other_reason() {
    let mut reasons = blockers(&PAUSE_REASONS);
    reasons.remove(Blocker::UserPause);

    assert!(!reasons.contains(Blocker::UserPause));
    for reason in PAUSE_REASONS
        .into_iter()
        .filter(|reason| *reason != Blocker::UserPause)
    {
        assert!(
            reasons.contains(reason),
            "clearing user pause also cleared {reason:?}"
        );
    }
    assert!(!reasons.is_empty());
}

#[test]
fn stop_action_has_priority_over_any_remaining_pause_reason() {
    let stop_snapshot = snapshot(
        AssignmentState::Stopping,
        blockers(&PAUSE_REASONS),
        Some(ReasonCode::UserStop),
    );

    assert_eq!(
        pause_action(stop_snapshot, suspend_policy()),
        Ok(PauseAction::Stop {
            kind: StopKind::User
        }),
    );
    assert_eq!(
        pause_action(
            snapshot(
                AssignmentState::Stopping,
                Blockers::default(),
                Some(ReasonCode::UserPause)
            ),
            suspend_policy(),
        ),
        Err(PausePolicyError::MissingStopKind),
    );
}

#[test]
fn lock_and_display_off_suspend_without_cosmetic_transition() {
    for (reason, blocker) in [
        (ReasonCode::SessionLocked, Blocker::SessionLocked),
        (ReasonCode::DisplayOff, Blocker::DisplayOff),
    ] {
        let snapshot = snapshot(
            AssignmentState::Suspended,
            blockers(&[blocker]),
            Some(reason),
        );
        assert_eq!(
            pause_action(snapshot, suspend_policy()),
            Ok(PauseAction::Suspend {
                resources: SuspendResourcePolicy::ReleaseExpensive,
                surface: SurfaceDisposition::Release,
                transition: TransitionPolicy::Bypass,
            }),
        );
    }
}

#[test]
fn user_pause_freezes_frame_but_suspend_applies_resource_policy() {
    let paused = snapshot(
        AssignmentState::Paused,
        blockers(&[Blocker::UserPause]),
        Some(ReasonCode::UserPause),
    );
    assert_eq!(
        pause_action(paused, suspend_policy()),
        Ok(PauseAction::FreezeSimulation),
    );

    let suspended = snapshot(
        AssignmentState::Suspended,
        blockers(&[Blocker::HostUnavailable]),
        Some(ReasonCode::HostUnavailable),
    );
    assert_eq!(
        pause_action(
            suspended,
            SuspendPolicy {
                resources: SuspendResourcePolicy::ReleaseExpensive,
                surface: SurfaceDisposition::Retain,
            }
        ),
        Ok(PauseAction::Suspend {
            resources: SuspendResourcePolicy::ReleaseExpensive,
            surface: SurfaceDisposition::Retain,
            transition: TransitionPolicy::Bypass,
        }),
    );

    let throttled = snapshot(
        AssignmentState::Throttled,
        blockers(&[Blocker::BatterySaver]),
        Some(ReasonCode::BatterySaver),
    );
    assert_eq!(
        pause_action(throttled, suspend_policy()),
        Ok(PauseAction::ThrottleRender),
    );
}

#[derive(Clone, Copy)]
struct FakeClock {
    now_ns: u64,
}

impl FakeClock {
    fn advance(&mut self, delta_ns: u64) {
        self.now_ns += delta_ns;
    }
}

#[derive(Clone, Copy)]
struct FakeEvents {
    latest_sequence: u64,
}

impl FakeEvents {
    fn emit(&mut self) -> u64 {
        self.latest_sequence += 1;
        self.latest_sequence
    }
}

#[test]
fn resume_resets_baseline_drops_stale_events_and_clamps_catch_up() {
    let mut fake_clock = FakeClock { now_ns: 100 };
    let mut fake_events = FakeEvents { latest_sequence: 7 };
    let mut clock = ResumeClock::new(
        fake_clock.now_ns,
        fake_events.latest_sequence,
        NonZeroU64::new(25).unwrap(),
    );

    fake_clock.advance(10);
    assert_eq!(clock.tick_delta_ns(fake_clock.now_ns), 10);

    fake_clock.advance(86_400_000_000_000);
    let plan = clock.resume_after_reprobe(
        fake_clock.now_ns,
        fake_events.latest_sequence,
        PlaybackPositionPolicy::RealignToClock,
    );
    assert_eq!(plan.monotonic_baseline_ns, fake_clock.now_ns);
    assert_eq!(plan.discard_external_events_through, 7);
    assert_eq!(plan.position, PlaybackPositionPolicy::RealignToClock);
    assert_eq!(clock.tick_delta_ns(fake_clock.now_ns), 0);

    assert!(!clock.accept_external_event(6));
    assert!(!clock.accept_external_event(7));
    let fresh_event = fake_events.emit();
    assert!(clock.accept_external_event(fresh_event));
    assert!(!clock.accept_external_event(fresh_event));

    fake_clock.advance(1_000);
    assert_eq!(clock.tick_delta_ns(fake_clock.now_ns), 25);
    fake_clock.advance(10);
    assert_eq!(clock.tick_delta_ns(fake_clock.now_ns), 10);
}

#[test]
fn resume_preserves_restore_or_realign_source_policy() {
    let mut clock = ResumeClock::new(10, 0, NonZeroU64::new(1).unwrap());
    let restore = clock.resume_after_reprobe(20, 5, PlaybackPositionPolicy::RestoreSavedPosition);
    assert_eq!(
        restore.position,
        PlaybackPositionPolicy::RestoreSavedPosition
    );

    let realign = clock.resume_after_reprobe(30, 8, PlaybackPositionPolicy::RealignToClock);
    assert_eq!(realign.position, PlaybackPositionPolicy::RealignToClock);
    assert_eq!(realign.monotonic_baseline_ns, 30);
    assert_eq!(realign.discard_external_events_through, 8);
}

#[test]
fn non_pause_lifecycle_states_are_not_misreported_as_running() {
    let snapshot = snapshot(AssignmentState::Preparing, Blockers::default(), None);
    assert_eq!(
        pause_action(snapshot, suspend_policy()),
        Err(PausePolicyError::LifecycleStateNotControllable),
    );
}
