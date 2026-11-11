# Multi-monitor geometry, identity, and scaling

## Coordinate spaces

Maintain explicit transforms between OS-desktop coordinates, output-local logical coordinates, physical framebuffer pixels, scene-normalized coordinates, and character-cell coordinates. Every event states its space and display identity. Never pass an unlabelled `(x, y)` across an adapter boundary.

Support negative origins, mixed DPI, rotation, portrait monitors, mirrored outputs, and fractional scale. A cursor crossing displays must not jump due to double-applied scaling. Keep physical pixel rounding at presentation boundaries and use floating-point scene coordinates for interaction.

## Assignment modes

| Mode | Semantics |
|---|---|
| Independent | Different package/settings/timeline per output |
| Mirror | Same source/time; fit independently, not necessarily identical grids |
| Span | One virtual scene over a selected rectangle; account for gaps and negative origins |
| Synchronized | Separate scenes sharing a monotonic phase/clock |

Span across mixed refresh-rate/GPU outputs is best-effort, not perfectly synchronized scanout. Letterbox/crop rules must be shown in preview. Missing outputs do not stretch the remaining scene unless the user selected that policy.

## Identity and persistence

Use a local composite match of platform identifiers and remembered user labels. Hardware identifiers can be missing or duplicated. Never use an EDID serial as an exported portable assignment identity. Present a remapping screen when two candidates are ambiguous.

Disconnect parks the assignment, cancels its surface work, and releases resources after a short configured grace period. Reconnect revalidates scale, backend, and package availability. A dock appearing under a new connector name should not permanently discard the user's settings.

## Resource sharing

Share decoders and conversions only when source, timeline, settings, font metrics, and grid are compatible. Different resolutions may share decoded media but require different cell grids. Different GPUs can require separate devices/atlases and copies; sharing a Rust Arc does not share a native GPU allocation across devices.

One slow output must not block presentation everywhere. Use per-output pacing and bounded queues. Aggregate all outputs when enforcing memory, decoder, and export budgets.

## Wallpaper geometry versus application geometry

Window rectangles are clipped to each output before mapping into scene space. An overlapping application can affect multiple wallpapers. Fullscreen pause is per-output where confidence permits; global game mode is an explicit separate user policy.

Coverage uses the union of relevant rectangles, not the sum of their areas. Transparent windows, rounded corners, shadows, and unknown workspace membership make exact occlusion uncertain. Never advertise exact visibility when only rectangles are available.

## Test patterns

Use deterministic calibration scenes: numbered cell grid, corner markers, cursor crosshair, monitor-label overlay in diagnostics only, and a shared moving phase line. Test 100/125/150/200 percent scale, a monitor left of primary, portrait rotation, hotplug during Apply, laptop-lid closure, and mirrored outputs. These are test designs; no physical-monitor validation has been performed for this document set.
