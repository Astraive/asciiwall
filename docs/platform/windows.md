# Windows wallpaper and interaction adapter

## Native placement

The supported Windows wallpaper-management interface manages image/slideshow settings; it is not a documented API for a custom continuously rendered GPU wallpaper. [S01](../research/source-register.md#s01) The practical live approach is a native window attached into the Explorer desktop hierarchy, commonly involving Progman/WorkerW/DefView. This is an explicitly shell-sensitive integration, not a stable public wallpaper contract. [S03](../research/source-register.md#s03) [S50](../research/source-register.md#s50)

Implement a small adapter that discovers current desktop windows, validates their relationships, and chooses a tested attach strategy. Never assume a hard-coded parent, one WorkerW layout, one magic-message sequence, or a numeric window handle remains valid after Explorer restarts. Use bounded discovery timeouts. Keep all unsafe Win32 calls inside reviewed modules.

Before attach, establish the intended DPI-awareness context, styles, non-activation behavior, and ownership. `SetParent` alone does not fix incompatible styles or DPI-awareness modes. [S02](../research/source-register.md#s02) Do not inject code into Explorer, patch its memory, subclass foreign processes, or require administrator privileges.

## Surface lifecycle

`ascii-platform` now contains an experimental `WindowsHost` library adapter; it is not wired to an application/runtime owner. `WindowsHost::new` requires the caller's UI thread, confirms the input desktop is `Default`, validates the initial frame, creates a hidden disabled controller HWND, and requests a WGPU device compatible with a temporary surface for that HWND. No wallpaper surface is created until `attach`.

`displays` performs a complete bounded `EnumDisplayMonitors` pass under per-monitor DPI awareness. The fixed-capacity public snapshot reports physical-pixel geometry and opaque local monitor identities, not HWNDs or OS device names. Topology is re-enumerated periodically and on display-change messages; message dimensions alone are never treated as a topology.

`attach` accepts only a bounded, unambiguous Explorer hierarchy containing `SHELLDLL_DefView` with a direct `Progman`/`WorkerW` parent in the same shell process and a per-monitor-aware DPI context. Unsupported or ambiguous hierarchies, inaccessible shell state, and incompatible DPI contexts fail closed. The adapter creates a disabled, nonactivating, non-topmost owned window hidden at the output bounds, creates a Win32 WGPU surface, configures an sRGB/FIFO/opaque target, and presents the initial valid frame while hidden. It then changes only that owned HWND to a disabled child, attaches it to the verified shell parent, and checks exact parent/process/class, bottom sibling order, output geometry, styles, and visibility. It revalidates shell hierarchy and the `Default` input desktop, checks that foreground ownership did not change, then reveals with `SW_SHOWNOACTIVATE`. These checks establish structural Win32 state only; they do not prove that the surface is visibly below icons or preserves desktop interaction.

`present` requires the exact active `SurfaceLease` and rechecks visible placement before rendering. Surface loss, outdated surfaces, and suboptimal presentation retire the affected lease before bounded recreation. The host polls WTS/session messages and checks the input desktop; lock, disconnect, or a non-`Default` desktop hides owned surfaces and stops presentation. Unlock triggers a full topology and shell revalidation, a hidden current-frame presentation, and only then a nonactivating reveal. Topology changes retire and rebuild affected outputs. Shell recovery backs off from 100 ms to 1.6 s, stops after five failed attempts, and requires an explicit `retry_recovery` call to reset the circuit. `Drop` releases each WGPU surface before destroying only its token-verified owned HWND; Explorer-owned windows are never destroyed.

The adapter currently reports `session.lock-state` and a low-confidence `wallpaper.surface` capability; the latter means that the adapter's structural checks succeeded, not that user-visible wallpaper placement is accepted. It does not implement pointer, window-geometry, power-state, or static-wallpaper feeds/fallbacks. The host has no application/runtime lifecycle owner, so this library implementation does not close COR-005 or provide a usable product-level Stop/controller path.

## Native acceptance status

Native evidence is **partially measured; not accepted**. On Windows 11 Home Single Language build 26300 (x64), one host/display completed 20/20 attach/present/detach cycles: each `present` returned `Skipped(Unchanged)`, each detach returned `Ok(())`, and host drop reported owned windows released. Attach #1 took 282.0724 ms; cycles 2–20 took 13.2384–16.3314 ms. No separate HWND/leak count was collected.

One guarded Explorer restart/recovery was observed (evidence: PR #371, code head `b443d0ef37d1780c271c4a13d0812b3b87fea4c2`): the monitor reported `SurfaceRetired`/HostLost and health became Unavailable, then capability returned Available/Healthy with replacement `SurfaceAttached` generation 1 and registry epoch 1. After 150 seconds it remained Healthy; `WallpaperSurface` was Available at Low confidence with lease_count 1. Host drop reported owned windows released.

Still unverified: below-icon placement, focus/nonactivation, desktop icon click/selection/menus, typing into an app, shell visual recovery, Task View/virtual desktops, topology/hotplug, lock/disconnect, recovery exhaustion, and application-controller Stop. Win+D was inconclusive: no host events were recorded, and delivery or visible effect was not confirmed. A subsequent user visual attempt is recorded below. These observations do not establish zero leaks, icon interaction, focus preservation, or acceptance; WND-002 remains experimental and unsupported.

### User visual acceptance attempt

The user ran `C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\wnd002-manual-check.exe`. It printed `READY: synthetic frame presented on the first current display; 180-second hold started.` followed by countdown `179` through `161`; the user pressed Ctrl+C. The runner printed `Graceful Ctrl+C requested; detaching.` and `EXIT: detached and host dropped cleanly.` User report: **“no i didnt see anything changed in desktop bg”**. The visible-background gate is **NOT PASSED** for this attempt. `READY` records that `host.present(...)` returned without error, not that the compositor visibly displayed the frame; the user reported no visible background change. Clean teardown is distinct from visible presentation.

- Runner source SHA-256: `8986aa92113818aee0e2dda853d5c1e6699e61fbfacbc62698b00c41e0af78c1`.
- Runner executable SHA-256: `9e83cda913833cfe4f88d4bd09e5acd833cfc338745bba22acfe1bcf749805d0`.
- `Cargo.lock` SHA-256: `91bc65be2e89983c92f0157bb124178a147fdd114e04e054dec46a0232e0abf6`.
- `Cargo.toml` SHA-256: `40e44cc390c1bdfc0f51320e40a7101b7e43f9f8ee3c59b8171f44e1aadbf3c2`.

Desktop icon click/selection and typing into an application remain **NOT TESTED**. No screenshot, focus pass, root cause, or production support is established by this attempt.

## Pointer and window feeds

A passive raw-input registration is a candidate for background mouse button observation. Do not use flags that suppress normal legacy delivery or capture the device. Raw relative movement is not a global cursor location; obtain and transform desktop coordinates separately. [S07](../research/source-register.md#s07) [S08](../research/source-register.md#s08)

Use an out-of-context WinEvent hook plus an initial snapshot to maintain window metadata. Callbacks enqueue bounded events and return immediately; they never perform conversion or GPU work. The observer needs a functioning message loop. [S04](../research/source-register.md#s04)

Normalize window rectangles deliberately: `GetWindowRect` can be DPI-virtualized and include invisible resize borders; DWM attributes offer different bounds. [S05](../research/source-register.md#s05) [S06](../research/source-register.md#s06) Exclude own windows, shell chrome, cloaked/minimized/off-desktop windows where reliably detectable. Fullscreen is a heuristic over output overlap/state, not an infallible game detector.

## Desktop-only clicks

Passive observation sees more than the wallpaper. A desktop-only reaction requires a trustworthy hit-policy result. Desktop icons, context menus, drag-selection, and app windows must still receive their original input. Never capture then replay clicks to emulate pass-through. When hit testing is uncertain, suppress the reaction; do not guess that the click was on empty wallpaper.

## Session and recovery constraints

Stop observation/rendering at lock, suspend, secure-desktop transitions where detectable, and session disconnect. Do not attempt to render into UAC/login desktops. Power events inform the scheduler; the app must not keep the computer awake. [S59](../research/source-register.md#s59)

On normal exit, destroy only owned surfaces and restore only settings the app actually changed. If the user changed static wallpaper independently, do not overwrite their newer choice. Forced termination cannot guarantee an exit animation or restoration callback. Keep a safe static background configured only with explicit user consent.

## Acceptance gate

A Windows release must survive 20 attach/detach cycles, Explorer restart, mixed-DPI monitor movement, lock/unlock, and rapid Apply changes without losing icon interaction, taking focus, leaking windows, or leaving a topmost surface. These are proposed tests, not results already measured.
