# Dependency selection and version policy

## Research snapshot, not a lockfile

On 2026-11-11 the published metadata inspected for `egui-wgpu 0.37.0` listed `wgpu ^30.0` and optional `winit ^0.30.13`. The FFmpeg wrapper pages showed `ffmpeg-the-third 6.0.0+ffmpeg-9.0` and `ffmpeg-next 9.0.0`. These are observations, not recommendations to copy version numbers blindly into a manifest. [S25](../research/source-register.md#s25) [S29](../research/source-register.md#s29) [S30](../research/source-register.md#s30)

Choose a coherent graphics/UI family first, then compile it on every target. Do not independently select the newest version of each crate and assume GPU callback types or raw-window-handle versions match. Pin the resolved application graph in `Cargo.lock` and document the minimum supported Rust version after actual builds.

## Binding selection gate

Compare both FFmpeg wrappers on current ABI coverage, supported FFmpeg versions, build scripts, error propagation, frame ownership, unsafe surface, cross-compilation, published issue activity, and security response. The description of one fork calling another abandoned is not sufficient maintenance evidence. Keep all binding-specific types inside `ascii-media`.

Record the exact FFmpeg source revision, build configuration, enabled codecs/demuxers/protocols, compiler, library hashes, licenses, and patches in release metadata. A Rust wrapper version alone cannot establish native-library compatibility or legal obligations. [S33](../research/source-register.md#s33)

## Policy

Application crates share workspace dependencies. Use explicit feature flags and disable unnecessary codecs, network protocols, default backends, or optional integrations. Do not pull `latest`, a moving Git branch, or wildcard production versions. Toolchain updates and dependency updates get dedicated pull requests with lockfile diffs and platform tests.

## Current executable dependency pins

`ascii-convert` pins `fontdue = "=0.9.3"` with default features disabled and only `hashbrown` enabled; the default SIMD and parallel features are not active. It pins `sha2 = "=0.10.9"` for glyph-table identity. The workspace `Cargo.lock` records the resolved transitive graph. These pins describe the current glyph-rasterization boundary, not a complete application dependency set or a cross-platform support claim.

Maintain an SBOM and third-party notices for each distributed artifact. Scan Rust and native dependencies; an absence of Rust advisories does not establish FFmpeg or system-library safety. An urgent security update can override normal release cadence, but must still run package parsing and platform smoke tests.

## Compatibility record

For every accepted dependency family, record Rust compiler version, target triples, graphics backends, native media ABI, system package prerequisites, enabled features, known limitations, and build commands. Store this record beside CI results. Version ranges in a research document do not replace that evidence.

Prefer versioned documentation links in release notes. When consulting a `latest` documentation URL, record the resolved version and date because its contents can change.
