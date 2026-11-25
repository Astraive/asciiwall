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

## SCN-011 design proposal: finite mathematical fields

**Status: proposal for review; no formula below is implemented or registered by this design.** This decision compares candidate patterns against portable `waves` and `noise-field` entries and the proposed terrain-contour family. Terrain contour is documented as a proposal, not an implemented family. The new candidates below require a versioned extension and are not scene-version 1.0 capabilities.

### Existing-family comparison

| Request / visual | Existing representation | Decision |
| --- | --- | --- |
| Smooth periodic bands, traveling waves, or two-axis interference | `waves` provides bounded amplitude, frequency, speed, and color; two waves can be composed where existing graph arity permits. | Use existing `waves`; do not add a second sinusoid family. |
| Organic mottling / slowly changing field | `noise-field` provides bounded scale, speed, and color. | Use existing `noise-field`; its algorithm/version contract owns exact output. |
| Isolines over terrain-like or scalar content | Terrain contour is a documented proposed family; contour spacing over an existing field is a conversion/composition concept. | No new math family proposed; availability remains subject to that family's future implementation. |
| Concentric rings | Neither a single `waves` node nor noise expresses center-origin radial phase in its schema. | Candidate `radial-wave` below, as a versioned built-in. |
| Two-frequency interference | Composable from two `waves` nodes where registry and graph composition permit. | Reject as a separate family; existing built-ins suffice. |

### Candidate decision table (all values proposed)

Coordinates are converted to signed Q16.16 by round-to-nearest, ties-to-even; `center_x`/`center_y` and `phase_turns` convert from Q0.16 by dividing their unsigned integer value by 65536. `speed_q8_8` converts by dividing by 256; `gain_q8_8` likewise. Multiplication results return to Q16.16 with round-to-nearest, ties-to-even; addition/subtraction are checked. Output `v` is converted to signed Q1.15 with round-to-nearest, ties-to-even and clamp to `[-32768,32767]`. `t` is a fixed-step tick in `[0,65535]`; outside this domain reject before reservation. Phase uses checked signed 64-bit Q16.16 arithmetic followed by Euclidean fractional reduction to `[0,1)`, never wraparound. No wall clock, random generator, per-node seed, downloaded data, or expression evaluator is used. Unknown fields and non-finite values reject before allocation.

| Candidate and formula | Finite typed parameters (inclusive bounds; default) | Singular/domain outcome | Deterministic fixture (exact signed-Q1.15 expected values, rows top-to-bottom) | Maximum work |
| --- | --- | --- | --- | --- |
| `radial-wave`: let `d²=(x-cx)²+(y-cy)²`; `u=frac(cycles*d² + speed_q8_8*t/256 + phase_turns/65536)`; `v=1 - 4*min(u,1-u)`. This triangle-wave phase defines concentric rings without sqrt or trig. | `center_x`,`center_y`: `u16` Q0.16 `[0,65535]` (`32768`); `cycles`: `u8` `[1,32]` (`1`); `speed_q8_8`: `i16` `[-256,256]` (`0`); `phase_turns`: `u16` `[0,65535]` (`0`). | No division by radius or singular center; `frac` uses Euclidean modulo in `[0,1)`. `t` outside `[0,65535]` rejects; phase arithmetic uses checked 64-bit intermediates and Euclidean modular reduction, never wraparound. Overflow rejects at preflight. | 3×3; center `(32768,32768)`, cycles `1`, speed `0`, phase `0`, tick `0`. Expected `[[3640,18204,3640],[18204,32767,18204],[3640,18204,3640]]`. Repeat at identical inputs must be byte-identical. | Count each normalized coordinate conversion as 3 units (integer add, multiply, divide/round); dx/dy as 2; squares and sum as 3; cycles multiply as 1; phase/speed conversion and addition as 4; Euclidean frac as 2; min/triangle as 2; output conversion/clamp as 2. Total `≤25` work units/cell including fixed center/phase conversion allowance; one output field, no scratch. At most 40,000 cells per invocation (`≤1,000,000` work units), also bounded by implemented runtime's 65,536-cell ceiling. |
| `saddle-field`: `v=clamp(gain_q8_8/256 * 4(x-½)(y-½) + bias_q1_15/32768, -1, 1)`. | `gain_q8_8`: `u16` `[0,1024]` (`256`, i.e. gain 1); `bias_q1_15`: `i16` `[-32768,32767]` (`0`). | No singularity. Checked intermediate overflow rejects at preflight; clamp only the final value before Q1.15 output conversion. | 3×3; gain `256`, bias `0`, tick `0`. Expected `[[14564,0,-14564],[0,0,0],[-14564,0,14564]]`. Repeat at identical inputs must be byte-identical. | Count coordinates as 6 units; centered coordinates as 2; product and scale as 3; bias addition as 1; clamp and output conversion as 2. Total `≤14` work units/cell plus 2 fixed parameter-conversion units; one output field, no scratch. At most 65,536 cells per invocation (`≤917,506` work units including parameter conversion), also bounded by implemented runtime's 65,536-cell ceiling. |

