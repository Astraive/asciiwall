# Procedural scene system

## Scene contract

`SceneGenerator` is a sealed first-party execution contract driven by generic `SceneRuntime<G>`. It exposes the versioned descriptor and node ID, checked scratch sizing, reservation-permitted preparation, bounded fixed-step updates, and rendering into caller-owned character storage. Each update receives validated parameters, fixed-step context, dimensions, and capability-filtered approved snapshots. The crate must not open files, query native/OS state, access the network, or evaluate package code.

Generator descriptors declare a local parameter schema/version, limits, supported signals, reduced-motion policy, export support, presets, and a logical resource estimate. Runtime construction reserves checked resources before calling generator preparation; updates and rendering reuse owned buffers without per-step allocation. The crate exposes the runtime contract, not a consumer: this repository has no wallpaper, thumbnail, application-preview, or native-surface consumer, so those integrations are not claimed.

## Initial generator families

| Family | Output | Core controls | Interaction hook |
| --- | --- | --- | --- |
| Digital rain | character streams | speed, density, trail, glyph set | pointer deflection, click burst |
| Starfield | particles/cells | depth, velocity, brightness | attraction, warp impulse |
| Wave field | scalar field | wavelength, phase, amplitude | ripple impulses |
| Fire | scalar/color field | cooling, turbulence, fuel | optional gust signal |
| Terrain contour | field/edges | scale, seed, contour spacing | parallax |
| Wireframe solids | projected edges | object, angular speed, camera | controlled orbit |
| Particle garden | particles/cells | emission, lifetime, drag | pointer attraction/repulsion |
| Clock typography | character layout | format, alignment, locale | none required |

These are first-party generator proposals, not third-party downloadable code. The bounded particle field, SCN-002 matrix-rain generator, and SCN-003 depth-projected starfield generator are implemented in `ascii-scene`; the other families remain proposals. Matrix-rain supports bounded deterministic offline character-frame export through `ExportContext::execute_frame_with` when the caller supplies the generator, validated scene parameters, and immutable glyph table, but there is no generic offline export, executable portable registry, or colored-frame/native consumer. A community package must select a reviewed generator through a separately owned package contract and cannot provide executable code.

### SCN-002 matrix-rain generator (partial; issue remains blocked)

`ascii-scene::MatrixRainGenerator` now keeps one bounded stream record per grid column, derives a separate deterministic random stream for each column from the node stream, advances heads in the SCN-001 fixed-step runtime, and clips every trail cell to the grid before indexing the caller-owned frame. It borrows the existing immutable `ascii_convert::GlyphTable` and selects glyphs by its exposed table order; it does not copy or regenerate font records. Density, speed, trail length, and `#RRGGBBAA` configuration are validated before the runtime allocates its frame and stream buffers. A zero-speed generator freezes the simulation and does not refresh its frame.

This is generator-core behavior, not a complete portable node or application feature. SCN-001's generic `SceneRuntime` powers the offline export path, and its caller-selected `ExportContext` identity, seed/settings fingerprints, event track, and exact step index are retained for MatrixRain frames. MatrixRain declares no supported signals, so nonempty recorded interaction tracks are rejected rather than synthesized. The current schema is documentation rather than a shared executable registry, so missing/unknown-key and arity rejection have not been proven through that registry. The character-frame API carries no RGBA value, so the color is retained in configuration but is not rendered; consequently neither the safe-preset flash criterion nor native appearance is verified. No matrix-specific pointer-deflection or click-burst reaction operator is wired in. FNT-002/FNT-003 remain open, so the borrowed immutable glyph table is used without claiming an accepted font mapping. These limits, plus open SCN-001 integration (#124) and the separate AST-001/006/010/017 consumers, keep SCN-002 open and Blocked Yes.

### SCN-003 depth-projected starfield generator (partial; issue remains blocked)

