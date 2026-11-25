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

**Status: proposal for review; no formula below is implemented or registered by this design.** This decision uses the portable `waves` and `noise-field` schema entries as the baseline, and the existing terrain-contour family as composition context. It proposes two additional reviewed Rust generator families only as versioned extensions; these are not scene-version 1.0 capabilities.

### Existing-family comparison

| Request / visual | Existing representation | Decision |
| --- | --- | --- |
| Smooth periodic bands, traveling waves, or two-axis interference | `waves` provides bounded amplitude, frequency, speed, and color; a graph can combine existing built-ins where registry arity permits. | Use `waves` for v1. Do not add another sinusoid family merely to rename it. A second wave with a fixed phase offset is a graph composition, not a new formula kind. |
| Organic mottling / slowly changing field | `noise-field` provides bounded scale, speed, and color. | Use `noise-field` for v1; exact noise algorithm and cross-version output are governed by that node's own contract. |
| Isolines over terrain-like or scalar content | Terrain contour is a proposed family; contour spacing over an existing field is a conversion/contour composition, not a new math language. | Reuse existing contour capability if available in a future owning implementation; no added contour generator in this proposal. |
| Concentric expanding rings | Neither a single `waves` node nor noise expresses a center-origin radial phase in the existing parameter schema. | Candidate `radial-wave` below; requires a new versioned built-in. |
| Two-frequency interference | Can be composed from two `waves` nodes where the graph's fixed registry and composition permit it. | Reject as a separate family; no new parameters or algorithm are justified. |

### Candidate decision table (all values proposed)

The formulas are evaluated in normalized cell-center coordinates `x=(column+0.5)/width`, `y=(row+0.5)/height`. `t` is a nonnegative fixed-step tick count, not wall-clock time. Proposed arithmetic is finite fixed-point (Q16.16 inputs/intermediates with checked operations); trig is a reviewed lookup/table implementation pinned by algorithm version, never a parsed expression. Color mapping remains the existing downstream conversion. All parameter objects reject unknown keys and non-finite/out-of-range values before allocation.

| Candidate and formula | Finite typed parameters (inclusive bounds; default) | Singular/domain outcome | Determinism / fixture | Maximum work |
| --- | --- | --- | --- | --- |
| `radial-wave`: `v = sin(2π f (r - s t) + φ)`, `r = sqrt((x-cx)²+(y-cy)²)` | `center_x`, `center_y`: `u16` Q0.16 in `[0,65535]` (`32768`); `cycles`: `u8` `[1,32]` (`6`); `speed_q8_8`: `i16` `[-256,256]` (`0`); `phase_turns`: `u16` `[0,65535]` (`0`); `seed`: `u32` `[0,4294967295]` (`1`, reserved for future deterministic jitter; output must not depend on it in v1 proposal). | At `r=0`, define `v=sin(φ)`; no division by radius. Clamp `r` to the fixed-point representable normalized diagonal; checked overflow rejects the scene at preflight, never wraps. | Fixture: 4×4, center `(32768,32768)`, cycles `4`, speed `0`, phase `0`, seed `1`, tick `0`; require byte-identical cells across repeated runs and golden values for each cell after formula/table version is pinned. Tick fixtures `0` and `60` with nonzero speed must repeat exactly. | One fixed-point radius and table lookup per cell; proposed `≤ 12` work units/cell, no scratch beyond output field; dimensions constrained by existing `1,048,576`-cell ceiling. |
| `saddle-field`: `v = clamp(4(x-½)(y-½), -1, 1)` | `gain_q8_8`: `u16` `[0,1024]` (`256`, interpreted as `[0,4]`); `bias_q1_15`: `i16` `[-32768,32767]` (`0`); `seed`: `u32` `[0,4294967295]` (`1`, reserved, no v1 effect). | No singularity. Coordinates and product are fixed-point; checked arithmetic failure is a preflight error. Clamp final value to `[-1,1]` before downstream mapping. | Fixture: 3×3, gain `256`, bias `0`, seed `1`; center exactly zero, opposite corners positive, off-diagonal sign symmetry and identical output across runs. | At most 4 multiply/add/clamp units per cell; proposed `≤ 8` work units/cell, no scratch beyond output field; same existing cell ceiling. |

The seed fields above are included only to keep a uniform serialized-generator convention; because neither proposed formula uses randomness, changing seed MUST leave these candidate fields unchanged. A later design should omit seed if this uniformity does not justify extra schema surface. These are proposed cost envelopes, not measurements. Work units need an implementation-owned definition and checked aggregate budgeting before this ceiling can be accepted.

### Measurable implementation acceptance proposal

For each approved candidate, a later implementation issue must require: (1) a versioned descriptor and exact finite parameter schema with unknown-key rejection; (2) pinned fixed-point/table algorithm and hand-reviewed golden fixture values, exact same-version output for repeated identical seed/params/dimensions/tick; (3) width/height/cell-boundary tests including 0, maximum, checked arithmetic and invalid-value rejection before reservation; (4) measured maximum work count at one-cell and maximum supported grids, with observed work never above the table bound and no per-frame allocation; (5) unsupported scene/schema versions rejected without changing the active safe scene. Proposed quality target: exact cells for the same version and fixture, with no visual drift accepted unless a new algorithm version and golden update are reviewed. These targets are not run or evidenced here.

Reduced motion freezes `t` at the selected initial tick and must not schedule continuous redraws for a static candidate; export uses explicit start tick and fixed-step timeline, and must match the same-version fixture exactly. Neither formula consumes live signals, wall clock, optional host capabilities, or downloaded assets. Proposed offline export support is yes after deterministic conformance; this is not current engine support.

### Unresolved artistic choices and versioning

Reviewers still need to choose radial phase convention (turns versus radians), normalized versus aspect-corrected distance, ring contrast/threshold mapping, saddle sign/orientation, and whether static variants deserve UI exposure. This proposal deliberately does not pick visual palettes, glyph ramps, native tolerances, or benchmark hardware. A review must decide these before implementation.

Add approved generators only under a new scene schema version or explicitly negotiated feature extension that defines node IDs, exact parameter types/defaults/bounds, serialization and algorithm versions, required-node behavior, deterministic fixtures, and work accounting. A v1 reader MUST reject unknown generator kinds and unknown node fields rather than ignore them; extension negotiation must fail closed for unsupported features. Conformance requires schema rejection fixtures, deterministic CPU goldens per algorithm version, max-dimension and work-bound fixtures, and reduced-motion/export tests. No evidence here changes engine capabilities.
