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

These are first-party generator proposals, not third-party downloadable code. The bounded particle field and the incomplete SCN-002 matrix-rain generator are implemented in `ascii-scene`; the other families remain proposals. Matrix-rain does not yet have an executable portable registry, colored-frame/native consumer, or generic offline export. A community package must select a reviewed generator through a separately owned package contract and cannot provide executable code.

### SCN-002 matrix-rain generator (partial; issue remains blocked)

`ascii-scene::MatrixRainGenerator` now keeps one bounded stream record per grid column, derives a separate deterministic random stream for each column from the node stream, advances heads in the SCN-001 fixed-step runtime, and clips every trail cell to the grid before indexing the caller-owned frame. It borrows the existing immutable `ascii_convert::GlyphTable` and selects glyphs by its exposed table order; it does not copy or regenerate font records. Density, speed, trail length, and `#RRGGBBAA` configuration are validated before the runtime allocates its frame and stream buffers. A zero-speed generator freezes the simulation and does not refresh its frame.

This is generator-core behavior, not a complete portable node or application feature. SCN-001's export context is still specialized to `ParticleFieldGenerator`; the current schema is documentation rather than a shared executable registry, so missing/unknown-key and arity rejection have not been proven through that registry. The character-frame API carries no RGBA value, so the color is retained in configuration but is not rendered; consequently neither the safe-preset flash criterion nor native appearance is verified. No matrix-specific pointer-deflection or click-burst reaction operator is wired in. FNT-002/FNT-003 remain open, so the borrowed immutable glyph table is used without claiming an accepted font mapping. These limits, plus open SCN-001 integration (#124) and the separate AST-001/006/010/017 consumers, keep SCN-002 open and Blocked Yes.

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
