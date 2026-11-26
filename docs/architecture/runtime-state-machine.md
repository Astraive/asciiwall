# Runtime state machine and lifecycle

## Assignment lifecycle

```text
unloaded -> validating -> preparing -> attaching -> running
                |            |            |          |
                +---------- failed <-------+          |
                                                     v
                                  paused / throttled / suspended
                                                     |
                                                     v
                                    stopping -> detached -> unloaded
```

Transitions are serialized by the controller. Heavy work occurs asynchronously and reports a generation-tagged result. A stale preparation cannot replace a newer requested wallpaper. Activation is transactional: validate, prepare a safe first frame, attach a hidden/ready surface, then switch. Keep the old assignment active until the new one can be shown.

`ascii-runtime::AssignmentController` owns the portable, serialized lifecycle. `request` validates before returning a `PreparationJob`; callers execute the job on their chosen worker and submit its generation-tagged completion. A newer request or stop invalidates older completions, and stale successful output is dropped so its owned preparation resources are released.

Activation is validate, prepare a safe first frame, attach it hidden/ready, then atomically switch. The previous active resource and identity remain owned until the host switch succeeds. Validation, preparation, attachment, and switch failures preserve that prior assignment. The `AssignmentHost::switch` contract requires an error result to leave the previous assignment visible; native Apply orchestration remains outside this portable module.

Snapshots expose state, generation, active assignment identity, independent fixed-size blockers, and bounded `ReasonCode` values only. They retain no paths, window titles, input history, or arbitrary host error text. Blockers independently map policy pause, resource throttling, and session/display/host suspension.

`SwitchTransition::UserRequested` permits a host's configured user wallpaper-switch behavior; immediate startup/recovery replacement and `StopKind::Security`/`Forced` are separate paths. Stop invalidates in-flight work before detach acknowledgement and releases the active resource on completion. No native detach or animation is implemented here. The workspace has no desktop/CLI app consumer yet; consumers can inspect the snapshot API once that surface exists.

While `Stopping`, `request` rejects new assignments without advancing the generation or changing the stop reason/token, so the authorized detach acknowledgement remains valid.

During preparation, a session-lock, display-off, or unavailable-host blocker invalidates the pending generation and suspends the previous committed assignment; its stale completion cannot replace that assignment. A precommit failure still retains the previous active resource, and a later hard blocker suspends it as well. New requests are rejected while any hard blocker remains. Clearing all hard blockers restores the previous assignment to the state derived from remaining blockers.

## Pause reasons are a set, not one boolean

Reasons include user pause, session lock, display off, fullscreen policy, low battery, battery saver, thermal policy, host unavailable, and export contention. `AssignmentController` stores them as independent `Blockers`; clearing one cause never clears another. COR-004's pure `ascii_runtime::pause` policy maps lifecycle snapshots to explicit simulation, rendering, surface, and stop actions without introducing another lifecycle controller.

`PauseAction::FreezeSimulation` retains the assignment and freezes the approved frame. `PauseAction::ThrottleRender` reduces rendering without describing the simulation as paused. `PauseAction::Suspend` keeps resource release and surface release as explicit consumer policy. `PauseAction::Stop` reflects the controller's stop path: the host must detach, end background work, and acknowledge stop; `AssignmentController::begin_stop` remains the single owner that invalidates pending work.

The portable policy marks lock, display-off, and unavailable-host suspension as transition-bypassing. It performs no native calls. A host consumer must reprobe fresh topology and capabilities after unlock/resume, then call `ResumeClock::resume_after_reprobe`; this resets the monotonic baseline, records the event watermark for discarding queued stale events, and bounds each simulation delta. The source policy selects restoring playback position or realigning to a clock. PWR-001 owns native signal subscriptions and effective platform policy.

On lock or display power-off, stop visual work promptly; do not play an exit animation on a security transition. On unlock, reprobe topology and capabilities before displaying a fresh frame. On suspend/resume, reset monotonic baselines, discard stale external events, and clamp simulation catch-up. Never integrate hours of missed physics in one tick.

## Shell and device recovery

Explorer or compositor loss invalidates native handles. Device loss invalidates GPU resources. Neither means the underlying package is corrupt. Retain declarative state, rebuild only the affected host/device resources, and retry with bounded backoff. If safe attachment cannot be established, offer preview-only or an explicit static-image fallback.

## Exit behavior

A user-requested wallpaper switch can run a configured transition. Normal app exit can attempt a short graceful out-animation. Forced termination, OS shutdown, power loss, or a crash cannot guarantee one. Cleanup must not delay security or system shutdown for aesthetics.

State changes are logged as structured reason codes without window titles, input histories, or full file paths. The UI shows the current state and blocking reasons so users do not confuse policy-driven pausing with failure.