Fixture fractions are exact before fixed-point conversion: radial corners `1/9`, edge middles `5/9`, center `1`; saddle corners have signed magnitude `4/9` and all other cells are zero. With specified Q16.16 conversion and operation rounding, radial corner Q16.16 phase is `14564`, edge phase is `7282`, and saddle corner magnitude is Q16.16 `29128`. Final Q1.15 values are radial `3640`/`18204`/`32767` and saddle `±14564`/`0`. These are analytically calculated expected values for proposed algorithms, not observed implementation outputs.

Work units are proposal accounting: each listed primitive operation costs one unit, and comparisons/clamps/rounding/modulo each cost one, except the explicitly grouped coordinate conversion counts as three. Counts deliberately include coordinate conversion, radial square distance, phase work, and final conversion; radial includes a conservative fixed center/phase conversion allowance, while saddle includes its two fixed parameter conversions. A future implementation must instrument/verify each operation category, reject dimensions whose checked product exceeds both the candidate work ceiling and the engine's actual 65,536-cell cap, and demonstrate no undercounting. These are proposed ceilings, not measured runtime data.

### Measurable implementation acceptance proposal

An implementation issue must require a versioned descriptor and closed typed parameter schema; exact same-version golden values above plus repeated-run equality; tick-domain, zero/maximum dimension, checked-overflow, and invalid-parameter rejection before reservation; measured per-category and total work at one-cell and maximum candidate grids without exceeding the stated ceilings; no per-frame allocation; and rejection of unsupported schema versions without changing the active safe scene. Exact same-version output is the quality target; any algorithm/output change requires a new algorithm version and reviewed golden update. No benchmark or implementation evidence is claimed here.

Reduced motion freezes the tick at the selected initial value and must not schedule continuous redraws for a static candidate. Export uses the explicit bounded start tick and fixed-step track and must match these same-version fixtures exactly. Neither formula consumes live signals, wall clock, optional host capabilities, or downloaded assets. Offline export is proposed only after conformance; it is not current engine support.

### Unresolved artistic choices and versioning

Reviewers must decide whether the triangle-wave radial profile is artistically preferable to sinusoidal rings, aspect-corrected distance, contrast mapping, saddle orientation, and UI exposure. No palette, glyph ramp, native tolerance, or benchmark hardware is chosen here.

Add approved generators only under a new schema version or explicitly negotiated feature extension defining node IDs, exact parameter types/defaults/bounds, serialization and algorithm versions, required-node behavior, deterministic fixtures, and checked work accounting. A v1 reader must reject unknown kinds and fields; unsupported required features fail closed. Conformance requires schema rejection fixtures, exact deterministic CPU goldens, max-dimension/work-bound cases, and reduced-motion/export tests. This proposal does not change engine capability.
