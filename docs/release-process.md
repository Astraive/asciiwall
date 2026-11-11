# Release process, signing, and distribution

> **Current release status:** **No application binary, installer, signed release artifact, SBOM, or published artwork package exists yet.** Neither `Astraive/asciiwall` nor `Astraive/asciiwall-library` has configured production signing key custody, and no release or pre-release has been published.

## Scope and release independence

As defined in [CI and release](delivery/ci-and-release.md), [packaging and signing](delivery/packaging-and-signing.md), [compatibility and migrations](package/compatibility-and-migrations.md), and [library release pipeline](library/release-pipeline-and-distribution.md):
- **`asciiwall` (engine, desktop app, CLI, hosts)** and **`asciiwall-library` (starter artwork recipes, previews, signed static catalog)** release independently.
- Library releases (`library-alpha` at `L1`/`L2`, `library-1.0.0` at `L3`, maintenance at `L4`) pin reviewed, compatible engine/schema versions rather than moving branch tips or creating a runtime fork.
- Every release artifact is accompanied by explicit supported platform tuples, exact codec/backend inventory (`doctor`), checksums, and third-party notices—never blanket OS claims.

## Semantic versioning and draft formats

Version numbers track each compatibility surface independently so incrementing the application version does not implicitly break portable formats:

| Surface | Versioning rule | Current status |
|---|---|---|
| Application (`asciiwall`) | SemVer (`0.x` prototypes/alpha/beta -> `1.0.0` at `M9`/`QA-M9`) | Unimplemented (`Backlog`) |
| `.asciiwall` package format | Independent format SemVer; frozen only at `PKG-012` / `QA-M6` | **Draft 1.0** (not frozen) |
| Scene graph schema | Versioned closed v1 built-in node registry (`PKG-003`, `FX-001`) | **Draft 1.0** |
| `.ascii-clip` binary layout | Header `64`B / index `40`B / cell `12`B + version flags (`VID-007`) | **Draft 1.0** |
| Catalog schema & sequences | Static JSON + monotone sequence numbers (`CAT-001`–`005`) | **Draft 1.0** |
| Local config / IPC protocol | Versioned migrations (`CFG-001`, `SEC-001`) | Proposed contracts |
| Community library (`asciiwall-library`) | Independent SemVer (`library-1.0.0` aligned with app 1.0 at `L3`/`LQA-L3`) | Foundation only (`LICENSE` + `.gitignore`) |

## CI separation and signing key custody

Following `REL-001`, `LQA-001`, and [supply chain and updates](security/supply-chain-and-updates.md):
- **Untrusted PR workflows:** Run formatting, repository/link checks, portable tests, schema checks, and bounded fuzz smoke with `contents: read`, SHA-pinned actions, and **no** production secrets or privileged `pull_request_target` execution ([source S43](research/source-register.md#s43)).
- **Protected release & signing jobs:** Separated from untrusted PR caches and artifacts; require maintainer review/approval on protected tags/commits before signing, uploading immutable assets, or generating build provenance attestations ([source S61](research/source-register.md#s61)).
- **Key custody status:** Production code-signing certificates, Apple Developer ID/notarization credentials, and Ed25519 catalog signing keys (`SEC-008`, `LREL-003`, [source S60](research/source-register.md#s60)) are **currently unconfigured**. Branch protection will not require nonexistent signing checks until real key custody and release workflows exist.
- **Catalog signatures & offline limits:** Catalogs use exact-byte detached signatures (`CAT-005`), monotonically increasing sequence numbers, and expiration metadata; because clients work offline, delayed revocation visibility on disconnected machines remains an explicitly documented security limitation (`EC-093`, [source S44](research/source-register.md#s44)).

## Rights, fonts, and codec notices

Both `Astraive/asciiwall` and `Astraive/asciiwall-library` currently preserve their existing public **MIT** `LICENSE` baseline. Any future release distribution must complete (`LEG-001`, `REL-002`, `LLEG-001`–`004`, [licensing and attribution](library/licensing-and-attribution.md)):
- **Rust & Cargo dependencies:** Committed `Cargo.lock`, license inventory, and `cargo-audit` advisory checks ([source S34](research/source-register.md#s34), [source S62](research/source-register.md#s62) — noting a clean scan is not proof of zero vulnerabilities).
- **FFmpeg & native codecs:** Record exact FFmpeg configure flags, dynamic/static linkage, LGPL vs GPL obligations, and corresponding-source notices ([source S33](research/source-register.md#s33)).
- **Fonts & artwork provenance:** Distinguish built-in redistributable fonts (e.g., SIL OFL reserved-name and notice retention, [source S53](research/source-register.md#s53)) from user local system fonts; distinguish MIT/Apache code from CC0 / CC BY / CC BY-SA asset ledgers ([source S54](research/source-register.md#s54), [source S55](research/source-register.md#s55)). No font binaries or artwork assets are shipped today.

## Platform distribution lifecycles

| Platform | Planned artifacts (`M8` / `M9`) | Key release verification requirements |
|---|---|---|
| **Windows** (`REL-003`) | Per-user installer + explicit writable portable ZIP | Signed binaries/installer; clean-machine DLL/path check; consent-only `.asciiwall` import association; opt-in autostart; no admin privileges |
| **macOS** (`REL-004`) | Signed & notarized app bundle + helpers | Hardened runtime, deliberate entitlements, Developer ID notarization ([source S45](research/source-register.md#s45)), Gatekeeper first launch, login-item removal |
| **Linux** (`REL-005`) | Documented native/portable packages + separate Plasma/GNOME companions | Explicit ABI/FFmpeg/driver expectations; Flatpak host-access boundaries ([source S46](research/source-register.md#s46)); per-user autostart ([source S47](research/source-register.md#s47)) |

## Manual native verification and rollback

Before signing off `QA-M9` (`1.0.0`):
- Execute clean-machine offline install, first launch, conversion/wallpaper run, upgrade, non-destructive downgrade inspection, and uninstall while running / after crash (`REL-006`, `REL-007`).
- Verify uninstall removes owned startup/association entries and stops owned hosts/workers **without deleting referenced user original media** or overwriting an externally changed desktop wallpaper (`EC-090`).
- Verify immutable package versions and prior release hashes remain recoverable if an update fails.

## Private vulnerability reporting

Tracked by `REL-008`, releases publish a private security contact and hash-based advisory policy without silent crash-upload or media collection (`privacy.crash-upload = false` in v1). Emergency release and catalog key-rotation procedures must be rehearsed in a nonproduction namespace (`LREL-003`, `REL-008`) prior to `1.0.0`. See `.github/release.yml` and repository security policy for release-note categories and reporting channels.
