# WND-001 Windows host-probe evidence

## Scope and strategy

This issue-scoped probe performs read-only queries: `GetShellWindow`, bounded `EnumWindows` / `EnumChildWindows` traversal, class-name and owner-process inspection for candidate desktop roots and `SHELLDLL_DefView` nodes, plus OS-version, native-architecture, and current-session queries. The strategy identifier is `windows-shell-hierarchy-enumeration-v1`.
The traversal caps relevant shell/candidate-root and child records at 512 and enforces a 250 ms callback deadline. Unrelated top-level roots are only class-checked under that deadline; they consume no work or record budget and are not traversed. Budget exhaustion, timeout, missing shell, and failed owner queries for candidate roots or desktop-view nodes are reported explicitly.
`SHELLDLL_DefView` nodes under unrelated roots are excluded from desktop classification; the probe does not count Explorer folder views outside its candidate roots. It retains only class categories/counts; no HWND or foreign process ID is returned.

The probe itself is read-only and does not create a window or graphics device, attach a surface, send shell messages, restart Explorer, or change desktop state. A separate disposable WND-002 integration runner did create and attach a surface; its evidence is recorded below, and does not change the scope of this hierarchy-only probe. GPU adapter/backend identity is **NOT QUERIED** by the hierarchy-only probe; neither the probe nor the runner identified which GPU rendered the surface.

## Recorded non-interactive host observation

The previously supplied read-only host observation (not output from this implementation or a CI run) reported the following exact environment tuple:

- OS: Windows 11 Home Single Language, build `26300`, x64.
- Session: Console.
- Graphics devices observed in the host inventory: Intel UHD, driver `32.0.101.7026`; NVIDIA RTX 3050 laptop, driver `32.0.16.2124`.
- Probe strategy: `windows-shell-hierarchy-enumeration-v1`; the probe itself does not query GPU identities.

The query result was: `GetShellWindow()` returned an Explorer-owned `Progman` window with a `SHELLDLL_DefView` child. In that same process, `FindWindowW("Progman", nullptr)` did not find a window. Top-level `WorkerW` enumeration returned 18 windows and found no `SHELLDLL_DefView` child. The probe deliberately does not rely on `FindWindowW`, a fixed parent, a numeric handle, or one universal WorkerW layout. No HWNDs or foreign process IDs are recorded here.

This is hierarchy observation only. It is not evidence that a surface can be placed below icons or that the hierarchy is stable across other Windows builds, sessions, or Explorer restarts.

## Disposable WND-002 integration-runner evidence

This is separate from the historical user-provided observation above and from the read-only WND-001 probe. It records one disposable WND-002 attach/detach run; it is not a visual acceptance result or a repeated-cycle test.

- Host tuple: Windows 11 Home Single Language, build `26300` (`10.0.26300`), x64; Console session ID `2`.
- Graphics inventory: Intel UHD, driver `32.0.101.7026`; NVIDIA RTX 3050 laptop, driver `32.0.16.2124`. The probe/runner did not identify the WGPU adapter/backend used; do not infer which GPU rendered the surface.
- Probe: 1 display, `1920x1080` at origin `0,0`; elapsed `873.8 µs`; `Experimental / ShellHierarchyObserved`. Shell class `Progman`; shell enumerated `true`; counts: 1 top-level `Progman`, 16 `WorkerW`, 1 desktop view; ownership/parentage: shell-owned 1, parented by `Progman` 1, by `WorkerW` 0, under `Progman` root 1, under `WorkerW` root 0.
- Attach: `433.9142 ms`; lease owner `ascii-platform.windows-explorer` v`1.0.0`, instance 1, generation 0, registry epoch 1.
- Present: `Ok(Skipped(Unchanged))`; surface reported active; host events reached `Healthy`.
- Detach: `Ok(())`; host dropped and reported owned windows released.
- Runner source SHA-256: `3a7f65288abca4410bed8b320053cdfe24b52a134ce40b41d85e16b768a96cad`.
- `Cargo.lock` SHA-256: `f73dc0c5038492dc8462eb684f0eae85210e4aed1de3204d28029fa91f1ddf7f`.
- Rebuilt executable SHA-256: `3838693f52de182338ee952d15228457fbf0fd735a2fbffa4ec98351cf99d7ec`.
- Runner built with pinned Rust `1.100.0`, target `x86_64-pc-windows-msvc`.
- WND-001 source hashes: `crates/ascii-platform/src/probe.rs` `fe2d75f1569ebc539f7fabc5db170cbe4bde1fce85671b250e3dd05483e4f416`; `crates/ascii-platform/src/probe/windows.rs` `27d4b228f01546df6030954ea49ac5d888d7ad3fda896954c1798d309109dec9`; `crates/ascii-platform/src/lib.rs` `5cbb5ea5a0d2fd07f283050934bce27101f8dd229c963a33ba7af7e1f3589e6b`; `crates/ascii-platform/tests/host_probe.rs` `ddef24f1fb9e7a509f7a2c0d9e3c4eac4038137d78fa8cd7e5e21615b899c00c`.

