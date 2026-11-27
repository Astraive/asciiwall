use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use ascii_core::{AssignmentId, Generation};
use ascii_runtime::{
    ApplyInput, AssignmentController, AssignmentHost, AssignmentState, Blocker, CompletionOutcome,
    PauseAction, ReasonCode, StopKind, SurfaceDisposition, SuspendPolicy, SuspendResourcePolicy,
    SwitchTransition, TransitionPolicy, pause_action,
};

fn assignment(value: u8) -> AssignmentId {
    AssignmentId::new([value; 16]).expect("nonzero assignment id")
}

#[derive(Clone)]
struct Drops(Arc<AtomicUsize>);

impl Drops {
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for Drops {
    fn default() -> Self {
        Self(Arc::new(AtomicUsize::new(0)))
    }
}

struct Owned(Drops);

impl Drop for Owned {
    fn drop(&mut self) {
        self.0.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Prepared {
    id: u8,
    resource: Owned,
}

struct Attached {
    id: u8,
    resource: Owned,
}

struct Active {
    id: u8,
    transition: SwitchTransition,
    _resource: Owned,
}

#[derive(Clone, Copy)]
struct Input {
    id: u8,
    validation_fails: bool,
}

#[derive(Default)]
struct Host {
    drops: Drops,
    prepare_fails: bool,
    attach_fails: bool,
    switch_fails: bool,
    switch_count: usize,
}

impl AssignmentHost for Host {
    type Input = Input;
    type Prepared = Prepared;
    type Attached = Attached;
    type Active = Active;

    fn validate(&mut self, input: &Input) -> Result<(), ReasonCode> {
        if input.validation_fails {
            Err(ReasonCode::InvalidAssignment)
        } else {
            Ok(())
        }
    }

    fn prepare(&mut self, input: Input) -> Result<Prepared, ReasonCode> {
        if self.prepare_fails {
            Err(ReasonCode::PreparationFailed)
        } else {
            Ok(Prepared {
                id: input.id,
                resource: Owned(self.drops.clone()),
            })
        }
    }

    fn attach_hidden(&mut self, prepared: Prepared) -> Result<Attached, ReasonCode> {
        if self.attach_fails {
            Err(ReasonCode::AttachmentFailed)
        } else {
            Ok(Attached {
                id: prepared.id,
                resource: prepared.resource,
            })
        }
    }

    fn switch(
        &mut self,
        _previous: Option<&Active>,
        attached: Attached,
        transition: SwitchTransition,
    ) -> Result<Active, ReasonCode> {
        self.switch_count += 1;
        if self.switch_fails {
            Err(ReasonCode::SwitchFailed)
        } else {
            Ok(Active {
                id: attached.id,
                transition,
                _resource: attached.resource,
            })
        }
    }
}

fn input(id: u8) -> ApplyInput<Input> {
    input_with_transition(id, SwitchTransition::UserRequested)
}

fn input_with_transition(id: u8, transition: SwitchTransition) -> ApplyInput<Input> {
    ApplyInput {
        assignment: assignment(id),
        transition,
        payload: Input {
            id,
            validation_fails: false,
        },
    }
}

fn install_initial(controller: &mut AssignmentController<Active>, host: &mut Host) {
    let job = controller.request(input(1), host).unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(host), host),
        CompletionOutcome::Committed
    );
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
}

fn suspend_policy() -> SuspendPolicy {
    SuspendPolicy {
        resources: SuspendResourcePolicy::ReleaseExpensive,
        surface: SurfaceDisposition::Release,
    }
}

fn immediate_suspend_action() -> PauseAction {
    PauseAction::Suspend {
        resources: SuspendResourcePolicy::ReleaseExpensive,
        surface: SurfaceDisposition::Release,
        transition: TransitionPolicy::Bypass,
    }
}

#[test]
fn only_newest_generation_commits_and_stale_preparation_releases_its_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let old = controller.request(input(2), &mut host).unwrap();
    let new = controller.request(input(3), &mut host).unwrap();
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));

    let stale_completion = old.execute(&mut host);
    let current_completion = new.execute(&mut host);
    assert_eq!(
        controller.complete_preparation(stale_completion, &mut host),
        CompletionOutcome::Stale
    );
    assert_eq!(host.drops.count(), 1);
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.complete_preparation(current_completion, &mut host),
        CompletionOutcome::Committed
    );
    assert_eq!(controller.snapshot().generation, Generation::from_value(3));
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(3)));
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
    assert_eq!(
        controller
            .active_resource()
            .map(|resource| resource.transition),
        Some(SwitchTransition::UserRequested)
    );
}

