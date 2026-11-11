# Technology stack and alternatives

## Recommended stack

| Area | Proposed choice | Reason and boundary |
| --- | --- | --- |
| Language/build | Rust, Cargo workspace | One core for app, CLI, tooling, and renderer |
| GPU abstraction | `wgpu` + first-party WGSL | Glyph instancing, compositing, later measured compute acceleration |
| Settings UI | `egui`, `egui-wgpu`, `egui-winit` | Rust-native control surface with the same rendering ecosystem |
| Ordinary windows | `winit` | UI/preview windows; not the universal wallpaper-surface factory |
| Font masks | `fontdue` | Deterministic glyph rasterization for a restricted monospace repertoire |
| Raster codecs | `image` with selected features | Local input/output, explicit decoder limits |
| Video codecs | FFmpeg behind `Decoder` abstraction | Mature demux/codec support; conversion stays our Rust code |
| FFmpeg binding | Initial candidate `ffmpeg-the-third`; retain replaceability | Must pass ABI/build/security review against `ffmpeg-next` alternative |
| CPU work | `rayon` with explicit pool size | Parallel independent cells/tiles; not uncontrolled nested pools |
| Queues | `crossbeam-channel` or equivalent bounded channels | Backpressure and generation-aware cancellation |
| Settings | `serde`, `toml`, `serde_json` | Human-readable local config and portable package contracts |
| Schema checks | `jsonschema` or equivalent reviewed validator | Strict bundled schemas; no remote `$ref` resolution |
| Packages | `zip`, SHA-256 implementation, optional `zstd` for clips | Safe archive transport and deterministic content identity |
| CLI | `clap` | Explicit subcommands and machine-readable output |
| Errors/logs | `thiserror`, `tracing` | Typed failures and redacted structured diagnostics |
| File dialogs | `rfd` | Native user-mediated local file selection |
| Tray | `tray-icon`, backend selected per OS | Optional convenience; not the only control entry point |
| Windows APIs | `windows` | Win32, DWM, display, input, shell metadata |
| macOS APIs | `objc2`, `objc2-app-kit`, CoreGraphics bindings | Main-thread AppKit and carefully scoped native observation |
| X11 | `x11rb` | X connection, properties, supported extension observation |
| Wayland | `wayland-client`, `smithay-client-toolkit` | Native protocol negotiation and layer-shell surfaces |
| Local paths | `directories` or equivalent | Per-user config/data/cache locations |
| Tests | Rust tests, `proptest`, `criterion`, `cargo-fuzz` | Invariants, benchmarks, malformed-input coverage |
| CI/tooling | GitHub Actions + Rust `xtask` | Reusable release and library validation pipeline |

These are proposed dependencies, not an already compiled bill of materials. The core graphics, font, image, and native-toolkit choices have documented capabilities. [S24](../research/source-register.md#s24) [S25](../research/source-register.md#s25) [S26](../research/source-register.md#s26) [S27](../research/source-register.md#s27) [S23](../research/source-register.md#s23) [S52](../research/source-register.md#s52)

## Alternatives considered

**Tauri or Electron control UI:** workable for a control panel, but adds a web stack without solving native wallpaper surfaces or conversion. Not selected for this Rust-first product. A later remote-web UI is not part of the plan.

**Bevy or another general game engine:** useful for broad scene/game functionality, but larger than the initial cell-grid renderer needs. Reconsider only if 3D scene authoring becomes a dominant requirement; do not rebuild a general-purpose game engine accidentally.

**CPU-only drawing:** valuable for deterministic PNG/text export and a reduced static fallback, but not the main high-resolution animated path. A software adapter is not guaranteed on every machine, so provide an explicit CPU rasterizer for static exports rather than relying on a magical GPU fallback.

**Platform-native video decoders everywhere:** potentially efficient, but adds multiple decoding and color/timing paths before the product is stable. Start with a pinned FFmpeg adapter, then evaluate hardware-assisted/native decode on measured workloads.

## Native integration caveats

The Linux tray can require a particular backend and host support; it cannot be assumed to appear merely because the binary links. Keep Open Settings and Stop available through the application launcher and CLI. macOS native objects and menu integration must obey main-thread requirements. [S51](../research/source-register.md#s51) [S52](../research/source-register.md#s52)

The application may use small QML/GJS/native bridge components for desktop integration. 'Rust engine' does not require pretending that every desktop-shell plugin is naturally pure Rust.