`ascii_scene::StarfieldGenerator` implements deterministic Q16 depth projection through the SCN-001 `SceneRuntime`, borrowing the existing immutable `ascii_convert::GlyphTable`. It keeps one independent SplitMix64 stream per effective particle, recycles particles at the near plane or when their projection leaves the grid, and maps depth brightness to the measured table's immutable coverage order. Configuration validates requested count (1–20,000), speed (0–10), depth (0.1–100), and exact `#RRGGBBAA` spelling before runtime activation. The requested count remains available while the allocated/effective count is capped by SCN-001 `scene.max-particles`; checked scratch sizing and SCN-001 resource reservation happen before the particle vector is reserved. Pointer attraction and button-0 warp impulse consume only host-filtered `ApprovedSnapshots`. Zero speed and reduced-motion/time-scale gates do not evolve the scene.

Approved pointer-button tracks are admitted only up to `MAX_APPROVED_BUTTON_EVENTS` (256). The host filter rejects an oversized track in full (`pointer_buttons() == None`) before the approved snapshot retains its slice; it does not silently keep an arbitrary-length input or scan a prefix. Starfield's checked per-advance work estimate charges the full 256-event maximum times the configured catch-up step count (1,024 units at the default four-step limit), covering the worst-case button scan on every fixed step.

The local descriptor is generator introspection only, not the draft-v1 executable package registry: missing/unknown parameter rejection and node arity have not been validated through a shared registry. Color is retained as portable metadata but the character-cell output cannot render RGBA. The descriptor deliberately does not claim deterministic export because SCN-001 export remains specialized to `ParticleFieldGenerator`; no native surface, app behavior, schema, CFG persistence, or accepted FNT mapping is claimed. These external capabilities remain blockers to acceptance.

The dedicated integration test exercises a fixed 600-step deterministic timeline, near-plane recycling and a 27-case, 60-second simulation benchmark (ambient/warp/still × 160/240/320 columns × 20/30/60 FPS). Results below were measured on Windows 11 x64 build 26300 (`x86_64-pc-windows-msvc`), Rust `1.100.0` (`0ef1bc386`, LLVM `23.1.5`), debug test profile. Each case advanced exactly 60 seconds of elapsed simulation. p50/p95/p99 are `Instant` durations around `SceneRuntime::advance`, in nanoseconds. Peak state bytes are SCN-001's checked logical frame-plus-particle estimate, excluding allocator overhead and RSS.

| Preset | Columns | FPS | p50 ns | p95 ns | p99 ns | Peak state bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ambient | 160 | 20 | 96,000 | 99,700 | 123,800 | 88,808 |
| ambient | 160 | 30 | 81,300 | 83,400 | 84,900 | 88,808 |
| ambient | 160 | 60 | 66,600 | 67,000 | 70,300 | 88,808 |
| ambient | 240 | 20 | 111,300 | 114,500 | 172,500 | 127,208 |
| ambient | 240 | 30 | 96,700 | 99,500 | 110,800 | 127,208 |
| ambient | 240 | 60 | 82,000 | 84,100 | 86,900 | 127,208 |
| ambient | 320 | 20 | 126,500 | 130,200 | 160,400 | 165,608 |
| ambient | 320 | 30 | 111,800 | 115,000 | 150,500 | 165,608 |
| ambient | 320 | 60 | 97,500 | 100,300 | 167,100 | 165,608 |
| warp | 160 | 20 | 294,500 | 300,200 | 393,900 | 124,808 |
| warp | 160 | 30 | 236,000 | 242,900 | 248,700 | 124,808 |
| warp | 160 | 60 | 183,500 | 481,100 | 944,800 | 124,808 |
| warp | 240 | 20 | 705,800 | 984,300 | 1,313,300 | 163,208 |
| warp | 240 | 30 | 567,500 | 778,400 | 1,224,800 | 163,208 |
| warp | 240 | 60 | 442,900 | 742,700 | 2,174,100 | 163,208 |
| warp | 320 | 20 | 739,600 | 1,261,700 | 2,756,400 | 201,608 |
| warp | 320 | 30 | 606,800 | 1,118,200 | 2,616,700 | 201,608 |
| warp | 320 | 60 | 466,900 | 1,217,600 | 2,421,600 | 201,608 |
| still | 160 | 20 | 100 | 100 | 100 | 88,808 |
| still | 160 | 30 | 100 | 100 | 100 | 88,808 |
| still | 160 | 60 | 100 | 200 | 200 | 88,808 |
| still | 240 | 20 | 100 | 200 | 200 | 127,208 |
| still | 240 | 30 | 100 | 200 | 200 | 127,208 |
| still | 240 | 60 | 100 | 200 | 200 | 127,208 |
| still | 320 | 20 | 100 | 200 | 200 | 165,608 |
| still | 320 | 30 | 100 | 200 | 200 | 165,608 |
| still | 320 | 60 | 100 | 200 | 200 | 165,608 |

