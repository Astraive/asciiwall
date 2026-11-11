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

## Pause reasons are a set, not one boolean

Reasons include user pause, session lock, display off, fullscreen policy, low battery, battery saver, thermal policy, host unavailable, and export contention. Clearing one reason must not resume while another remains. Distinguish a paused simulation from a throttled renderer and a released graphics surface.

A `user-stop` detaches wallpaper surfaces and ends background work. A `user-pause` freezes the approved frame but retains the assignment. A `suspend` releases expensive resources according to policy. Resume may restore the playback position or realign to a clock; that choice belongs to the source settings.

## Session events

On lock or display power-off, stop visual work promptly; do not play an exit animation on a security transition. On unlock, reprobe topology and capabilities before displaying a fresh frame. On suspend/resume, reset monotonic baselines, discard stale external events, and clamp simulation catch-up. Never integrate hours of missed physics in one tick.

## Shell and device recovery

Explorer or compositor loss invalidates native handles. Device loss invalidates GPU resources. Neither means the underlying package is corrupt. Retain declarative state, rebuild only the affected host/device resources, and retry with bounded backoff. If safe attachment cannot be established, offer preview-only or an explicit static-image fallback.

## Exit behavior

A user-requested wallpaper switch can run a configured transition. Normal app exit can attempt a short graceful out-animation. Forced termination, OS shutdown, power loss, or a crash cannot guarantee one. Cleanup must not delay security or system shutdown for aesthetics.

State changes are logged as structured reason codes without window titles, input histories, or full file paths. The UI shows the current state and blocking reasons so users do not confuse policy-driven pausing with failure.
