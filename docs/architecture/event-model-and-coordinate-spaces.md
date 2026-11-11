# Events and coordinate spaces

## Event envelope

Every event contains a source id, monotonic timestamp, sequence number, session generation, capability scope, and payload. Pointer motion is coalescible; button edges, topology changes, and explicit stop commands are not silently discarded. Bound queues and report overflow counters. The latest pointer state can replace many intermediate motion samples.

Use `pointer-move`, `pointer-button`, `pointer-enter`, `pointer-leave`, `window-snapshot`, `display-topology`, `power-policy`, `source-invalidated`, and `user-command` as separate event categories. Missing data is `None` with a reason, not coordinates `(0, 0)` or an empty window list that falsely means nothing covers the desktop.

## Coordinate domains

| Domain | Meaning |
| --- | --- |
| Native desktop coordinates | OS-defined global or logical positions; may not exist on Wayland |
| Output-local logical units | Coordinates relative to a monitor/output before physical scaling |
| Surface pixels | Actual render-target pixels after scale/rotation |
| Scene normalized coordinates | Usually x and y in [0, 1] inside the fitted content rectangle |
| Character coordinates | Column/row plus optional sub-cell offsets |

Conversions are named functions with an explicit topology generation. Support negative monitor origins, portrait rotation, nonuniform DPI, fractional scaling, letterboxing, cropping, and gaps between displays. Do not equate a raw mouse delta with a desktop cursor location. [S05](../research/source-register.md#s05) [S07](../research/source-register.md#s07) [S15](../research/source-register.md#s15)

For an unrotated content rectangle `(left, top, width, height)`, normalize an observed point using `u=(x-left)/width` and `v=(y-top)/height`. The validity region and fit transform determine whether the event reaches the scene. Do not clamp an out-of-bounds point into a permanent edge interaction unless the user explicitly selects edge attraction.

## Multi-display policies

Per-display mode routes events only to the intersected output. Spanned mode maps into a virtual scene space with physical gaps preserved or explicitly collapsed by a user setting. Duplicated mode can synchronize scene clocks while retaining independent local cursor coordinates. A monitor change invalidates all cached transforms atomically.

## Time and ordering

Input uses the receiving process's monotonic clock after a documented clock-domain mapping. Media timestamps are not input timestamps. On a large clock discontinuity or restart, generate a new epoch. Simulations consume events in order at fixed steps and interpolate rendering; they do not replay old clicks after a long pause.

For window movement, use last-good snapshots with a short freshness budget. If the data becomes stale, fade reactive forces out rather than leaving an invisible permanent wall.