The SCN-003 synthetic fixture uses `synthetic_font(false)`, repertoire `BA `, `GlyphConfig(8.0, 8, 12, 9)`, node `starfield`, seed `0x0154`, ambient config, an `80 × 40` grid, empty approved snapshots, and 600 fixed steps at 60 Hz. Its immutable glyph-table fingerprint is `4ddc3a1585b233278e7050a05eb95b777916375d3e619efb8c76cbd85db648e0`; the row-major scalar-wise FNV-1a-64 cell hash is `0f7090f8c4d34773`. Both are pinned in `crates/ascii-scene/tests/starfield.rs`. The hash pins this synthetic-table implementation output, not an accepted FNT portable mapping.

### SCN-004 deterministic wave field (partial; issue remains blocked)

`ascii_scene::WaveFieldGenerator` samples a two-dimensional periodic scalar field with integer fixed-point phase and maps each value through the borrowed immutable glyph table's measured coverage order. It declares amplitude `0..10`, frequency `0..20`, speed `0..10`, and portable `#RRGGBBAA` metadata; the color remains metadata because this runtime outputs only characters. The calm preset is `0.5 / 2 / 0.5 / #66ccffff`, contour is `2 / 5 / 1 / #ffffffff`, and still reuses calm values with speed zero. Zero frequency is spatially uniform (temporal phase may still change it); zero amplitude selects the center glyph without division or nonfinite arithmetic. A zero-speed or reduced-motion runtime does not evolve.

The generator stores only a bounded phase word in addition to the SCN-001 frame; checked grid/resource limits and the shared runtime reservation govern construction. The configuration and generator descriptor are local Rust introspection only, not draft-v1 schema validation. Color rendering, generic deterministic export, native/application integration, and accepted FNT-002/FNT-003 glyph mapping are not supported. Ripple impulses are intentionally not synthesized from pointer/button observations: the current reviewed wave interface has no approved ripple-effect snapshot contract, so the generator advertises no supported signals and consumes no snapshots. SCN-001 acceptance remains blocked; this implementation is not an acceptance claim for its downstream consumers.

The focused fixture uses the source-generated `synthetic_font(false)`, repertoire `BA `, `GlyphConfig(8.0, 8, 12, 9)`, default wave node ID, seed `0x0155`, calm config, an `80 × 40` grid, 60-Hz simulation, empty snapshots, and 3,600 steps. Its immutable glyph-table fingerprint is `4ddc3a1585b233278e7050a05eb95b777916375d3e619efb8c76cbd85db648e0`. The config FNV-1a-64 hashes little-endian `u32` values `[amplitude_q16, frequency_q16, speed_q16, color_rgba]` to `4edd48d9b0f96007`; the scalar-wise row-major cell FNV-1a-64 is `16804a1f09ca39a5`. All three fixture values are pinned in `crates/ascii-scene/tests/waves.rs`; they identify this synthetic fixture, not an accepted portable font mapping.

### SCN-005 seeded noise field (portable generator-core slice; issue remains open)

`ascii_scene::NoiseFieldGenerator` samples a bounded per-cell scalar field through `SceneRuntime`, maps values through the borrowed immutable glyph table, and uses versioned `value-noise-q16-v1` (SplitMix64 lattice hashing plus fixed-point smoothstep interpolation). Scene/node seeds derive from SCN-001's independent streams; terrain uses an additional effect-derived stream mixed with its internal seed. Portable configuration is exactly scale `0.01..=100`, speed `0..=10`, and RGBA metadata. The built-in presets are cloud (`2 / 0.2 / #ccccffff`), fine-noise (`20 / 0.5 / #88ffccff`), and static terrain (scale `4`, internal spacing `0.1`, seed `1`). Internal terrain controls are typed Rust-only values, not portable-v1 properties. Optional terrain parallax consumes only the already capability-filtered normalized pointer snapshot and applies a bounded one-sample offset; it never queries or synthesizes input.