#[test]
fn immediate_replacement_is_distinct_from_user_switch_transition() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let job = controller
        .request(
            input_with_transition(2, SwitchTransition::Immediate),
            &mut host,
        )
        .unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Committed
    );
    assert_eq!(
        controller
            .active_resource()
            .map(|resource| resource.transition),
        Some(SwitchTransition::Immediate)
    );
}

#[test]
fn validation_failure_keeps_the_current_assignment_and_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let before = host.drops.count();
    let result = controller.request(
        ApplyInput {
            assignment: assignment(4),
            transition: SwitchTransition::UserRequested,
            payload: Input {
                id: 4,
                validation_fails: true,
            },
        },
        &mut host,
    );
    assert!(matches!(result, Err(ReasonCode::InvalidAssignment)));
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(controller.snapshot().state, AssignmentState::Failed);
    assert_eq!(host.drops.count(), before);
}

#[test]
fn preparation_failure_keeps_the_current_assignment_and_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let before = host.drops.count();
    host.prepare_fails = true;
    let job = controller.request(input(5), &mut host).unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Failed(ReasonCode::PreparationFailed)
    );
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(controller.snapshot().state, AssignmentState::Failed);
    assert_eq!(host.drops.count(), before);
}

#[test]
fn attachment_failure_keeps_the_current_assignment_and_releases_prepared_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let before = host.drops.count();
    host.attach_fails = true;
    let job = controller.request(input(6), &mut host).unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Failed(ReasonCode::AttachmentFailed)
    );
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(controller.snapshot().state, AssignmentState::Failed);
    assert_eq!(host.drops.count(), before + 1);
}

#[test]
fn switch_failure_keeps_the_previous_assignment_and_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let before = host.drops.count();
    host.switch_fails = true;
    let job = controller.request(input(7), &mut host).unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Failed(ReasonCode::SwitchFailed)
    );
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(controller.snapshot().state, AssignmentState::Failed);
    assert_eq!(host.drops.count(), before + 1);
}

#[test]
fn forced_stop_invalidates_pending_work_and_releases_active_resource() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let pending = controller
        .request(input(8), &mut host)
        .unwrap()
        .execute(&mut host);
    let before = host.drops.count();
    let token = controller.begin_stop(StopKind::Forced);
    assert_eq!(token.kind(), StopKind::Forced);
    assert_eq!(controller.snapshot().state, AssignmentState::Stopping);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::ForcedStop));
    assert_eq!(
        controller.complete_preparation(pending, &mut host),
        CompletionOutcome::Stale
    );
    assert_eq!(host.drops.count(), before + 1);
    assert!(controller.complete_stop(token));
    assert_eq!(controller.snapshot().state, AssignmentState::Detached);
    assert_eq!(controller.snapshot().active_assignment, None);
    assert_eq!(host.drops.count(), before + 2);
    assert!(controller.reset_unloaded());
    assert_eq!(controller.snapshot().state, AssignmentState::Unloaded);
}

