# Desktop Implementation Plan

> **For agentic workers:** Use a task-by-task execution/review workflow; where available, use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. The plan also works as a human implementation checklist.

**Goal:** Deliver the desktop work units without weakening the shared contracts.

**Architecture:** Rust libraries own portable algorithms and validation. Executables and native adapters compose them through bounded, versioned contracts; no hosted backend is introduced.

**Tech stack:** Rust Cargo workspace, wgpu/egui where applicable, image/fontdue, replaceable FFmpeg binding, native OS APIs, schema/ZIP tools and local IPC.

**Spec:** [System overview](../architecture/system-overview.md), [data contracts](../architecture/data-model-and-contracts.md), [package specification](../package/format-specification.md), and the linked subsystem documents.

## Global constraints

Stable package format is draft 1.0; it is not frozen until conformance passes. Core operation is offline. Control IPC is capped at 1 MiB. Decoded frame queues are bounded to two or three. Package archives are at most 1 GiB compressed and 2 GiB actually expanded. All visual inputs and work are resource-bounded. Packages cannot grant permissions or execute arbitrary code. No native platform is supported solely because it compiles.

All signatures below are target interfaces. Define missing types in the listed owning task, reusing the core models rather than duplicating them. Paths are future application-repository paths, not files implemented in this documentation bundle. The CLI crate's proposed package name is `ascii-cli`; the build helper package is `xtask`.

## Review focus

Untrusted paths and nested I/O: tests P03/H02 deny filesystem/network escape.

Stale async completion after Apply/seek: tests E09/D10 reject old generations.

Mixed coordinate spaces and missing observer support: tests D02/D08/D09 use explicit unknown states.

Resource totals across displays/transitions: tests E09/H03 enforce bounded aggregate usage.

User control after shell/worker failure: tests D03/D07/H05 restore a stoppable safe state.

## File ownership

The [repository layout](../architecture/repository-structure.md) defines crate boundaries. Each task below lists its production and test files. Core models are owned by E01; glyph models E02; media ownership E05; platform capabilities D02; settings P01; package contracts P02; IPC H01. Do not create shadow types with incompatible semantics in other crates.

## D01: Native hosting feasibility spikes

**Files:** Create or modify `crates/ascii-platform/src/probe.rs; tests/platform/host-probe.md`. Test file: `crates/ascii-platform/tests/host_probe.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01; defines documented supported/experimental/unavailable observations, not a production host.

**Produces:** `probe_host() -> HostProbeReport`

**Dependencies:** E01.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/host_probe.rs`. Assertions: On a named Windows build, clear-color background stays below icons through Win+D and Explorer restart or records a failed gate; no focus steal.
- [ ] Run `cargo test -p ascii-platform --test host_probe -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test host_probe -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D02: Platform capability and surface contracts

**Files:** Create or modify `crates/ascii-platform/src/{host.rs,capability.rs,events.rs}`. Test file: `crates/ascii-platform/tests/capabilities.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 D01; defines availability/reason/scope/freshness and surface generations.

**Produces:** `HostAdapter::capabilities(&self) -> CapabilitySnapshot; HostAdapter::poll(&mut self) -> Vec<HostEvent>`

**Dependencies:** E01 D01.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/capabilities.rs`. Assertions: Unknown is distinct from empty/zero; permission loss revokes only affected features; stale surface generation cannot receive a present request.
- [ ] Run `cargo test -p ascii-platform --test capabilities -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test capabilities -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D03: Windows production host

**Files:** Create or modify `crates/ascii-platform/src/windows/{desktop.rs,events.rs}`. Test file: `crates/ascii-platform/tests/windows_host.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E06; defines SurfaceLease lifetime and verified placement/cleanup.

**Produces:** `WindowsHost::attach(&mut self, display: &DisplayInfo) -> Result<SurfaceLease, Error>`

**Dependencies:** D02 E06.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/windows_host.rs`. Assertions: 20 attach/detach cycles; Explorer restart; mixed-DPI coordinates; lock/secure desktop stop; failed placement detaches rather than becoming topmost.
- [ ] Run `cargo test -p ascii-platform --test windows_host -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test windows_host -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D04: macOS host

**Files:** Create or modify `crates/ascii-platform/src/macos/{desktop.rs,spaces.rs}`. Test file: `crates/ascii-platform/tests/macos_host.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E06; AppKit main-thread ownership and explicit collection behavior.

**Produces:** `MacHost::attach(&mut self, display: &DisplayInfo) -> Result<SurfaceLease, Error>`

**Dependencies:** D02 E06.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/macos_host.rs`. Assertions: Finder icons, Spaces/Mission Control, permission denial, Retina/non-Retina and repeated close/quit are tested on named versions; no blanket flags.
- [ ] Run `cargo test -p ascii-platform --test macos_host -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test macos_host -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D05: Named X11 host combinations

**Files:** Create or modify `crates/ascii-platform/src/x11/{desktop.rs,events.rs}`. Test file: `crates/ascii-platform/tests/x11_host.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E06; tested EWMH/input-shape behavior and disconnect cleanup.

**Produces:** `X11Host::attach(&mut self, display: &DisplayInfo) -> Result<SurfaceLease, Error>`

**Dependencies:** D02 E06.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/x11_host.rs`. Assertions: Desktop-icon provider does not hide incorrectly stacked surface; no grabs; workspace/root-coordinate mapping correct; X connection loss exits cleanly.
- [ ] Run `cargo test -p ascii-platform --test x11_host -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test x11_host -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D06: Layer-shell Wayland host

**Files:** Create or modify `crates/ascii-platform/src/wayland/{layer.rs,output.rs}`. Test file: `crates/ascii-platform/tests/wayland_host.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E06; configure/frame/output handling and explicit observer absence.