These logs establish one attach and clean detach only. They do not verify visual placement below icons, focus/nonactivation, icon behavior, or recovery. No Win+D, Explorer restart, Task View, virtual-desktop, display-topology, lock, or remote-session tests were run.

### User visual acceptance attempt

- The user ran `C:\Users\gskne\AppData\Local\Temp\wnd002-manual-check\wnd002-manual-check.exe`. It printed `READY: synthetic frame presented on the first current display; 180-second hold started.` followed by countdown `179` through `161`; the user pressed Ctrl+C. The runner printed `Graceful Ctrl+C requested; detaching.` and `EXIT: detached and host dropped cleanly.`
- User report: **“no i didnt see anything changed in desktop bg”**. The visible-background gate is **NOT PASSED** for this attempt. `READY` means `host.present(...)` returned without error; it is not evidence that the compositor displayed the frame. Teardown returned cleanly, but visible output was not observed.
- Runner source SHA-256: `8986aa92113818aee0e2dda853d5c1e6699e61fbfacbc62698b00c41e0af78c1`; executable SHA-256: `9e83cda913833cfe4f88d4bd09e5acd833cfc338745bba22acfe1bcf749805d0`; `Cargo.lock` SHA-256: `91bc65be2e89983c92f0157bb124178a147fdd114e04e054dec46a0232e0abf6`; `Cargo.toml` SHA-256: `40e44cc390c1bdfc0f51320e40a7101b7e43f9f8ee3c59b8171f44e1aadbf3c2`.
- Desktop icon click/selection and typing into an application: **NOT TESTED**.

This evidence does not establish why the frame was not visible, a screenshot, focus preservation, or production support.

## Windows test evidence

**Historical local Windows evidence:** `cargo fmt --package ascii-platform -- --check` passed; `cargo clippy --locked --package ascii-platform --all-targets -- -D warnings` passed; `cargo test --locked --package ascii-platform` passed (9 tests across 3 suites). These results describe the earlier source revision and are retained as historical evidence:

```text
cargo test --locked --package ascii-platform
```

The package includes deterministic injected hierarchy tests for an absent shell, changed WorkerW layout, Explorer-folder views excluded from desktop classification, unexpected-parent precedence when recognized and unknown views coexist, an unreadable actual shell-root class, timeout, and core work-budget exhaustion, plus one native read-only report test.

Historical source hashes captured for that earlier check: `lib.rs` `5cbb5ea5a0d2fd07f283050934bce27101f8dd229c963a33ba7af7e1f3589e6b`; `probe.rs` `4546e1bb121cf662a907262c89bf1762c624df1bd3ab46eb8d65b1b0766e25d7`; `probe/windows.rs` `971572e35d0d8c20fbc749b4ace1379f7d50a5de2cfd4da2d7e86e0a8737a5a4`; `tests/host_probe.rs` `ddef24f1fb9e7a509f7a2c0d9e3c4eac4038137d78fa8cd7e5e21615b899c00c`.

No binary or fixture is bundled, so there are no corresponding binary/fixture hashes.

## User-interactive acceptance gates — INCOMPLETE

- Below-icon visible placement: **NOT VERIFIED**; runner attached a surface, but visual placement was not observed.
- Focus/non-activation interaction: **NOT VERIFIED**; no focus test was performed.
- Win+D / Show Desktop: **NOT VERIFIED**.
- Explorer process restart and host-level recovery: **OBSERVED ONCE**; visible Explorer shell recovery remains **NOT VERIFIED** (details below).
- Desktop icon click/selection preservation: **NOT VERIFIED**.

These remain unfinished acceptance gates, not passes or failed placement results. They require user-session interaction and must be recorded separately before any production strategy or support tier is selected.

## Additional disposable WND-002 lifecycle evidence