Each render samples exactly one scalar per bounded output cell; scratch is fixed-size, output uses the shared preallocated frame, and runtime dimensions/resource estimates are checked before preparation. Reduced motion and zero speed avoid phase evolution; exports use SCN-001's deterministic generator path. This does not implement or claim shared registry missing/unknown-key or arity validation, rendered RGBA, native surfaces, or an application exporter integration.

Portable regressions cover all presets, seed repeatability, config/internal-control bounds, zero-speed and scale boundaries, and resized grids. The deterministic fixture uses a source-generated synthetic font, repertoire `BA `, `GlyphConfig(8.0, 8, 12, 9)`, scene seed `1`, empty approved snapshots, a `32 × 16` grid, and 600 steps at 60 Hz. Config fingerprints (FNV-1a-64 over little-endian Q16 scale, Q16 speed, and RGBA) and row-major scalar-wise cell FNV-1a-64 hashes are cloud `9589057432bb37a3 / 6d180171bde34b25`, fine-noise `aa6994c4fd36aeed / 6d180171bde34b25`, terrain `cc60df6db371e31f / 3beb2df904f68d25`. Focused tests passed 3/3 on Windows x64 with Rust 1.100.0 (`x86_64-pc-windows-msvc`) in debug test profile. These are synthetic-font portable evidence, not accepted font mapping. No 160/240/320 × 20/30/60 FPS timing distributions or measured 60-second peak-memory results are claimed here; no native renderer is present in this repository slice.

The benchmark ran in the focused suite with `CARGO_HOME=C:/Users/gskne/.cargo/asciiwall-img001 CARGO_TARGET_DIR=D:/target cargo test -p ascii-scene --lib --test contracts --test matrix_rain --test starfield --test waves -- --nocapture`. Environment: Windows 11 Home Single Language build 26300 on an Intel Core i7-13650HX (20 logical processors, 16,873,545,728 bytes RAM), `x86_64-pc-windows-msvc`, Rust/cargo `1.100.0` (`rustc 0ef1bc386`, LLVM `23.1.5`), debug test profile. `Cargo.lock` SHA-256 is `9a6e77e2100867550053f0e79ae8b6ead95e2ea6b67edfa6058fe164be210ae8`; locked non-workspace dependencies include `fontdue 0.9.3`, `hashbrown 0.15.5`, `ttf-parser 0.21.1`, `sha2 0.10.9`, `allocator-api2 0.2.21`, `equivalent 1.0.3`, `foldhash 0.1.5`, `cfg-if 1.0.5`, `cpufeatures 0.2.17`, `digest 0.10.7`, `block-buffer 0.10.4`, `generic-array 0.14.7`, `typenum 1.20.1`, `crypto-common 0.1.7`, and `version_check 0.9.5`. Each case advances exactly 60 seconds at its requested presentation rate. p50/p95/p99 are `Instant` duration per `SceneRuntime::advance` in nanoseconds; logical state is the checked frame storage plus the generator's two-byte phase word, excluding allocator overhead and RSS.

Focused validation passed 41 tests across the five selected scene suites; the config/glyph fingerprint test passed in its targeted run. `cargo fmt --all` completed and `cargo clippy -p ascii-scene --all-targets -- -D warnings` passed.