**Produces:** `WaylandHost::attach(&mut self, display: &DisplayInfo) -> Result<SurfaceLease, Error>`

**Dependencies:** D02 E06.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/wayland_host.rs`. Assertions: Missing protocol returns unavailable; empty input region receives no falsely promised clicks; zero configure size pauses; scale changes rebuild transforms.
- [ ] Run `cargo test -p ascii-platform --test wayland_host -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test wayland_host -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D07: Plasma/GNOME bridge experiments

**Files:** Create or modify `integrations/{plasma,gnome}/; crates/ascii-platform/src/bridge.rs`. Test file: `crates/ascii-platform/tests/bridge_contract.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E06 H01; defines versioned bounded presenter/metadata bridge.

**Produces:** `validate_bridge_frame(message: &BridgeFrame, limits: &Budget) -> Result<(), Error>`

**Dependencies:** D02 E06 H01.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/bridge_contract.rs`. Assertions: Measure copies and shell-thread stalls; controller loss/extension disable cleans actors/resources; incompatible protocol rejected; no decoder inside shell.
- [ ] Run `cargo test -p ascii-platform --test bridge_contract -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test bridge_contract -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D08: Passive pointer and click effects

**Files:** Create or modify `crates/ascii-scene/src/pointer.rs; crates/ascii-platform/src/input.rs`. Test file: `crates/ascii-scene/tests/pointer_reactions.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E07 P01; typed spaces, consent and scope filtering.

**Produces:** `normalize_pointer(event: HostPointerEvent, transform: &OutputTransform) -> Option<PointerEvent>`

**Dependencies:** D02 E07 P01.

- [ ] Write the failing contract test in `crates/ascii-scene/tests/pointer_reactions.rs`. Assertions: 1000 Hz motion coalesces; unknown desktop hit suppresses desktop-only reaction; clicks are never replayed/consumed; permission revocation clears history.
- [ ] Run `cargo test -p ascii-scene --test pointer_reactions -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-scene --test pointer_reactions -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D09: Window metadata and occlusion policy

**Files:** Create or modify `crates/ascii-runtime/src/{windows.rs,occlusion.rs}`. Test file: `crates/ascii-runtime/tests/window_policy.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E09 P01; confidence-aware union geometry and pause hysteresis.

**Produces:** `coverage(snapshot: &WindowSnapshot, output: &OutputRect) -> CoverageEstimate`

**Dependencies:** D02 E09 P01.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/window_policy.rs`. Assertions: Two overlapping 60-percent rectangles do not report 120 percent; stale windows decay; handle reuse safe; state-only Wayland lacks geometry effects.
- [ ] Run `cargo test -p ascii-runtime --test window_policy -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test window_policy -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D10: Transactional Apply and transitions

**Files:** Create or modify `crates/ascii-runtime/src/{assignment.rs,transition.rs}`. Test file: `crates/ascii-runtime/tests/assignment_transaction.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E06 E07 E09 P01; resource preparation, newest-wins generation and commit.

**Produces:** `apply(candidate: AssignmentDraft, expected_revision: u64) -> Result<ApplyJob, Error>`

**Dependencies:** E06 E07 E09 P01.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/assignment_transaction.rs`. Assertions: Bad candidate retains old wallpaper; at most two presentation states; different atlases survive overlap; lock/force-stop skips cosmetic delay.
- [ ] Run `cargo test -p ascii-runtime --test assignment_transaction -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test assignment_transaction -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D11: Displays, power and session lifecycle

**Files:** Create or modify `crates/ascii-runtime/src/{outputs.rs,power.rs,session.rs}`. Test file: `crates/ascii-runtime/tests/display_lifecycle.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D02 E09 D10; per-output transforms, parking, pause priorities and recovery.

**Produces:** `reconcile_outputs(old: &OutputState, new: &HostSnapshot) -> ReconcilePlan`

**Dependencies:** D02 E09 D10.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/display_lifecycle.rs`. Assertions: Hotplug during Apply cannot target dead surface; ambiguous display ID requests remap; battery hysteresis stable; suspend never catches up hours of frames.
- [ ] Run `cargo test -p ascii-runtime --test display_lifecycle -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test display_lifecycle -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## D12: Native editor vertical slice

**Files:** Create or modify `crates/ascii-ui/src/{library.rs,editor.rs,preview.rs,settings.rs}; apps/desktop/src/app.rs`. Test file: `crates/ascii-ui/tests/editor_model.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D10 P01 P05; draft/undo/effective settings and shared-engine preview.

**Produces:** `EditorModel::dispatch(&mut self, action: EditorAction) -> Vec<UiCommand>`

**Dependencies:** D10 P01 P05.

- [ ] Write the failing contract test in `crates/ascii-ui/tests/editor_model.rs`. Assertions: Keyboard-driven import-preview-apply-stop-export flow; Cancel preserves saved assignment; output switch cannot apply a stale draft; missing capability explained.
- [ ] Run `cargo test -p ascii-ui --test editor_model -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-ui --test editor_model -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## Completion evidence

Record targeted command output, source revision, fixture hashes, any native test environment, and accepted limitations. A local compile does not prove wallpaper layering, sandbox confinement, energy use, or accessibility. Promote only the behaviors actually verified and update the backlog/support matrix accordingly.
