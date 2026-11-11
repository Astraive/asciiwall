# Packages Implementation Plan

> **For agentic workers:** Use a task-by-task execution/review workflow; where available, use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. The plan also works as a human implementation checklist.

**Goal:** Deliver the packages work units without weakening the shared contracts.

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

## P01: Settings registry and migrations

**Files:** Create or modify `crates/ascii-config/src/{registry.rs,resolve.rs,migrate.rs}`. Test file: `crates/ascii-config/tests/settings_resolution.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E01; defines dotted keys/types, requested/effective values, revisions and migrations.

**Produces:** `resolve_settings(layers: &SettingLayers, constraints: &Constraints) -> Result<EffectiveSettings, Error>`

**Dependencies:** E01.

- [ ] Write the failing contract test in `crates/ascii-config/tests/settings_resolution.rs`. Assertions: Package cannot set privacy keys; nonfinite/inverted thresholds rejected; battery cap does not overwrite requested FPS; interrupted write preserves prior config.
- [ ] Run `cargo test -p ascii-config --test settings_resolution -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-config --test settings_resolution -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P02: Manifest and scene semantic validator

**Files:** Create or modify `crates/ascii-package/src/{manifest.rs,schema.rs,semantic.rs}`. Test file: `crates/ascii-package/tests/manifest_validation.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E07 P01; defines closed schema, actual inventory and feature/capability checks.

**Produces:** `validate_manifest(bytes: &[u8], inventory: &AssetInventory) -> Result<ValidatedManifest, Error>`

**Dependencies:** E07 P01.

- [ ] Write the failing contract test in `crates/ascii-package/tests/manifest_validation.rs`. Assertions: Positive fixture passes; duplicate keys/unknown fields fail; missing source/observer declaration fails; wrong graph arity/cycle rejected.
- [ ] Run `cargo test -p ascii-package --test manifest_validation -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-package --test manifest_validation -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P03: Safe staged archive importer

**Files:** Create or modify `crates/ascii-package/src/{archive.rs,extract.rs,install.rs}`. Test file: `crates/ascii-package/tests/archive_import.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P02 H02; validates entries/actual limits and atomic commit before indexing.

**Produces:** `import_archive(input: &mut dyn ReadSeek, target: &LibraryRoot, cancel: &CancellationToken) -> Result<ImportReceipt, Error>`

**Dependencies:** P02 H02.

- [ ] Write the failing contract test in `crates/ascii-package/tests/archive_import.rs`. Assertions: Traversal/link/collision/bomb fixtures rejected without outside writes; cancel/disk-full keeps old library; same ID/version differing hash returns conflict.
- [ ] Run `cargo test -p ascii-package --test archive_import -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-package --test archive_import -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P04: Deterministic portable packer/exporter

**Files:** Create or modify `crates/ascii-package/src/{pack.rs,export.rs}`. Test file: `crates/ascii-package/tests/package_roundtrip.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P02 P03; defines inventory generation and deterministic archive profile.

**Produces:** `pack(source: &PackageSource, output: &Path, options: &PackOptions) -> Result<PackageDigest, Error>`

**Dependencies:** P02 P03.

- [ ] Write the failing contract test in `crates/ascii-package/tests/package_roundtrip.rs`. Assertions: Two identical builds match bytes; hash is external to manifest; clean import requires no creator paths; attribution preserved and private settings excluded.
- [ ] Run `cargo test -p ascii-package --test package_roundtrip -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-package --test package_roundtrip -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P05: Local immutable library index

**Files:** Create or modify `crates/ascii-library/src/{index.rs,variants.rs,rebuild.rs}`. Test file: `crates/ascii-library/tests/local_index.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P03; defines local entries, variants, versions and rebuildable index.

**Produces:** `Library::install_receipt(&mut self, receipt: ImportReceipt) -> Result<LibraryEntry, Error>`

**Dependencies:** P03.

- [ ] Write the failing contract test in `crates/ascii-library/tests/local_index.rs`. Assertions: Duplicate hash deduplicates; index rebuild preserves packages/overrides; removing managed entry never deletes referenced original; active dependency blocks unsafe delete.
- [ ] Run `cargo test -p ascii-library --test local_index -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-library --test local_index -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P06: Optional signed catalog client

**Files:** Create or modify `crates/ascii-library/src/{catalog.rs,trust.rs,download.rs}`. Test file: `crates/ascii-library/tests/catalog_trust.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P04 P05; defines exact signed bytes, allowlisted downloads and stale/rollback policy.

**Produces:** `verify_catalog(bytes: &[u8], signature: &Envelope, trust: &TrustStore) -> Result<TrustedCatalog, Error>`

**Dependencies:** P04 P05.

- [ ] Write the failing contract test in `crates/ascii-library/tests/catalog_trust.rs`. Assertions: Bad key/signature/hash rejects; old sequence/expired metadata handled; redirect denied; disabled network leaves all local flows functional.
- [ ] Run `cargo test -p ascii-library --test catalog_trust -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-library --test catalog_trust -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P07: Community repository build pipeline

**Files:** Create or modify `asciiwall-library/scripts/; asciiwall-library/.github/workflows/`. Test file: `crates/ascii-library/tests/catalog_build.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** P04 P06 E11; consumes pinned engine packer and reviewed asset ledger.

**Produces:** `build_catalog(validated_artifacts: &[ArtifactReceipt]) -> Result<CatalogDocument, Error>`

**Dependencies:** P04 P06 E11.

- [ ] Write the failing contract test in `crates/ascii-library/tests/catalog_build.rs`. Assertions: Catalog derives real hashes/sizes; no PR secrets; arbitrary contribution scripts not executed; incomplete license blocks curated publication.
- [ ] Run `cargo test -p ascii-library --test catalog_build -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-library --test catalog_build -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## P08: CLI command and error contract

**Files:** Create or modify `apps/cli/src/{main.rs,convert.rs,package.rs,control.rs,error.rs}`. Test file: `apps/cli/tests/cli_contract.rs`. Any necessary crate manifest/setup belongs to this unit, not an untestable separate scaffold ticket.

**Consumes and owns:** E11 P04 P05 H01; sets proposed package name ascii-cli and machine output contract.

**Produces:** `run_cli(args: impl Iterator<Item=OsString>) -> ExitCode`

**Dependencies:** E11 P04 P05 H01.

- [ ] Write the failing contract test in `apps/cli/tests/cli_contract.rs`. Assertions: JSON stdout clean; binary+JSON conflict rejected; stable exits; Ctrl+C cleanup; stale revision error; paths never interpolated into shell.
- [ ] Run `cargo test -p ascii-cli --test cli_contract -- --nocapture` and confirm the new assertion fails for the intended missing behavior, not an unrelated environment/setup failure. Native-only evidence uses an ignored/feature-gated test plus the named manual script; do not fabricate a CI pass.
- [ ] Implement the stated interface in the owned files with validated inputs, explicit error categories, cancellation/resource limits where relevant, and no package-controlled OS/network actions.
- [ ] Run `cargo test -p ascii-cli --test cli_contract -- --nocapture` again, then affected workspace regressions. Expected: all applicable assertions pass; record unsupported native environments separately, not as success.
- [ ] Review the diff and commit this independently testable unit with its fixtures/spec changes. Do not amend a failing unrelated test merely to make the suite green.

## Completion evidence

Record targeted command output, source revision, fixture hashes, any native test environment, and accepted limitations. A local compile does not prove wallpaper layering, sandbox confinement, energy use, or accessibility. Promote only the behaviors actually verified and update the backlog/support matrix accordingly.
