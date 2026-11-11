# Continuous integration and release engineering

## CI layers

| Pipeline | Trust level | Checks |
|---|---|---|
| Pull request | Untrusted | Formatting, lints, portable tests, schema/fixture checks, license/advisory scan, bounded fuzz smoke |
| Native build matrix | Untrusted or isolated reviewed code | Windows/macOS/Linux compile, codec linkage, feature combinations |
| Protected integration | Reviewed | Host smoke scripts and broader GPU/media tests on controlled machines |
| Release preparation | Trusted protected commit | Reproducible builds, SBOM/notices, artifact inventory, clean-machine install tests |
| Signing/publication | Restricted trusted job | Verify artifact hashes/provenance, sign, publish, attest |
| Library release | Separate trusted workflow | Pinned packer, rights-approved assets, package validation, signed static catalog |

Untrusted PRs never receive signing keys or release-write credentials. Do not execute PR checkout with privileged `pull_request_target`. [S43](../research/source-register.md#s43) Separate caches/artifact channels by trust context and revalidate what enters signing.

## Suggested development commands

These are future-repository commands after the relevant crates/tests exist:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p ascii-package --test archive_import
cargo test -p ascii-media --test clip_codec
cargo test -p ascii-config --test settings_resolution
```

Add feature-matrix jobs rather than blindly assuming `--all-features` is meaningful when native backends are mutually exclusive. Advisory/license tools require their own pinned installation and policy. Known-advisory scanning is useful but not proof of absence of vulnerabilities. [S62](../research/source-register.md#s62)

## Build reproducibility

Pin Rust toolchain, Cargo.lock, native codec source/configuration, target triples, environment-sensitive build inputs, and packaging tools. Record hashes and features in a release manifest. Source timestamp/absolute-path normalization can matter for reproducibility; measure rather than promise byte-identical binaries across arbitrary systems.

Do not reference moving branch names as production dependencies. Review graphics family updates together so wgpu/egui/raw-window-handle types remain coherent. [S25](../research/source-register.md#s25) [S34](../research/source-register.md#s34)

## Test artifacts

Retain machine-readable test/benchmark reports, sanitized native test evidence, golden diffs, schema versions, and failure logs with bounded retention. Never upload user media or personal window/input traces as CI artifacts. Use small rights-cleared fixtures.

Build attestations can establish provenance of produced artifacts, not that their code or artwork is harmless. [S61](../research/source-register.md#s61) Document how a user verifies the downloaded release and its checksum/signature.

## Release checklist

Version and changelog match; all in-scope P0 gates pass; support matrix names tested tuples; package/scene/clip/IPC/config versions agree; dependency and license inventory complete; app/helpers signed where required; offline clean-machine paths pass; downgrade behavior documented; recovery and uninstall tested; catalog keys protected.

A failed platform job may result in that platform remaining experimental or omitted, not an unqualified all-platform release. Security or destructive-data failures cannot be waived by relabelling a build 'beta'.