#[test]
fn hard_blocker_during_initial_preparation_invalidates_and_reports_blocker_reason() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    let job = controller.request(input(11), &mut host).unwrap();
    let completion = job.execute(&mut host);
    let generation = controller.snapshot().generation;
    let drops_before_completion = host.drops.count();

    controller.set_blocker(Blocker::DisplayOff, true);
    assert_ne!(controller.snapshot().generation, generation);
    assert_eq!(controller.snapshot().state, AssignmentState::Unloaded);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::DisplayOff));
    assert_eq!(
        controller.complete_preparation(completion, &mut host),
        CompletionOutcome::Stale
    );
    assert_eq!(host.switch_count, 0);
    assert_eq!(host.drops.count(), drops_before_completion + 1);
    assert_eq!(controller.snapshot().active_assignment, None);
    assert!(controller.active_resource().is_none());

    controller.set_blocker(Blocker::SessionLocked, true);
    assert_eq!(
        controller.snapshot().reason,
        Some(ReasonCode::SessionLocked)
    );
    controller.set_blocker(Blocker::SessionLocked, false);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::DisplayOff));
    controller.set_blocker(Blocker::DisplayOff, false);
    assert_eq!(controller.snapshot().reason, None);
}

#[test]
fn soft_user_pause_during_preparation_does_not_cancel_replacement() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let job = controller.request(input(9), &mut host).unwrap();
    controller.set_blocker(Blocker::UserPause, true);
    assert_eq!(controller.snapshot().state, AssignmentState::Preparing);
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Committed
    );
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(9)));
    assert_eq!(controller.snapshot().state, AssignmentState::Paused);
    assert!(controller.snapshot().blockers.contains(Blocker::UserPause));
    controller.set_blocker(Blocker::UserPause, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
}

#[test]
fn lock_and_display_off_during_preparation_suspend_old_assignment_and_invalidate_job() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let job = controller.request(input(8), &mut host).unwrap();
    let completion = job.execute(&mut host);

    let mut event_snapshot = controller.snapshot();
    event_snapshot.blockers.insert(Blocker::SessionLocked);
    assert_eq!(
        pause_action(event_snapshot, suspend_policy()),
        Ok(immediate_suspend_action())
    );

    let preparing_generation = controller.snapshot().generation;
    controller.set_blocker(Blocker::SessionLocked, true);
    controller.set_blocker(Blocker::DisplayOff, true);
    let blocked = controller.snapshot();
    assert_ne!(blocked.generation, preparing_generation);
    assert_eq!(blocked.state, AssignmentState::Suspended);
    assert_eq!(blocked.reason, Some(ReasonCode::SessionLocked));
    assert_eq!(blocked.active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(
        pause_action(blocked, suspend_policy()),
        Ok(immediate_suspend_action())
    );
    assert_eq!(
        controller.complete_preparation(completion, &mut host),
        CompletionOutcome::Stale
    );
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(host.drops.count(), 1);
    assert_eq!(
        controller.request(input(10), &mut host).map(|_| ()),
        Err(ReasonCode::SessionLocked)
    );

    controller.set_blocker(Blocker::SessionLocked, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Suspended);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::DisplayOff));
    controller.set_blocker(Blocker::DisplayOff, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );

    let token = controller.begin_stop(StopKind::User);
    assert_eq!(
        pause_action(controller.snapshot(), suspend_policy()),
        Ok(PauseAction::Stop {
            kind: StopKind::User
        })
    );
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert!(controller.complete_stop(token));
    assert_eq!(controller.snapshot().state, AssignmentState::Detached);
    assert_eq!(controller.snapshot().active_assignment, None);
    assert_eq!(host.drops.count(), 2);
}