| Preset | Columns | FPS | p50 ns | p95 ns | p99 ns | Peak logical state bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| calm | 160 | 20 | 268,700 | 284,700 | 463,600 | 57,602 |
| calm | 160 | 30 | 270,000 | 286,900 | 417,300 | 57,602 |
| calm | 160 | 60 | 269,200 | 283,000 | 366,700 | 57,602 |
| calm | 240 | 20 | 597,600 | 622,300 | 1,095,300 | 129,602 |
| calm | 240 | 30 | 1,571,400 | 3,078,300 | 4,679,800 | 129,602 |
| calm | 240 | 60 | 2,102,000 | 3,495,900 | 4,903,300 | 129,602 |
| calm | 320 | 20 | 3,733,900 | 5,856,700 | 8,240,800 | 230,402 |
| calm | 320 | 30 | 3,671,800 | 5,990,800 | 10,318,400 | 230,402 |
| calm | 320 | 60 | 3,694,800 | 5,895,600 | 9,446,800 | 230,402 |
| contour | 160 | 20 | 943,200 | 1,695,400 | 3,512,000 | 57,602 |
| contour | 160 | 30 | 941,500 | 1,422,200 | 2,701,200 | 57,602 |
| contour | 160 | 60 | 922,800 | 1,482,700 | 3,241,000 | 57,602 |
| contour | 240 | 20 | 2,101,400 | 3,474,300 | 6,021,400 | 129,602 |
| contour | 240 | 30 | 2,227,300 | 4,338,300 | 5,911,400 | 129,602 |
| contour | 240 | 60 | 2,127,100 | 3,758,100 | 5,703,900 | 129,602 |
| contour | 320 | 20 | 3,833,300 | 6,260,800 | 8,566,400 | 230,402 |
| contour | 320 | 30 | 3,761,600 | 5,746,800 | 8,127,000 | 230,402 |
| contour | 320 | 60 | 3,784,000 | 6,037,100 | 10,662,900 | 230,402 |
| still | 160 | 20 | 100 | 200 | 200 | 57,602 |
| still | 160 | 30 | 100 | 200 | 200 | 57,602 |
| still | 160 | 60 | 100 | 200 | 200 | 57,602 |
| still | 240 | 20 | 100 | 200 | 200 | 129,602 |
| still | 240 | 30 | 100 | 100 | 100 | 129,602 |
| still | 240 | 60 | 100 | 200 | 200 | 129,602 |
| still | 320 | 20 | 100 | 100 | 100 | 230,402 |
| still | 320 | 30 | 100 | 200 | 200 | 230,402 |
| still | 320 | 60 | 100 | 100 | 100 | 230,402 |

#### Deterministic fixture

The focused fixture is generated from `crates/ascii-convert/tests/support/synthetic_font.rs::synthetic_font(false)` (rights-cleared, source-generated SFNT), repertoire `BA `, and `GlyphConfig(8.0, 8, 12, 9)`. With node `matrix`, seed `0x0153`, ambient configuration, a `32 × 16` grid, empty approved snapshots, and 600 steps at 60 Hz, the immutable glyph-table fingerprint is `4ddc3a1585b233278e7050a05eb95b777916375d3e619efb8c76cbd85db648e0`; row-major scalar-wise FNV-1a-64 cell hash is `5e536b47a9c9df32`. Both are pinned in `crates/ascii-scene/tests/matrix_rain.rs`. This pins the SCN-002 generator output against this synthetic table; it is not a claim that FNT-002's portable font mapping is accepted.

#### Host-side simulation benchmark

Command: `CARGO_HOME=C:/Users/gskne/.cargo/asciiwall-img001 CARGO_TARGET_DIR=D:/target cargo test -p ascii-scene --test contracts --test matrix_rain -- --nocapture`. Verification host: Windows x64 (`x86_64-pc-windows-msvc`), Rust `1.100.0` (`0ef1bc386`, LLVM `23.1.5`), debug test profile. Each row supplied exactly 60 seconds of elapsed time over its requested frame count; dynamic presets reached 3,600 fixed 60-Hz steps, while static correctly advanced zero steps. p50/p95/p99 are `Instant` durations around each `SceneRuntime::advance`, in nanoseconds. Peak bytes are the checked logical requested frame-plus-column-buffer estimate, excluding allocator overhead/RSS. Work units are the checked per-advance upper bound. These are local debug-profile simulation measurements, not native renderer performance or cross-machine limits.

