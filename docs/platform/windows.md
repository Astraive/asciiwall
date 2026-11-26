# Windows wallpaper and interaction adapter

## Native placement

The supported Windows wallpaper-management interface manages image/slideshow settings; it is not a documented API for a custom continuously rendered GPU wallpaper. [S01](../research/source-register.md#s01) The practical live approach is a native window attached into the Explorer desktop hierarchy, commonly involving Progman/WorkerW/DefView. This is an explicitly shell-sensitive integration, not a stable public wallpaper contract. [S03](../research/source-register.md#s03) [S50](../research/source-register.md#s50)

Implement a small adapter that discovers current desktop windows, validates their relationships, and chooses a tested attach strategy. Never assume a hard-coded parent, one WorkerW layout, one magic-message sequence, or a numeric window handle remains valid after Explorer restarts. Use bounded discovery timeouts. Keep all unsafe Win32 calls inside reviewed modules.

Before attach, establish the intended DPI-awareness context, styles, non-activation behavior, and ownership. `SetParent` alone does not fix incompatible styles or DPI-awareness modes. [S02](../research/source-register.md#s02) Do not inject code into Explorer, patch its memory, subclass foreign processes, or require administrator privileges.

## Surface lifecycle

Create hidden; configure size and rendering; present a valid first frame; attach; verify placement and non-focusability; then reveal. Failed verification detaches and shows an explanation in the controller. A window must never temporarily appear above normal applications during recovery.

Observe display changes and re-enumerate outputs. The dimensions in a display-change message are not a full multi-monitor topology description. [S58](../research/source-register.md#s58) Explorer termination invalidates shell references; discard them and rediscover. Use a bounded exponential backoff, with a circuit breaker after repeated failures.

Validate Win+D, Show Desktop, Task View, virtual-desktop changes, desktop icon toggling, auto-hide taskbars, custom scale, desktop refresh, shell restarts, and remote-session transitions. Keep a lightweight controller window/message pump alive even when the settings UI is closed.

## Passive pointer and window observation

`ascii_platform::WindowsObserver` is a Windows-only observer, separate from wallpaper-surface ownership. `WindowsObserverConsent::default()` is disabled and starts no worker or hooks. The application must obtain explicit user consent before granting pointer/button or window-metadata observation; desktop-click classification additionally requires both grants. `revoke()` synchronously stops this observer, unregisters only its own raw-input registration, unhooks, and clears retained observations. It does not stop or own wallpaper rendering.

When consented, mouse buttons use `RIDEV_INPUTSINK` only. Registration is refused if the process already has a mouse Raw Input registration; `RIDEV_NOLEGACY`, capture, suppression, and input replay are never used. Raw Input supplies button transitions; global screen coordinates are sampled separately with `GetCursorPos` in the native window procedure and associated with the monitor under that point. The procedure copies fixed-size button fields and enqueues them; WinEvent callbacks only enqueue bounded events; enumeration callbacks only copy bounded native handles/rectangles. `HostAdapter` publishes only the existing pointer-position, pointer-button, window-rectangle, and window-state feeds. No output-scale or session-lock feed is published. The queues and snapshots are fixed-capacity (128 native events, 32 host events, 32 click observations, 10 windows, and 8 outputs). Overflow is reported through reconciliation or an explicit click-batch overflow flag; consumers must discard hit-sensitive history on click overflow.

An out-of-context WinEvent hook set and initial `EnumWindows` snapshot maintain metadata. If the complete hook set is unavailable, the window feeds are unavailable and no initial snapshot is exposed. Own-process windows are excluded; shell-process windows are excluded only when Windows reports their process identity. Hidden, minimized, and reliably DWM-cloaked windows are retained as state-only entries. If the cloak query fails, geometry is retained with `WindowState::Other`; windows are not discarded based on an unavailable virtual-desktop query. No title or process name is collected or logged.

Bounds prefer DWM extended-frame coordinates. `GetWindowRect` is used only when the worker can verify its per-monitor-v2 thread DPI context; otherwise missing bounds remain missing. The Windows-local `windows_snapshot()` retains each rectangle's source (`DwmExtendedFrame` or `GetWindowRectPerMonitorV2`), while `HostAdapter` publishes only the existing handle-free pointer/button/rectangle/state feeds. This adapter does not publish output-scale values or promote its extra observations into shared capability/feed fields. Fullscreen candidates are Windows-local and per-output: at least 98% overlap is required and confidence is medium, not an infallible game/fullscreen detector. Output association is left unknown on a tie.

## Desktop-only click observations

Click classification is a Windows-local passive observation, not a shared capability claim or an instruction to deliver input. A positive empty-wallpaper result requires a shell-process `Progman`/`WorkerW` target or the verified Explorer desktop icon list and a bounded list-view hit test that finds no item. Icon hits, menus, taskbar surfaces, and non-shell windows are occupied. Failed, timed-out, stale, or ambiguous hit tests are unknown; unknown and occupied results must suppress wallpaper reactions. The click queue never captures, changes, or replays input, so the normal icon/menu/application click path remains owned by Windows. Consumers should use `drain_desktop_clicks()` and discard the batch's hit-sensitive history if `overflowed` is set.

## Lock and secure-desktop boundaries

The observer registers for current-session WTS notifications and independently checks the input desktop every 250 ms. A reported lock, disconnect/logoff, non-default input desktop, or inability to verify the default desktop stops the observer and clears pointer, window, output, and click observations. Lock notifications are used only to revoke/clear observations; this adapter does not publish a shared session-lock feed or resume itself after a boundary. It does not observe UAC/login desktops and does not control or stop wallpaper rendering. An application may create a new observer only after its own consent policy permits it.

## Acceptance evidence

The deterministic Windows-only tests cover disabled-by-default consent, consent dependency validation, pure shell hit-policy outcomes, bounded/coalescing event queues, click-overflow reporting, rectangle validation, and the per-output 98% fullscreen heuristic. Pending Windows x64 checks are:

```text
cargo check -p ascii-platform --target x86_64-pc-windows-msvc
cargo test -p ascii-platform --target x86_64-pc-windows-msvc observer::windows::tests
cargo test -p ascii-platform --target x86_64-pc-windows-msvc observer::tests
```

These checks have not been run in this change. Live acceptance evidence is also pending: unchanged original icon/menu/application click delivery, 1000-Hz/event-storm behavior, mixed-DPI calibration and monitor movement, lock/secure-desktop transitions, Explorer restart, and revocation while wallpaper rendering continues. Do not treat the adapter implementation or its deterministic tests as evidence that these native/manual gates passed.