#[test]
fn host_unavailable_during_preparation_suspends_old_assignment_and_invalidates_job() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    let job = controller.request(input(6), &mut host).unwrap();
    let completion = job.execute(&mut host);

    let preparing_generation = controller.snapshot().generation;
    controller.set_blocker(Blocker::HostUnavailable, true);
    let blocked = controller.snapshot();
    assert_ne!(blocked.generation, preparing_generation);
    assert_eq!(blocked.state, AssignmentState::Suspended);
    assert_eq!(blocked.reason, Some(ReasonCode::HostUnavailable));
    assert_eq!(blocked.active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(
        pause_action(blocked, suspend_policy()),
        Ok(immediate_suspend_action())
    );
    assert_eq!(
        controller.complete_preparation(completion, &mut host),
        CompletionOutcome::Stale
    );
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(host.drops.count(), 1);
    assert_eq!(
        controller.request(input(12), &mut host).map(|_| ()),
        Err(ReasonCode::HostUnavailable)
    );

    controller.set_blocker(Blocker::HostUnavailable, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
}
#[test]
fn display_off_during_failed_precommit_suspends_and_then_resumes_old_assignment() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    host.prepare_fails = true;

    let job = controller.request(input(7), &mut host).unwrap();
    assert_eq!(
        controller.complete_preparation(job.execute(&mut host), &mut host),
        CompletionOutcome::Failed(ReasonCode::PreparationFailed)
    );
    assert_eq!(controller.snapshot().state, AssignmentState::Failed);
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(host.drops.count(), 0);

    let mut event_snapshot = controller.snapshot();
    event_snapshot.blockers.insert(Blocker::DisplayOff);
    assert_eq!(
        pause_action(event_snapshot, suspend_policy()),
        Ok(immediate_suspend_action())
    );

    controller.set_blocker(Blocker::DisplayOff, true);
    assert_eq!(controller.snapshot().state, AssignmentState::Suspended);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::DisplayOff));
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );
    assert_eq!(
        controller.request(input(11), &mut host).map(|_| ()),
        Err(ReasonCode::DisplayOff)
    );

    controller.set_blocker(Blocker::DisplayOff, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
    assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
    assert_eq!(
        controller.active_resource().map(|resource| resource.id),
        Some(1)
    );

    let token = controller.begin_stop(StopKind::Security);
    assert!(controller.complete_stop(token));
    assert_eq!(controller.snapshot().state, AssignmentState::Detached);
    assert_eq!(host.drops.count(), 1);
}

#[test]
fn requests_are_rejected_during_security_and_forced_stop() {
    for (kind, reason) in [
        (StopKind::Security, ReasonCode::SecurityStop),
        (StopKind::Forced, ReasonCode::ForcedStop),
    ] {
        let mut controller = AssignmentController::<Active>::new();
        let mut host = Host::default();
        install_initial(&mut controller, &mut host);
        let token = controller.begin_stop(kind);
        assert_eq!(token.kind(), kind);
        assert_eq!(controller.snapshot().reason, Some(reason));
        let stopping = controller.snapshot();

        assert!(matches!(
            controller.request(input(10), &mut host),
            Err(ReasonCode::StopInProgress)
        ));
        assert_eq!(controller.snapshot(), stopping);
        assert_eq!(controller.snapshot().active_assignment, Some(assignment(1)));
        assert_eq!(
            controller.active_resource().map(|resource| resource.id),
            Some(1)
        );

        assert!(controller.complete_stop(token));
        assert_eq!(controller.snapshot().state, AssignmentState::Detached);
        assert_eq!(controller.snapshot().active_assignment, None);
        assert_eq!(host.drops.count(), 1);
    }
}

#[test]
fn blockers_are_independent_and_expose_the_strongest_current_state() {
    let mut controller = AssignmentController::<Active>::new();
    let mut host = Host::default();
    install_initial(&mut controller, &mut host);
    controller.set_blocker(Blocker::UserPause, true);
    controller.set_blocker(Blocker::SessionLocked, true);
    assert_eq!(controller.snapshot().state, AssignmentState::Suspended);
    controller.set_blocker(Blocker::SessionLocked, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Paused);
    assert_eq!(controller.snapshot().reason, Some(ReasonCode::UserPause));
    controller.set_blocker(Blocker::UserPause, false);
    assert_eq!(controller.snapshot().state, AssignmentState::Running);
}
