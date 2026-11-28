# WND-001 Windows host-probe evidence

## Scope and strategy

The probe performs read-only queries using `GetShellWindow`, bounded `EnumWindows` / `EnumChildWindows`, and `GetWindow` Z-order traversal; it classifies candidate roots and `SHELLDLL_DefView` nodes, then records OS/session identity. Current strategy identifier: `windows-shell-hierarchy-enumeration-v4`.
The combined shell-root, descendant, sibling, and top-level Z-order inspection shares the 512-work-unit ceiling and 250 ms deadline. Unrelated top-level roots are class-checked but not traversed. Time/work exhaustion, missing shell, failed ownership queries, and incomplete/unstable sibling order fail closed as unavailable or unknown fields.
The raised-desktop fields record tri-state style-bit observations (`Some(true)` / `Some(false)` only after a successful query; `None` means the required unique shell-owned window or style query was unavailable). `workerw_immediately_after_desktop_view` is true only when the unique shell-owned top-level Progman and direct-child DefView have the expected style bits, and exactly one shell-owned direct-child WorkerW is verified as the immediate next sibling under that same Progman. A missing/intervening/foreign/duplicate sibling is false; unreadable or unstable evidence is unknown. This is hierarchy evidence, not identification of a wallpaper surface or an attachment recommendation.
Any incomplete enumeration fails closed for the raised-desktop proof fields: the Progman and DefView style booleans and immediate-WorkerW order result are `None`, even if a valid-looking prefix was observed. Partial counts may remain but are non-authoritative.
Microsoft-referenced raised-desktop behavior is quoted in [Lively issue #2074](https://github.com/lively-community/lively/issues/2074): builds 26100+ reportedly use a raised desktop where Progman has `WS_EX_NOREDIRECTIONBITMAP`, DefView is `WS_EX_LAYERED`, and a WorkerW child renders the wallpaper. This is community reporting quoting Microsoft, not a public Windows API contract; the probe independently records only observed style bits and window relationships.
The probe retains class categories/counts, observed style-bit booleans, and order relationships; no HWND or foreign process ID is returned. It does not create a window or graphics device, attach a surface, send shell messages, restart Explorer, or change desktop state. GPU identity is **NOT QUERIED**.


## Recorded non-interactive host observation

The recorded v3 observation predates raised-desktop style and direct-child WorkerW-slot evidence; v3 lacked those style/order fields, which were not captured in that run. Keep these v3 results as historical; do not infer their values from the recorded hierarchy. Interactive acceptance gates remain **NOT RUN**.
The user-supplied Windows 11 build 26300 (x64) v3 probe reported shell=`Progman`; `top_level_progman_count=1`; `top_level_workerw_count=25` in the latest v3 observation (26 in a prior run); and a stable Progman/DefView relationship: one shell-owned `SHELLDLL_DefView` directly parented by `Progman`, with no previous or next sibling and `workerw_sibling_after_desktop_view=Some(false)`. The shell's previous top-level sibling class was `Other`, and its next top-level sibling class was `None`. `first_shell_owned_workerw_after_shell=Some(false)`: this bounded walk found no shell-owned top-level `WorkerW` after `Progman` (candidate absence only, not universal absence). No wallpaper HWND is inferred.

The user-supplied Windows 11 build 26300 (x64) v4 probe reported `shell_window_class=Progman`; `top_level_progman_count=1`; `top_level_workerw_count=17`; one shell-owned DefView direct child of Progman; `progman_no_redirection_bitmap=Some(true)`; `desktop_view_layered=Some(true)`; `workerw_immediately_after_desktop_view=Some(false)`; `shell_owned_progman_child_workerw_count=0`; previous and next DefView sibling classes `None`. Thus no shell-owned direct-child WorkerW slot immediately after DefView was observed. The top-level WorkerW count is an observation from this run and may vary. No 0x052C shell message was sent, no surface was attached, and there was no desktop/window interaction. No wallpaper HWND is inferred.


Hierarchy evidence is not proof of below-icon presentation, compositor visibility, click-through, or stability across Windows builds, sessions, or Explorer restarts.

## Windows test evidence

Verification reported by the maintainer for v4:
```text
cargo +1.100.0 fmt -p ascii-platform --check
cargo +1.100.0 clippy --offline --locked -p ascii-platform --all-targets -- -D warnings
cargo +1.100.0 test --offline --locked -p ascii-platform -- --nocapture
```

All three commands passed. Package tests passed: 50 across 5 suites; 1 test was ignored. The v4 command did not rerun the isolated layered WGPU compatibility test; its earlier v3 run passed on the host-selected adapter. That test uses hidden isolated HWNDs—not a desktop-attached surface—and establishes only that the selected backend could create, configure, render, and present an opaque WGPU surface with the tested layered child styles. It does not establish desktop visibility, below-icon placement, click-through, input/focus behavior, or recovery. Earlier v3 verification, reported by the maintainer (not v4 verification): formatting and strict Clippy passed; package tests passed, 47 total (31 unit, 15 capabilities, 1 host probe), with 1 ignored. The isolated layered WGPU compatibility test passed on v3.

## User-interactive acceptance gates — NOT RUN

- Below-icon visible placement: **NOT RUN**; no desktop-attached surface was created.
- Focus/non-activation interaction: **NOT RUN**; no focus test was performed.
- Win+D / Show Desktop: **NOT RUN**.
- Explorer restart/recovery: **NOT RUN**.
- Desktop icon click/selection preservation: **NOT RUN**.
- Blank-desktop click-through, icon open, desktop context menu, and typing/focus preservation: **NOT RUN**.

These are unfinished acceptance gates, not passes and not failed placement results. They require user-session interaction and must be recorded separately; the existing Progman attachment path remains unverified, with no support or acceptance claim.