| Preset | Columns | FPS | p50 ns | p95 ns | p99 ns | Peak bytes | Work units | Steps |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ambient | 160 | 20 | 20,100 | 21,900 | 31,800 | 34,560 | 11,520 | 3,600 |
| dense | 160 | 20 | 102,000 | 121,500 | 122,700 | 34,560 | 17,920 | 3,600 |
| static | 160 | 20 | 0 | 100 | 100 | 34,560 | 7,680 | 0 |
| ambient | 160 | 30 | 19,400 | 21,400 | 24,000 | 34,560 | 11,520 | 3,600 |
| dense | 160 | 30 | 101,500 | 120,400 | 124,500 | 34,560 | 17,920 | 3,600 |
| static | 160 | 30 | 0 | 100 | 100 | 34,560 | 7,680 | 0 |
| ambient | 160 | 60 | 18,800 | 20,900 | 21,500 | 34,560 | 11,520 | 3,600 |
| dense | 160 | 60 | 101,300 | 120,600 | 157,700 | 34,560 | 17,920 | 3,600 |
| static | 160 | 60 | 0 | 100 | 100 | 34,560 | 7,680 | 0 |
| ambient | 240 | 20 | 31,400 | 34,200 | 37,900 | 51,840 | 17,280 | 3,600 |
| dense | 240 | 20 | 153,800 | 182,800 | 185,200 | 51,840 | 26,880 | 3,600 |
| static | 240 | 20 | 0 | 100 | 100 | 51,840 | 11,520 | 0 |
| ambient | 240 | 30 | 30,700 | 33,400 | 35,200 | 51,840 | 17,280 | 3,600 |
| dense | 240 | 30 | 153,200 | 181,500 | 185,200 | 51,840 | 26,880 | 3,600 |
| static | 240 | 30 | 0 | 100 | 100 | 51,840 | 11,520 | 0 |
| ambient | 240 | 60 | 30,000 | 32,700 | 33,900 | 51,840 | 17,280 | 3,600 |
| dense | 240 | 60 | 152,500 | 181,000 | 184,300 | 51,840 | 26,880 | 3,600 |
| static | 240 | 60 | 0 | 100 | 100 | 51,840 | 11,520 | 0 |
| ambient | 320 | 20 | 43,400 | 47,500 | 48,600 | 69,120 | 23,040 | 3,600 |
| dense | 320 | 20 | 211,100 | 251,300 | 261,300 | 69,120 | 35,840 | 3,600 |
| static | 320 | 20 | 0 | 100 | 100 | 69,120 | 15,360 | 0 |
| ambient | 320 | 30 | 42,500 | 46,600 | 47,500 | 69,120 | 23,040 | 3,600 |
| dense | 320 | 30 | 210,400 | 248,900 | 252,100 | 69,120 | 35,840 | 3,600 |
| static | 320 | 30 | 0 | 100 | 100 | 69,120 | 15,360 | 0 |
| ambient | 320 | 60 | 41,600 | 45,800 | 47,800 | 69,120 | 23,040 | 3,600 |
| dense | 320 | 60 | 473,100 | 928,900 | 2,526,400 | 69,120 | 35,840 | 3,600 |
| static | 320 | 60 | 100 | 100 | 100 | 69,120 | 15,360 | 0 |

## Simulation

`ascii-scene` uses an integer Q16.16 fixed-step clock and a default catch-up maximum of four steps (configurable from one through eight). The finite time-scale parameter is rounded to Q16.16 during validation. Each advance clamps elapsed simulation time to that step budget; a 60-second resume gap therefore cannot cause more than the configured number of updates. A zero time scale or disabled background motion skips simulation and frame regeneration, with zero estimated per-advance work units.

The particle field caps the grid at 1,024 columns, 1,024 rows, and 65,536 cells; particles at 20,000; and per-runtime logical storage at 1 MiB. A runtime advance is bounded to 1,000,000 estimated work units, counting grid writes and bounded particle passes; export preflight also charges its recorded-event scan against that cap. Velocity is clamped to 64 cells/second; initial acceleration is bounded to ±1 cell/second² and pointer attraction is combined under the 16 cells/second² descriptor cap; lifetime is at most 7,200 simulation ticks. A single bounded pass updates each active particle per fixed step; collision solving is not implemented. Character-grid output is discrete, so no render interpolation is exposed. Character and particle buffers are allocated after checked preflight and reused.

