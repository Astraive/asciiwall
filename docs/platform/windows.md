# Windows wallpaper and interaction adapter

## Native placement

The supported Windows wallpaper-management interface manages image/slideshow settings; it is not a documented API for a custom continuously rendered GPU wallpaper. [S01](../research/source-register.md#s01) The practical live approach is a native window attached into the Explorer desktop hierarchy, commonly involving Progman/WorkerW/DefView. This is an explicitly shell-sensitive integration, not a stable public wallpaper contract. [S03](../research/source-register.md#s03) [S50](../research/source-register.md#s50)

Implement a small adapter that discovers current desktop windows, validates their relationships, and chooses a tested attach strategy. Never assume a hard-coded parent, one WorkerW layout, one magic-message sequence, or a numeric window handle remains valid after Explorer restarts. Use bounded discovery timeouts. Keep all unsafe Win32 calls inside reviewed modules.

Before attach, establish the intended DPI-awareness context, styles, non-activation behavior, and ownership. `SetParent` alone does not fix incompatible styles or DPI-awareness modes. [S02](../research/source-register.md#s02) Do not inject code into Explorer, patch its memory, subclass foreign processes, or require administrator privileges.

## Surface lifecycle

`ascii-platform` now contains an experimental `WindowsHost` library adapter; it is not wired to an application/runtime owner. `WindowsHost::new` requires the caller's UI thread, confirms the input desktop is `Default`, validates the initial frame, creates a hidden disabled controller HWND, and requests a WGPU device compatible with a temporary surface for that HWND. No wallpaper surface is created until `attach`.

`displays` performs a complete bounded `EnumDisplayMonitors` pass under per-monitor DPI awareness. The fixed-capacity public snapshot reports physical-pixel geometry and opaque local monitor identities, not HWNDs or OS device names. Topology is re-enumerated periodically and on display-change messages; message dimensions alone are never treated as a topology.

`attach` accepts only a bounded, unambiguous raised hierarchy: the unique shell-owned top-level `Progman` must be the actual `GetShellWindow()` handle and carry `WS_EX_NOREDIRECTIONBITMAP`; its unique direct shell-owned `SHELLDLL_DefView` child must independently carry `WS_EX_LAYERED`; and one unique shell-owned direct-child `WorkerW` must follow the view. The v4 host probe observed `WS_EX_NOREDIRECTIONBITMAP` on `Progman` and `WS_EX_LAYERED` on `DefView`, on different windows. Unsupported or ambiguous hierarchies, inaccessible shell state, and incompatible DPI contexts fail closed. The adapter creates only an opaque (`WS_EX_LAYERED`, alpha 255), disabled, nonactivating child of `Progman` at the output bounds, creates a Win32 WGPU surface, configures an sRGB/FIFO/opaque target, and presents the initial valid frame while hidden. It then places the child with `SetWindowPos(hWnd, DefView, ...)` and verifies the complete host-owned child band between `DefView` and `WorkerW`, exact class/process/parent/styles, physical output geometry, and visibility. It never parents to `WorkerW` and has no `HWND_BOTTOM` or top-level-WorkerW fallback. A missing raised slot may trigger one conditional `0x052C` request only during attach/recovery, after uniqueness, raised styles, an unambiguous sibling band, the `Default` input desktop, and cancellation are checked. The request uses `SendMessageTimeoutW` with `SMTO_ABORTIFHUNG` and a hard 250 ms timeout; one fresh enumeration must then confirm the shell-owned `WorkerW` slot and exact order. `0x052C` is a private, undocumented Explorer message, so this remains an experimental dependency; periodic hierarchy checks and the diagnostic probe never send it. It then revalidates shell hierarchy and the `Default` input desktop, checks that foreground ownership did not change, and reveals with `SW_SHOWNOACTIVATE`. After successful reveal, it invalidates the renderer and requires a fresh post-reveal `Presented` result before attach succeeds. If that presentation is cancelled or fails, it hides the window and retires the lease. These checks establish structural Win32 state and renderer submission only; they do not prove that the compositor visibly displays the surface below icons or preserves desktop interaction.
The Progman target thread must differ from the caller's because Win32 does not apply `SendMessageTimeoutW`'s timeout to a same-thread target; a zero return is failure even when `GetLastError` reports success.

`present` requires the exact active `SurfaceLease` and rechecks visible placement before rendering. Surface loss, outdated surfaces, and suboptimal presentation retire the affected lease before bounded recreation. The host polls WTS/session messages and checks the input desktop; lock, disconnect, or a non-`Default` desktop hides owned surfaces and stops presentation. Unlock triggers a full topology and shell revalidation, a hidden current-frame presentation, and only then a nonactivating reveal. Topology changes retire and rebuild affected outputs. Shell recovery backs off from 100 ms to 1.6 s, stops after five failed attempts, and requires an explicit `retry_recovery` call to reset the circuit. `Drop` releases each WGPU surface before destroying only its token-verified owned HWND; Explorer-owned windows are never destroyed.
The per-present check is limited to the visible surface's parent, styles, and geometry; full sibling-order and owned-band validation runs before reveal and during periodic hierarchy checks.

The adapter currently reports `session.lock-state` and a low-confidence `wallpaper.surface` capability; the latter means that the adapter's structural checks succeeded, not that user-visible wallpaper placement is accepted. It does not implement pointer, window-geometry, power-state, or static-wallpaper feeds/fallbacks. The host has no application/runtime lifecycle owner, so this library implementation does not close COR-005 or provide a usable product-level Stop/controller path.

## Native acceptance status

Native evidence is **partially measured; not accepted**. On Windows 11 Home Single Language build 26300 (x64), one host/display completed 20/20 attach/present/detach cycles: each `present` returned `Skipped(Unchanged)`, each detach returned `Ok(())`, and host drop reported owned windows released. Attach #1 took 282.0724 ms; cycles 2–20 took 13.2384–16.3314 ms. No separate HWND/leak count was collected.

One guarded Explorer restart/recovery was observed (evidence: PR #371, code head `b443d0ef37d1780c271c4a13d0812b3b87fea4c2`): the monitor reported `SurfaceRetired`/HostLost and health became Unavailable, then capability returned Available/Healthy with replacement `SurfaceAttached` generation 1 and registry epoch 1. After 150 seconds it remained Healthy; `WallpaperSurface` was Available at Low confidence with lease_count 1. Host drop reported owned windows released.

Still unverified or untested: focus/nonactivation, desktop icon click/selection/menus, typing into an app, shell visual recovery, Task View/virtual desktops, topology/hotplug, lock/disconnect, recovery exhaustion, and application-controller Stop. Below-icon visible placement is **NOT PASSED** for the user visual attempt recorded below. The raised-desktop strategy has not had a post-change visual or interaction acceptance run. Win+D was inconclusive: no host events were recorded, and delivery or visible effect was not confirmed. These observations do not establish zero leaks, icon interaction, focus preservation, or acceptance; WND-002 remains experimental and unsupported.

### User visual acceptance attempt

The user ran `C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\wnd002-manual-check.exe`. It printed `READY: synthetic frame presented on the first current display; 180-second hold started.` followed by countdown `179` through `161`; the user pressed Ctrl+C. The runner printed `Graceful Ctrl+C requested; detaching.` and `EXIT: detached and host dropped cleanly.` User report: **“no i didnt see anything changed in desktop bg”**. The visible-background gate is **NOT PASSED** for this attempt. `READY` records that `host.present(...)` returned without error, not that the compositor visibly displayed the frame; the user reported no visible background change. Clean teardown is distinct from visible presentation.

- Runner source SHA-256: `8986aa92113818aee0e2dda853d5c1e6699e61fbfacbc62698b00c41e0af78c1`.
- Runner executable SHA-256: `9e83cda913833cfe4f88d4bd09e5acd833cfc338745bba22acfe1bcf749805d0`.
- `Cargo.lock` SHA-256: `91bc65be2e89983c92f0157bb124178a147fdd114e04e054dec46a0232e0abf6`.
- `Cargo.toml` SHA-256: `40e44cc390c1bdfc0f51320e40a7101b7e43f9f8ee3c59b8171f44e1aadbf3c2`.

Desktop icon click/selection and typing into an application remain **NOT TESTED**. No screenshot, focus pass, root cause, or production support is established by this attempt.

### Updated diagnostic runner (built and user-run; HWND creation failed)

The user ran this release runner. Shell discovery completed and reported one Progman, one DefView parented by Progman, 15 top-level WorkerW windows, and a 1920x1080 display at (0,0). `CreateWindowExW` then failed with reported Win32 code `0`; the process exited before creating a surface, attaching, or presenting. The user saw no desktop change. The run output was:

```text
PROBE: observation=Observation { status: Experimental, reason: ShellHierarchyObserved, error: None, win32_error: None }; shell_window_class=Some(Progman); top_level_progman_count=1; top_level_workerw_count=15; desktop_view_count=1; shell_owned_desktop_view_count=1; desktop_views_parented_by_progman=1; desktop_views_parented_by_workerw=0; desktop_views_with_other_parent=0
DISPLAY: left_px=0 top_px=0 width_px=1920 height_px=1080
Error: Win32 { operation: "CreateWindowExW", code: 0 }
```

- Build: **PASSED** with `CARGO_HOME=D:/tmp/cargo-cor008-acl-20261127` and `CARGO_TARGET_DIR=D:/tmp/target-cor008-acl-20261127`:
  `cargo +1.100.0 build --offline --locked --release --manifest-path 'C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\Cargo.toml'`
- Runner source SHA-256: `d6c65373bf88b463443adc2dcab3335fd54c47bebf33244dd80f929228aeb8e3`.
- Runner executable SHA-256: `026dbdb333413db331f1572fce6261aa6678b34d6ca7dd379ef49be0a0ec4145`.
- Runner executable: `D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe`.

### Instrumented diagnostic runner (user-run; HWND creation still fails)

The user ran this build. `CreateWindowExW` still failed before the application window procedure: captured Win32 `code=0`, `stage=0` (`WM_NCCREATE` and `WM_CREATE` were not reached), parent `458818`, styles `0x4e000000` / `0x08080080`, and bounds `(0,0,1920,1080)`. No HWND was created, so there was no attach, reveal, or present. The user saw no desktop change. Exact output:

```text
PROBE: observation=Observation { status: Experimental, reason: ShellHierarchyObserved, error: None, win32_error: None }; shell_window_class=Some(Progman); top_level_progman_count=1; top_level_workerw_count=15; desktop_view_count=1; shell_owned_desktop_view_count=1; desktop_views_parented_by_progman=1; desktop_views_parented_by_workerw=0; desktop_views_with_other_parent=0
DISPLAY: left_px=0 top_px=0 width_px=1920 height_px=1080
Error: CreateWindowFailed { kind: "surface", code: 0, parent: 458818, style: 1308622848, extended_style: 134742144, left: 0, top: 0, width: 1920, height: 1080, stage: 0 }
```

- Build: **PASSED** with `CARGO_HOME=D:/tmp/cargo-cor008-acl-20261127` and `CARGO_TARGET_DIR=D:/tmp/target-cor008-acl-20261127`:
  `cargo +1.100.0 build --offline --locked --release --manifest-path 'C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\Cargo.toml'`
- Runner source SHA-256: `d6c65373bf88b463443adc2dcab3335fd54c47bebf33244dd80f929228aeb8e3`.
- Instrumented executable SHA-256: `ae213a07b4ffb825a930a8ca9d8cc975e738ccf3bde438edfd309e61d4252cdd`.
- Executable: `D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe`.

### WND-002 post-reveal redraw diagnosis

Static diagnosis found that `attach_internal` rendered and cached the initial frame while the HWND was hidden. Once revealed, the manual runner's same-frame/same-target `host.present` could return `Skipped(Unchanged)`, so no renderer presentation was required after reveal. The fix invalidates renderer state after a successful reveal and requires a fresh `Presented` outcome; failure hides the window and retires its lease.

The user-run CBT diagnostic observed one `HCBT_CREATEWND` event for the expected class, parent, styles, and bounds. `CallNextHookEx` returned `0`, so no downstream CBT veto was observed in this invocation. `CreateWindowExW` nevertheless failed with captured code `0` and stage `0`; the app did not record `WM_NCCREATE` or `WM_CREATE`. The user reports no wallpaper/background change. This rules out only the observed downstream-CBT-veto path; the cause remains unknown. No usable application surface, attach, reveal, or present completed. Icon and typing/focus acceptance remain **UNVERIFIED**. Keep the implementation experimental and PR/issue open.

### Thread-specific CBT follow-up runner (user-run; creation still fails)

The previous run failed at `CreateWindowExW` with captured last-error `0` and stage `0`. This disposable runner installs a thread-specific `WH_CBT` hook only around `host.attach`, captures `HCBT_CREATEWND` fields from `CREATESTRUCTW`, then calls `CallNextHookEx` and returns its result unchanged ([`SetWindowsHookExW`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw), [`CallNextHookEx`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-callnexthookex)). If hook installation fails, it reports the error and proceeds without an installed observer. It unhooks immediately after attach; unhook failure reports incomplete capture and stops before presentation or hold. The temporary hook changes this thread's hook-chain ordering while installed; it is not global.

Interpret the output conservatively:
- `hcbt_createwnd_callbacks=1` with nonzero `hcbt_createwnd_downstream` is evidence that a downstream CBT hook vetoed that creation. Microsoft documents that a nonzero `HCBT_CREATEWND` result prevents window creation ([CBTProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/cbtproc)).
- Zero callbacks cannot distinguish a pre-CBT validation failure from creation on a different thread; `hook_install_failed` separately means the observer was not installed.
- More than one callback can include nested/reentrant creation. The runner stores one event's fields, which can be ambiguous in that case; send the full output and do not treat the downstream value as decisive.

- Format check: **PASSED** with `cargo +1.100.0 fmt --manifest-path 'C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\Cargo.toml' --check`.
- Release build: **PASSED** with `CARGO_HOME=D:/tmp/cargo-cor008-acl-20261127` and `CARGO_TARGET_DIR=D:/tmp/target-cor008-acl-20261127` using `cargo +1.100.0 build --offline --locked --release --manifest-path 'C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\Cargo.toml'`.
- Runner source SHA-256: `8739e9e6ba9f607371c0ae96baf324313945ef56406d49e2bb77eb8ed203f71c`.
- Runner executable SHA-256: `6054c41f4f292f0a97ee59c8cc1219e9c70e7edc278838f82a8fb778b1203252`.
- Runner executable: `D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe`.

The user ran this runner and reported no wallpaper/background change. The CBT callback saw a candidate HWND, but `CreateWindowExW` still returned failure before a usable surface existed. Exact output:

```text
PS C:\Users\gskne> & "D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe"
PROBE: observation=Observation { status: Experimental, reason: ShellHierarchyObserved, error: None, win32_error: None }; shell_window_class=Some(Progman); top_level_progman_count=1; top_level_workerw_count=15; desktop_view_count=1; shell_owned_desktop_view_count=1; desktop_views_parented_by_progman=1; desktop_views_parented_by_workerw=0; desktop_views_with_other_parent=0
DISPLAY: left_px=0 top_px=0 width_px=1920 height_px=1080
CBT: hcbt_createwnd_callbacks=1 hwnd=0x6F1292 parent=0x70042 class="AsciiWall.WindowsHost.0000000000000001" style=0x4E000000 exstyle=0x8080080 bounds=(0, 0, 1920, 1080) hcbt_createwnd_downstream=0
Error: CreateWindowFailed { kind: "surface", code: 0, parent: 458818, style: 1308622848, extended_style: 134742144, left: 0, top: 0, width: 1920, height: 1080, stage: 0 }
PS C:\Users\gskne>
```

The candidate handle `0x6F1292` was observed during `HCBT_CREATEWND`; it is not evidence of a successfully created or retained surface. The single callback with downstream result `0` does not support a downstream CBT veto. The next diagnosis must explain the failure after the CBT notification and before the app's window procedure.

### Hidden parent/style diagnostic runner (user-run; full style set still fails)

The production creation-stage tracker is reset immediately before the synchronous `CreateWindowExW` call and records `WM_NCCREATE` / `WM_CREATE` at the first statements of the app WndProc. The user's `stage: 0` result therefore strongly indicates neither creation message reached that WndProc; a second entry-only counter would be redundant. The cause remains unknown.

The follow-up runner leaves the normal attach attempt unchanged. If attach fails, it records the failed parent's validity, class, owner PID/TID, current PID/TID, style, and parent/current DPI contexts. It then tests hidden 1×1 `WS_CHILD | WS_DISABLED` creation (`0x48000000`, extended style `0`) with the same class/module and the shell parent's DPI context: first under the observed shell parent, then under the hidden controller. Only after the controller-parent minimal trial creates and destroys successfully does it test the full production child styles (`0x4E000000` / `0x08080080`) under that controller. No trial sets `WS_VISIBLE`, calls `ShowWindow`, or calls `SetParent`; successful trial windows are destroyed immediately. A cleanup failure stops further trials, and the original attach error then returns from the runner. These ablations diagnose window creation only; they are not placement or wallpaper-visibility acceptance.

- Independent Luna safety review: no blocker. The CBT capture buffer is 128 UTF-16 units; the production class name is a fixed prefix plus 16 hexadecimal digits, so it fits. If `DestroyWindow` fails, later probes stop and the original attach error returns from `main`; there is no in-function retry.
- Format check: **PASSED** with `cargo +1.100.0 fmt --manifest-path 'C:/Users/gskne/AppData/Local/Temp/wnd002-manual-check/Cargo.toml' --check`.
- Release build: **PASSED** with `CARGO_HOME=D:/tmp/cargo-cor008-acl-20261127` and `CARGO_TARGET_DIR=D:/tmp/target-cor008-acl-20261127`, using `cargo +1.100.0 build --manifest-path 'C:/Users/gskne/AppData/Local/Temp/wnd002-manual-check/Cargo.toml' --release`.
- Runner source SHA-256: `f63288ab49c5bd4c5d0d07fa146fbe4d8d85f12707586fca6ed51b834551dea5`.
- Runner executable SHA-256: `f2c4707401dbbbadeffd0e66fa4b8f4458a3e151e6ebf69ca7a72e4f4c90bf57`.
- Runner executable: `D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe`.

The user ran this binary; the assistant did not launch it. PowerShell command used:

```powershell
& "D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe"
```

The original attach attempt still failed before surface creation (`stage: 0`); the hidden minimal-style trials succeeded and were destroyed, while the full-style controller trial returned a null HWND with code `0`. The runner did not print `READY`.

#### User-run result

```text
PS C:\Users\gskne> & "D:\tmp\target-cor008-acl-20261127\release\wnd002-manual-check.exe"
PROBE: observation=Observation { status: Experimental, reason: ShellHierarchyObserved, error: None, win32_error: None }; shell_window_class=Some(Progman); top_level_progman_count=1; top_level_workerw_count=15; desktop_view_count=1; shell_owned_desktop_view_count=1; desktop_views_parented_by_progman=1; desktop_views_parented_by_workerw=0; desktop_views_with_other_parent=0
DISPLAY: left_px=0 top_px=0 width_px=1920 height_px=1080
CBT: hcbt_createwnd_callbacks=1 hwnd=0x3D12D8 parent=0x70042 class="AsciiWall.WindowsHost.0000000000000001" style=0x4E000000 exstyle=0x8080080 bounds=(0, 0, 1920, 1080) hcbt_createwnd_downstream=0
FAILED_PARENT: hwnd=0x70042 is_window=true class=Some("Progman") owner_pid=49008 owner_tid=30008 current_pid=62600 current_tid=50468 style=0x96000000 parent_dpi_context=34(other/unknown) current_dpi_context=24592(other/unknown)
TRIAL A_shell_minimal: parent=0x70042 style=0x48000000 exstyle=0x00000000 bounds=(0,0,1,1) hwnd=0x3E12D8 immediate_error=0 destroy_success=true destroy_error=Some(0)
TRIAL B_controller_minimal: parent=0x6E0BEA style=0x48000000 exstyle=0x00000000 bounds=(0,0,1,1) hwnd=0x3F12D8 immediate_error=0 destroy_success=true destroy_error=Some(0)
TRIAL C_controller_full: parent=0x6E0BEA style=0x4E000000 exstyle=0x08080080 bounds=(0,0,1,1) hwnd=0x0 immediate_error=0 destroy_success=false destroy_error=None
Error: CreateWindowFailed { kind: "surface", code: 0, parent: 458818, style: 1308622848, extended_style: 134742144, left: 0, top: 0, width: 1920, height: 1080, stage: 0 }
PS C:\Users\gskne>
```

`Progman` was a valid but foreign-process/thread parent at the metadata check. The same minimal child style succeeded under both Progman and the app's hidden controller, so invalid parent and foreign-process parenting alone do not explain the failure. The full production style/exstyle pair (`0x4E000000` / `0x08080080`) failed even at 1×1 under the same-process controller; the production 1920×1080 dimensions and Progman parent are therefore not necessary for this failure. The result narrows the problem to the full style/exstyle set or an interaction; it does not identify a specific bit. The `0` DPI-context values are opaque handles classified as unknown by the diagnostic, not DPI modes. The CBT callback returned `0`, so no downstream CBT veto was observed. No usable surface or visual/interaction acceptance was established.

#### User-run hidden 2×2 style/exstyle matrix
Source SHA-256: `1eded3eda7d6b8e08ed1d9fe6088d9d94952748f5c2d575849c3dbc218a39d95`; executable SHA-256: `9d95287bf4b6d808d0a3c13d8421c4527d7622d88692ea01754194c71f4a7fef`.

PowerShell command: `& "D:\tmp\target-cor008-acl-20261127\release\wnd002-style-matrix.exe"`.

The user ran this runner. It creates a hidden 1×1 controller with production controller styles, a private class, and a WndProc matching production creation-stage and special-message behavior. It adopts Progman's DPI context for four child-window trials at 1×1. This is a controlled controller/WndProc replica, not full production-runtime parity; it omits the WGPU bootstrap. It uses no `WS_VISIBLE`, `ShowWindow`, or `SetParent`; successful trials are destroyed immediately.

User-run output:

```text
PS C:\Users\gskne> & "D:\tmp\target-cor008-acl-20261127\release\wnd002-style-matrix.exe"
TRIAL child-disabled/no-exstyle: style=0x48000000 exstyle=0x00000000 hwnd=0x4212D8 get_last_error=0 creation_stage=0x3
CLEANUP child-disabled/no-exstyle: destroyed immediately
TRIAL production-style/no-exstyle: style=0x4E000000 exstyle=0x00000000 hwnd=0x4312D8 get_last_error=0 creation_stage=0x3
CLEANUP production-style/no-exstyle: destroyed immediately
TRIAL child-disabled/production-exstyle: style=0x48000000 exstyle=0x08080080 hwnd=0x0 get_last_error=0 creation_stage=0x0
TRIAL production-style/production-exstyle: style=0x4E000000 exstyle=0x08080080 hwnd=0x0 get_last_error=0 creation_stage=0x0
PS C:\Users\gskne>
```

Both window-style variants succeeded without production extended styles and reached stage `0x3`; both failed with the full production extended-style set before `WM_NCCREATE` (`stage=0x0`), even with the minimal window style. `GetLastError` remained `0`. This implicates the extended-style set or an interaction in this controlled replica, not a specific bit; it does not establish exact production-runtime causality.

#### Next hidden eight-case extended-style matrix (built; pending user run)
Source SHA-256: `6bb78c7dac67affb7b4a8ecc6efd8ecac41ac4be7aaf25b0d8092bc55352f9d2`; executable SHA-256: `4867939788be2d8fa3509290c30dcb7993d81e3249b2b5a10dbc46d78b973709`.

PowerShell command: `& "D:\tmp\target-cor008-acl-20261127\release\wnd002-exstyle-matrix.exe"`.

The runner fixes child style to `0x48000000` and tests all eight combinations of `WS_EX_NOACTIVATE` (`0x08000000`), `WS_EX_TOOLWINDOW` (`0x00000080`), and `WS_EX_LAYERED` (`0x00080000`) under one hidden 1×1 same-process controller. It uses the Progman DPI context and logs `GetLastError` plus creation-stage bits. Successful trials are destroyed immediately; the runner uses no `WS_VISIBLE`, `ShowWindow`, or `SetParent`. Luna reviewed it; `cargo fmt` and Windows x64 release build passed. It is a controlled replica without the WGPU bootstrap, has not been run, and does not test visible placement.

#### Lively Wallpaper source comparison (reference only)

Reviewed Lively's [`WinDesktopCore`](https://github.com/lively-community/lively/blob/core-separation/src/Lively/Lively/Core/WinDesktopCore.cs#L153-L236) on branch `core-separation`. It checks Progman's **extended** style for `WS_EX_NOREDIRECTIONBITMAP`; when raised desktop is detected, it looks for a `WorkerW` child of Progman. Otherwise it finds the top-level WorkerW after `SHELLDLL_DefView`. Its per-screen path positions an already-created renderer HWND before attaching; the raised attach path adds `WS_CHILD`, adds `WS_EX_LAYERED` with alpha 255 before `SetParent`, inserts the HWND relative to DefView, and repairs WorkerW z-order ([attach](https://github.com/lively-community/lively/blob/core-separation/src/Lively/Lively/Core/WinDesktopCore.cs#L1043-L1081), [transparency helper](https://github.com/lively-community/lively/blob/core-separation/src/Lively/Lively.Common/Helpers/WindowUtil.cs#L300-L329)). A historical [Windows 11 26002+ fix](https://github.com/lively-community/lively/commit/9142f6a4cfc2) added the Progman-child WorkerW lookup as a fallback.

This differs from AsciiWall's failing `CreateWindowExW` child creation and does not establish a fix. The current AsciiWall output reports Progman's `GWL_STYLE` (`0x96000000`), not `GWL_EXSTYLE`, and counts top-level WorkerW windows only; it does not establish `WS_EX_NOREDIRECTIONBITMAP` or a Progman-child WorkerW. Lively's setup also sends private message `0x052C` with `SMTO_NORMAL`/1000 ms; do not copy that behavior blindly. Lively is GPL-3.0; this comparison records behavior only, with no code copied.

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
