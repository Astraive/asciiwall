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

These are first-party generator proposals, not third-party downloadable code. Only the bounded particle field below is implemented in `ascii-scene`; the other families remain proposals. A community package must select a reviewed generator through a separately owned package contract and cannot provide executable code.

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
