# Build environment and toolchain

## Workspace setup

The workspace pins the tested stable Rust 1.100.0 toolchain in `rust-toolchain.toml`, uses Rust edition 2024 and Cargo resolver 3, and declares MSRV 1.100. This baseline passed the Windows/Linux/macOS core matrix in [PR #288](https://github.com/Astraive/asciiwall/pull/288). Revisit the MSRV if a future dependency requires a newer compiler. [S34](../research/source-register.md#s34)

The core-only check/test subset is implemented and runs without GPU drivers or FFmpeg. Add image, video, native-host, catalog, and profiling subsets only alongside exercised interfaces; do not create empty feature flags or require mutually exclusive `--all-features` builds.

## Development environments

| Target | Required development concerns |
| --- | --- |
| Windows | Rust MSVC target, Windows SDK, compatible FFmpeg import libraries/DLLs, D3D12-capable test machine |
| macOS | Xcode command-line tools/SDK, architecture-matched native libraries, AppKit main-thread integration, signing tests |
| Linux X11 | X11 development/runtime libraries for chosen features, Vulkan driver, desktop-manager test sessions |
| Linux Wayland | Wayland client development/runtime dependencies, selected compositor, protocol bindings, Vulkan driver |
| Plasma/GNOME | Separate supported shell-version environment and integration build/test tooling |

Exact package-install commands belong in platform-specific contributor setup once CI proves them. Do not claim one `apt install` command covers every distribution, or that cross-compilation replaces native desktop testing.

## Verified workspace baseline

The first cross-platform build used rustc/Cargo 1.100.0 and passed on GitHub-hosted Windows Server 2025 (x64), Ubuntu 26.04.1 LTS (x64), and macOS 26.7.2 (ARM64) runners in [PR #288](https://github.com/Astraive/asciiwall/pull/288).

The matrix runs:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo check --locked --package ascii-core --no-default-features
cargo test --locked --package ascii-core --no-default-features
cargo tree --locked --package ascii-core --edges all
```

## Proposed verification commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p ascii-package
cargo test -p ascii-convert
cargo bench -p ascii-convert
cargo run -p xtask -- verify-docs
cargo run -p xtask -- package --target <target-triple>
```

These commands describe intended repository tooling; `xtask` and all application crates still need implementation. Run platform-specific feature subsets rather than an impossible cross-platform `--all-features` combination when features are mutually exclusive.

## Development safety

Use disposable local test fixtures for malformed media. Never run an untrusted package's scripts; packages contain no scripts. Keep signing credentials outside local default environments and inaccessible to pull-request validation. A wallpaper host in debug mode needs an emergency Stop command reachable without its tray.

GPU validation layers and verbose logging are development features. Release builds retain structured failures without logging sensitive input or full media paths. Separate profiling traces from ordinary support bundles.
