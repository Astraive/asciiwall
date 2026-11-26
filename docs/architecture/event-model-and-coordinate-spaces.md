# Events and coordinate spaces

## Event envelope

The `ascii-core::events::EventEnvelope` carries a source ID, receiving-process monotonic instant and epoch, per-source sequence, captured generation, capability scope, and typed payload. `EventQueue<N>` is fixed-capacity and allocation-free: it replaces only adjacent motion from the same source/generation/scope, can evict motion to admit a critical event, and returns a full/out-of-order critical event to the producer for FIFO retry. Saturating counters report coalesced/dropped/evicted motion, critical backpressure, and out-of-order arrivals; critical edges are never silently discarded.

Payload variants distinguish pointer motion, button press/release, enter, leave, window snapshot, topology replacement, power policy, source invalidation, and user Start/Pause/Resume/Stop commands. `Observation<T>` exposes `Option<T>` plus a required `MissingReason` whenever the value is absent; a missing position or window rectangle is never fabricated as `(0, 0)` or an empty geometry/list.

## Coordinate domains

| Domain | Meaning |
| --- | --- |
| Native desktop coordinates | OS-defined global or logical positions; may not exist on Wayland |
| Output-local logical units | Coordinates relative to a monitor/output before physical scaling |
| Surface pixels | Actual render-target pixels after scale/rotation |
| Scene normalized coordinates | Usually x and y in [0, 1] inside the fitted content rectangle |
| Character coordinates | Column/row plus optional sub-cell offsets |

`ascii-core::coordinates::OutputTransform` names each conversion between signed desktop logical, output-local logical, surface-pixel, normalized fitted-scene, and character-cell coordinates. Output scale has independent positive x/y factors, and rotation is explicit. `FittedContent` names `Letterbox`, `Crop`, or `Stretch`; crop carries its visible normalized scene rectangle. Scene points use `[0, 1)` and all content rectangles are half-open, so points outside fitted content return an error instead of sticking to an edge. Each transform checks the caller's current `Generation`.

The deterministic regression assertion for desktop-position round trips is an absolute tolerance of `1e-9` desktop-logical units; character-cell/sub-cell round trips use `1e-12` normalized-scene units. These are numeric contract tolerances, not claims about native cursor precision.

## Multi-display policies

`TopologySnapshot<N>` routes per-display positions only to an intersecting output; a span can preserve desktop gaps or explicitly collapse horizontal/vertical gaps for aligned rows/columns, while points in uncovered gaps remain unrouted. Mirror mode routes to every matching output under one caller-selected clock-group ID, but each output keeps its own transform and local cursor coordinates. A topology event is enveloped in its previous generation, then the freshness gate advances to the event's new generation; cached transforms reject that new generation rather than observing partial geometry. Monitor enumeration, topology matching, and native routing are deliberately outside Core.

## Time and ordering

`ClockDomainMapping` translates one source clock epoch to the receiver's monotonic epoch with checked rational scaling and origin adjustment. A source restart/discontinuity requires `ClockEpochTracker` to advance; old-epoch events reject. `EventQueue<N>` accepts nondecreasing mapped timestamps, and `FixedStepScheduler` assigns nondecreasing time buckets while preserving queue order within a step. `EventFreshnessGate` rejects stale generations and clicks older than the latest Resume boundary or the configured click-age budget.

`WindowForceFreshness` updates only from a known geometry observation, holds full force through its freshness budget, then linearly fades to zero over the configured fade duration. Missing or cross-epoch observations do not refresh the last-good timestamp.

## Implemented portable boundary and verification

Because the exact WPH-001 #370 base has no `ascii-runtime` crate (COR-005 #311 remains separate), the event and coordinate contracts live in the existing `ascii-core` crate: `crates/ascii-core/src/events.rs` and `crates/ascii-core/src/coordinates.rs`. No new runtime package or workspace member is introduced. Deterministic synthetic coverage is in `crates/ascii-core/tests/event_coordinates.rs`; run `cargo test -p ascii-core --test event_coordinates` on Windows x64. This test command is pending verification.

The implementation does not include Windows adapter integration, monitor discovery/topology matching, native event routing, OS clock sampling, or live-desktop evidence. Those native responsibilities remain unverified and belong to their owning platform/monitor issues.
