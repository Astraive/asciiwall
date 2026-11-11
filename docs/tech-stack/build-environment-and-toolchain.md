# Build environment and toolchain

## Workspace setup

Use the stable Rust toolchain pinned in `rust-toolchain.toml` after the first multi-platform compile. Rust edition 2024 and resolver 3 are reasonable starting candidates, subject to the selected dependency family's MSRV. A virtual workspace must specify its resolver explicitly. [S34](../research/source-register.md#s34)

Keep ordinary engine tests independent of GPU drivers and FFmpeg. Feature flags should permit a core-only build, image conversion, video support, individual native hosts, optional remote-catalog access, and developer-only profiling. Do not require every OS dependency for every target.

## Development environments

| Target | Required development concerns |
| --- | --- |
| Windows | Rust MSVC target, Windows SDK, compatible FFmpeg import libraries/DLLs, D3D12-capable test machine |
| macOS | Xcode command-line tools/SDK, architecture-matched native libraries, AppKit main-thread integration, signing tests |
| Linux X11 | X11 development/runtime libraries for chosen features, Vulkan driver, desktop-manager test sessions |
| Linux Wayland | Wayland client development/runtime dependencies, selected compositor, protocol bindings, Vulkan driver |
| Plasma/GNOME | Separate supported shell-version environment and integration build/test tooling |

Exact package-install commands belong in platform-specific contributor setup once CI proves them. Do not claim one `apt install` command covers every distribution, or that cross-compilation replaces native desktop testing.

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
