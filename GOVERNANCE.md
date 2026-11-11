# Governance

## Governance Model

AsciiWall (`Astraive/asciiwall`) currently operates under a **single-maintainer bootstrap governance model** led by organization owner and repository administrator **`@NeerajCodz`** (`https://github.com/NeerajCodz`).

This model keeps accountability clear while the project moves from its 100-document design baseline through M0 architecture/platform risk spikes and initial Rust implementation.

## Roles and Responsibilities

### Project Maintainer (`@NeerajCodz`)

- Owns repository settings, branch protection, CI trust boundaries, release signing key custody (once configured), and final merge authority on `main`.
- Signs off on milestone gates (`QA-M0` through `QA-M10` per [docs/quality-gates.md](docs/quality-gates.md)) and verifies that no P0 destructive-data, archive-escape, unauthorized-observation, or security failure is waived.
- Triages private security reports under [SECURITY.md](SECURITY.md) and enforces [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
- Coordinates cross-repository contracts with [`Astraive/asciiwall-library`](https://github.com/Astraive/asciiwall-library).

### Contributors

- Submit scoped changes aligned with [CONTRIBUTING.md](CONTRIBUTING.md), [docs/delivery/contributor-workflow.md](docs/delivery/contributor-workflow.md), and [docs/issue-conventions.md](docs/issue-conventions.md).
- Maintain architectural boundaries (offline core, capability-aware adapters, data-only v1 packages), provide truthful test/manual verification evidence, and update affected docs/schemas in the same PR.

## Decision Making and Architectural Changes

- **Architecture Decision Records (ADRs):** substantial shifts to core contracts, native host integration strategies, package formats, or trust boundaries require an ADR under `docs/decisions/` and synchronized updates across affected plans/schemas/tests.
- **Format & Compatibility Contracts:** app, package, scene, clip, catalog, settings, and IPC versions evolve independently per [VERSIONING.md](VERSIONING.md) and [docs/package/compatibility-and-migrations.md](docs/package/compatibility-and-migrations.md).
- **Truthful Status & Support Promotion:** no platform tuple or milestone gate is promoted to `Supported` or `Complete` without recorded verification evidence ([docs/platform/support-policy.md](docs/platform/support-policy.md), [docs/quality/acceptance-criteria.md](docs/quality/acceptance-criteria.md)).

## Review Policy and Bootstrap Branch Protection

- **Protected `main` branch:** changes land through Pull Requests using squash merges, linear history, resolved review conversations, and required passing CI checks ([docs/github-setup.md](docs/github-setup.md)).
- **Dependabot PR triage (`REL-009`):** CI enforces the strict `type(scope): subject [TICKET-ID]` PR title convention without bot bypasses. Because Dependabot cannot add trailing ticket IDs automatically, the maintainer triages automatic dependency PRs and renames the PR title to include `[REL-009]` (or a dedicated security/upgrade ticket ID) before merge, re-running checks on `pull_request` `edited`.
- **Single-maintainer bootstrap exception:** because GitHub does not permit a sole maintainer to approve their own pull requests, required PR approvals are set to `0` during bootstrap. Every PR must still satisfy `.github/CODEOWNERS` review expectations, the structured PR checklist, and required CI checks. Once additional trusted maintainers hold write access, required approvals and strict `CODEOWNERS` enforcement will increase to `>= 1`.
- **High-scrutiny areas:** changes touching untrusted parsers, archive extraction, FFI/unsafe Rust, local IPC, CI workflows, or release/signing logic require explicit security review against [docs/security/threat-model.md](docs/security/threat-model.md).

## Licensing and Repository Separation

- **Code and specification license:** `Astraive/asciiwall` repository files remain licensed under the MIT License ([LICENSE](LICENSE)). Any prospective licensing adjustment for future binaries, bundled fonts, or native codec distributions (such as FFmpeg configure obligations) requires explicit provenance review and notices ([docs/library/licensing-and-attribution.md](docs/library/licensing-and-attribution.md), [docs/release-process.md](docs/release-process.md)).
- **Companion library separation:** `Astraive/asciiwall-library` maintains its own MIT repository foundation alongside explicit per-asset provenance and license records, and consumes pinned compatible engine contracts rather than duplicating the Rust engine.
