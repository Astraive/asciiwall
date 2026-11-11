# Hardening Implementation Plan

> **For agentic workers:** Use a task-by-task execution/review workflow; where available, use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. The plan also works as a human implementation checklist.

**Goal:** Deliver the hardening work units without weakening the shared contracts.

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

## H01: Authenticated local IPC and supervision

**Files:** Create or modify `crates/ascii-ipc/src/{envelope.rs,peer.rs,handles.rs}; apps/desktop/src/supervisor.rs`. Test file: `crates/ascii-ipc/tests/ipc_contract.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01 D02; defines per-user/session protocol, 1 MiB control cap and generations.

**Produces:** `accept_session(peer: PeerIdentity, hello: ClientHello) -> Result<Session, Error>`

**Dependencies:** E01 D02.

- [ ] Write the failing contract test in `crates/ascii-ipc/tests/ipc_contract.rs`. Assertions: Wrong peer/version/oversized message denied; duplicate request idempotent; controller death stops hosts; invalid shared buffer never consumed.
- [ ] Run `cargo test -p ascii-ipc --test ipc_contract -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-ipc --test ipc_contract -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H02: Restricted media-worker runner

**Files:** Create or modify `apps/media-worker/src/{main.rs,limits.rs}; crates/ascii-media/src/worker.rs`. Test file: `crates/ascii-media/tests/worker_confinement.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E05 E08 H01; approved handles plus OS-specific restrictions and explicit unsupported state.

**Produces:** `spawn_restricted(request: DecodeRequest, policy: WorkerPolicy) -> Result<WorkerLease, Error>`

**Dependencies:** E05 E08 H01.

- [ ] Write the failing contract test in `crates/ascii-media/tests/worker_confinement.rs`. Assertions: Unrelated file read/write and network fail; CPU/memory timeout terminates worker; child cannot spawn unrestricted helpers; main UI survives crash.
- [ ] Run `cargo test -p ascii-media --test worker_confinement -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-media --test worker_confinement -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H03: Adaptive quality and aggregate budgets

**Files:** Create or modify `crates/ascii-runtime/src/{quality.rs,budget_manager.rs}`. Test file: `crates/ascii-runtime/tests/quality_policy.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E12 D11; global resource accounting and gradual reversible quality reduction.

**Produces:** `QualityPolicy::update(&mut self, metrics: &RuntimeMetrics) -> QualityDecision`

**Dependencies:** E12 D11.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/quality_policy.rs`. Assertions: Four outputs share total cap; sustained overload reduces quality; hysteresis avoids oscillation; requested settings retained; static source stops work.
- [ ] Run `cargo test -p ascii-runtime --test quality_policy -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test quality_policy -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H04: Accessibility and localization gate

**Files:** Create or modify `crates/ascii-ui/src/{accessibility.rs,locale.rs}; tests/platform/accessibility.md`. Test file: `crates/ascii-ui/tests/accessibility_model.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D12; named controls, keyboard focus and reduced-motion integration.

**Produces:** `UiAccessibilityModel::validate(&self) -> Vec<AccessibilityIssue>`

**Dependencies:** D12.

- [ ] Write the failing contract test in `crates/ascii-ui/tests/accessibility_model.rs`. Assertions: Screen-reader/keyboard flows pass on named hosts; long/RTL labels usable; reduced motion changes during transition; no focus trap.
- [ ] Run `cargo test -p ascii-ui --test accessibility_model -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-ui --test accessibility_model -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H05: Diagnostics and recovery tools

**Files:** Create or modify `crates/ascii-runtime/src/{diagnostics.rs,recovery.rs}; apps/cli/src/doctor.rs`. Test file: `crates/ascii-runtime/tests/diagnostics_redaction.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D11 H01; bounded logs, safe mode and explicit local report export.

**Produces:** `redact(snapshot: DiagnosticSnapshot, policy: RedactionPolicy) -> DiagnosticReport`

**Dependencies:** D11 H01.

- [ ] Write the failing contract test in `crates/ascii-runtime/tests/diagnostics_redaction.rs`. Assertions: Default report contains no titles/input paths/source pixels; crash loop opens safe mode; cache clear preserves originals; tray-less controls recover.
- [ ] Run `cargo test -p ascii-runtime --test diagnostics_redaction -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-runtime --test diagnostics_redaction -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H06: Packaging, signing and release CI

**Files:** Create or modify `xtask/src/{package.rs,release.rs}; packaging/; .github/workflows/`. Test file: `xtask/tests/release_manifest.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P08 H02 H04 H05; exact native deps, signatures/notices and clean-machine smoke tests.

**Produces:** `verify_release(manifest: &ReleaseManifest) -> Result<ReleaseReceipt, Error>`

**Dependencies:** P08 H02 H04 H05.

- [ ] Write the failing contract test in `xtask/tests/release_manifest.rs`. Assertions: Installer includes helpers/codecs/notices; uninstall removes owned startup only; signing validated; no mutable dependencies; clean offline launch works.
- [ ] Run `cargo test -p xtask --test release_manifest -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p xtask --test release_manifest -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H07: Fuzz and regression corpus

**Files:** Create or modify `fuzz/fuzz_targets/; tests/fixtures/; crates/ascii-package/tests/regression_corpus.rs`. Test file: `crates/ascii-package/tests/regression_corpus.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E10 P03 H01; parser/clip/IPC seeds, sanitizers and minimized regressions.

**Produces:** `validate_regression_corpus(fixtures: &[RegressionFixture]) -> RegressionReport`

**Dependencies:** E10 P03 H01.

- [ ] Write the failing contract test in `crates/ascii-package/tests/regression_corpus.rs`. Assertions: All 120 matrix cases assigned evidence owners; known malformed samples stay rejected; bounded fuzz jobs do not escape sandbox; regressions minimized.
- [ ] Run `cargo test -p ascii-package --test regression_corpus -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-package --test regression_corpus -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## H08: Native support promotion

**Files:** Create or modify `tests/platform/; docs/platform/support-policy.md`. Test file: `crates/ascii-platform/tests/release_support_matrix.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** D03 D04 D05 D06 D07 H03 H06 H07; per-host pass/fail matrix, not blanket parity.

**Produces:** `promote_support(evidence: &NativeEvidenceSet) -> Result<SupportClaim, Error>`

**Dependencies:** D03 D04 D05 D06 D07 H03 H06 H07.

- [ ] Write the failing contract test in `crates/ascii-platform/tests/release_support_matrix.rs`. Assertions: Only tested tuples promoted; all relevant P0 cases pass; unsupported observers shown; benchmark claims backed by named runs; rollback documented.
- [ ] Run `cargo test -p ascii-platform --test release_support_matrix -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-platform --test release_support_matrix -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## Completion evidence

Record targeted command output, source revision, fixture hashes, any native test environment, and accepted limitations. A local compile does not prove wallpaper layering, sandbox confinement, energy use, or accessibility. Promote only the behaviors actually verified and update the backlog/support matrix accordingly.