The following are separate disposable WND-002 integration-runner observations, not outputs or capabilities of the read-only WND-001 hierarchy probe. They do not change the probe's read-only scope or establish visual acceptance, absence of leaks, or production support.

- Host tuple for these runs: Windows 11 Home Single Language, `10.0.26300` / build `26300`, x64, Console session `2`. Inventory: Intel UHD driver `32.0.101.7026`; NVIDIA RTX 3050 laptop driver `32.0.16.2124`. Neither the probe nor runner identified the WGPU adapter/backend that rendered the surface; do not infer which GPU was used.
- Synthetic fixture: 96x54 cell payload SHA-256 `99dfabc5771bbad91bb671529bcae877b06f6e0bbd95d3d94991f430c5febbd2`.

### Twenty attach/present/detach cycles

- Disposable runner source SHA-256 `eaa64c5549be2eb14e9a64f748254bdad2d1ffa0968cde9cdc08194e9c2ff5b4`; executable SHA-256 `e8cbe73b02d43a1793f48a49d740e0d47280f5110a34fe56af116f98ca4850a2`; same `Cargo.lock` SHA-256 as above: `f73dc0c5038492dc8462eb684f0eae85210e4aed1de3204d28029fa91f1ddf7f`.
- WindowsHost completed 20/20 attach/present/detach cycles. Each present returned `Ok(Skipped(Unchanged))`, each detach returned `Ok(())`, and host drop reported owned windows released.
- First attach: `282.0724 ms`; cycles 2–20: `13.2384–16.3314 ms`.
- This run did not separately count HWNDs or measure leaks; no zero-leak claim is supported.

### Win+D attempt — inconclusive

A 45-second monitor began Healthy/revision 6 and ended Healthy/revision 6, with no lifecycle events. A PowerShell action process was started in session 2, but captured output contains only the session/start line and no per-toggle confirmations. Neither successful Win+D keystrokes nor a visible Show Desktop effect was verified. This acceptance gate remains **NOT VERIFIED**.

### Guarded Explorer restart and host recovery — observed once

- Monitor runner source SHA-256 `623300b0fc97bab68f4958e407afcff79522b749adf8f62bb7cfee9d95ee775c`; executable SHA-256 `e519fd3f8ec90e5f9a0816864c2fc17cabb52e3d484e2fc9f7869e54a9043812`; same `Cargo.lock` SHA-256 `f73dc0c5038492dc8462eb684f0eae85210e4aed1de3204d28029fa91f1ddf7f`.
- Watchdog script SHA-256 `b7f1cc73cadb5caba6c6e55f5bf25bbd1ec2b354cecf0f5ce3949b6eaba2024d`; watchdog log SHA-256 `981eacd60098139bdaf0a269f1a16c3ea946d33a76797480ace9cbc721682017`.
- In session 2, `taskkill` exited 0 after terminating Explorer PID `46244` (start `2026-11-24T19:44:19.6222379+05:30`). Watchdog PID `12540` detected exit and launched `%WINDIR%\explorer.exe`; log records replacement Explorer PID `49008` (start `2026-11-27T23:06:32.2492521+05:30`) in session 2.
- WND-002 monitor initially attached at approximately `295.9 ms`, Healthy revision 6. At `10.447734s`, it emitted `SurfaceRetired` with reason `HostLost`; capability became temporarily unavailable and health Unavailable/revision 9. At `12.0259053s`, capability became Available, health Healthy/revision 11, and replacement `SurfaceAttached` emitted with generation 1 and registry_epoch 1.
- After 150 seconds, final snapshot was Healthy revision 12, `WallpaperSurface` Available at Low confidence, lease_count 1. Host drop at `150.0165s` reported owned windows released.

This documents one Explorer process restart and host-level recovery with a replacement lease. It does not verify visible desktop/taskbar recovery or icon interaction; shell visuals remain unknown. No separate HWND/leak count was made. Recovery exhaustion and other session/topology conditions were not tested.

## Remaining acceptance gates — NOT VERIFIED

- Win+D / visible Show Desktop effect: **NOT VERIFIED**.
- Below-icon visible placement and focus/non-activation: **NOT VERIFIED**.
- Desktop icon click/selection and typing into an application: **NOT VERIFIED**.
- Explorer shell visual recovery: **UNKNOWN / NOT VERIFIED**; only process replacement and host-level recovery were observed once.

These observations do not establish a support tier or complete acceptance. Do not merge/close the PR or promote support on this evidence.
