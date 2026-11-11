# Versioning Policy

AsciiWall tracks **application versions and format/protocol contracts independently** so a bug-fix or UI patch never silently breaks saved settings, `.asciiwall` packages, or IPC boundaries. Full release execution, signing key custody, and platform distribution lifecycles are governed by [docs/release-process.md](docs/release-process.md).

## Current Pre-Release Status

- **Application / Engine:** Unreleased (pre-implementation baseline; no `0.x` or `1.0` tags exist yet).
- **Package, Scene, Clip, Catalog, Settings, and IPC formats:** Draft specifications only (`Draft 1.0` / `Draft 1`), not frozen external standards ([docs/package/format-specification.md](docs/package/format-specification.md)).

## Independent Version Streams

Per [docs/package/compatibility-and-migrations.md](docs/package/compatibility-and-migrations.md), AsciiWall separates the following version streams:

| Stream | Scheme | Current State | Stability Rule |
| --- | --- | --- | --- |
| Application & CLI (`asciiwall`) | Semantic Versioning (`MAJOR.MINOR.PATCH[-prerelease]`) | Unreleased | Follows SemVer once `0.1.0` / `1.0.0` begins; breaking CLI or host behavior requires a major bump after `1.0.0`. |
| `.asciiwall` package format | Format version (`1.0` draft) | Draft `1.0` | Frozen only after the format-freeze conformance gate passes across all five source kinds. |
| `scene.json` schema | Schema version (`1.0` draft) | Draft `1.0` | Closed schema in v1; new node kinds require explicit format/feature version updates. |
| `.ascii-clip` binary layout | Integer layout version (`1` draft) | Draft `1` | Binary header versioned independently of app SemVer. |
| Curated catalog schema | Schema version (`1.0` draft) | Draft `1.0` | Verified alongside monotonic sequence numbers and signatures in `Astraive/asciiwall-library`. |
| Settings / config schema | Schema version | Draft | Migrations preserve user backups and never rewrite packages in place. |
| Local IPC protocol | Protocol version | Draft | Controller, host, worker, and CLI negotiate/check explicit envelope compatibility. |

## Pre-Release Identifiers and Format Freeze

1. **Pre-`1.0.0` releases (`0.MINOR.PATCH[-alpha.N|-beta.N|-rc.N]`):** minor releases may adjust internal crate APIs and draft schemas before the format-freeze gate passes, while always documenting breaking changes in [CHANGELOG.md](CHANGELOG.md).
2. **Format-freeze gate:** `.asciiwall` `1.0` is frozen only after reference packer, semantic validator, staged importer, all five source-kind fixtures, cross-platform path conformance (Windows/macOS/Linux), and `Astraive/asciiwall-library` catalog integration agree ([docs/package/compatibility-and-migrations.md#format-freeze-gate](docs/package/compatibility-and-migrations.md#format-freeze-gate)).
3. **Compatibility vs. validity:** a syntactically valid package can still report incompatibility with an older engine, disabled codec build, missing font rights, or unavailable desktop host capability — incompatible items must explain why rather than silently altering behavior.
4. **Downgrade and rollback safety:** rolling back an application binary must not destructively overwrite newer configuration or package data; read-only inspection and safe fallback remain available where feasible.
5. **Library repository cadence:** `Astraive/asciiwall-library` releases (`L0`–`L4` milestones and static catalog snapshots) are versioned independently and pin explicit compatible engine releases/schema digests ([docs/library/catalog-and-index-schema.md](docs/library/catalog-and-index-schema.md), [docs/release-process.md#scope-and-release-independence](docs/release-process.md#scope-and-release-independence)).