The default shared aggregate resource budget is 4 MiB. The maximum current logical estimate is 902,144 requested bytes per runtime (65,536 `char` cells plus 20,000 32-byte particle records), or 3,608,576 requested bytes for four such previews. These requested-storage estimates exclude allocator over-allocation, allocator metadata, and process RSS. Node identifiers are limited to 128 UTF-8 bytes and recorded export tracks to 65,536 events.

## Implemented SCN-001 slice

The version-1 `builtin.particle-field` descriptor reports five typed, bounded parameters: seed (`u32`, default 1), time scale (finite 0..4, default 1), particle cap (0..20,000, default 2,000), simulation frequency (15..120 Hz, default 60), and background motion (default enabled). The generator-local parameter metadata is not a settings registry, persistent CFG contract, or package schema. Settings persistence and graph semantics remain owned by CFG and PKG.

The built-in generator emits direct character cells and supports only normalized pointer-position signals. `ApprovedSnapshots` can retain normalized pointer, button, window-rectangle, or battery values only when the host explicitly approves the matching capability; pointer attraction consumes only the approved pointer position and never observes the OS or global state itself. Q0.16 coordinates map linearly to grid Q16.16; each fixed step adds one-eighth of the shortest wrapped pointer delta to base acceleration, clamped to the 16 cells/second² descriptor cap. Empty snapshots and explicit idle export remain valid no-input paths. No application UI or native input adapter is claimed.

Unit and integration regressions in `crates/ascii-scene/src/generator.rs` and `crates/ascii-scene/tests/contracts.rs` check same-seed exact cells across 600 fixed steps, trait-driven runtime dispatch and preparation-failure reservation cleanup, exact exports for seeds 0 and `u32::MAX`, deterministic synthetic pointer interaction and recorded-track replay, last-event-wins duplicate ordering, future-event exclusion, the maximum-event/two-scan work boundary before reservation, independent node and effect streams, default/configured catch-up boundaries, static behavior, buffer reuse, aggregate resource budgets, capability filtering, unsupported-event rejection, and invalid input/resource failures. The suite is contract evidence, not a performance benchmark. No native visual, cross-device, or wall-clock performance measurement is claimed.

## Determinism

Version 1 pins SplitMix64 for random streams. Node seeds use FNV-1a-64 over the versioned node domain, little-endian `u32` scene seed, and length-prefixed UTF-8 node ID; effect seeds use a separate versioned domain and include both node and effect IDs. IDs are bounded to 128 bytes; adding a node or effect does not consume an existing stream. Particle state and fixed-step accumulation use integer Q16.16 arithmetic. Under identical parameters, dimensions, and step track, the implementation produces exact same-version character cells; no cross-version serialized-state guarantee is implied.

`ExportContext` requires a selected start timestamp, validated scene parameters/seed, settings fingerprint, font identity, and a recorded normalized event track or explicit idle fallback. Recorded tracks are nondecreasing by step and capped at 65,536 events. Only the descriptor-supported pointer-position event is replayed; unsupported kinds fail before runtime reservation. Events at a step are processed in input order, so the last pointer position at duplicate steps wins. Step-zero input applies before the first update; a future event beyond the requested output step is retained in the returned track but does not affect that frame. A single forward cursor makes replay O(events + steps); checked preflight charges both full-track scan bounds (supported-signal validation and replay) against the work-unit cap before runtime reservation, with no per-step allocation. Empty recorded tracks and explicit idle fallback perform no synthetic interaction.

## Creator affordances

Descriptor metadata groups the five bounded parameters and supplies descriptions suitable for control labels/tooltips, along with baseline and static preset values. A local runtime can use empty snapshots or host-approved normalized pointer snapshots without global observation permission. The built-in field supports deterministic pointer attraction only; no application controls, preview UI, live input adapter, or native consumer are implemented here. Preset cost is explicitly an estimate; no performance benchmark or native visual measurement is claimed.
