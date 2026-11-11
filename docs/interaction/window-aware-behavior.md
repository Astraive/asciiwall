# Reactions to windows above the wallpaper

## What the feature means

Applications remain above the wallpaper. The engine can make particles avoid their estimated rectangles, produce a ripple when a window moves, dim exposed areas near a window, or pause while the desktop is covered. It does not draw over applications, read their content, or modify their windows.

Window geometry is platform dependent. Wayland toplevel lists do not universally provide global rectangles; compositor bridges require separate support. [S15](../research/source-register.md#s15) [S17](../research/source-register.md#s17) [S18](../research/source-register.md#s18) Windows bounds also require DPI/border normalization. [S05](../research/source-register.md#s05) [S06](../research/source-register.md#s06)

## Metadata model

Maintain an ephemeral handle, output intersections, bounds, minimized/cloaked/fullscreen/workspace state when available, z-order when available, and timestamp/confidence. IDs are local to the current session and may be reused after a window is destroyed. Do not persist titles or document names.

Snapshot initially, then reconcile event deltas with periodic low-frequency repair where supported. Filter own windows and shell chrome. Every remove/move event validates object generation. When metadata expires, fade its visual influence rather than freezing a permanent invisible obstacle.

## Effects

`avoid-rectangles` uses inflated rectangles with configurable margin and rounded-distance approximation. `window-wake` leaves short-lived flow behind a moved rectangle. `edge-glow` colors nearby glyphs in the wallpaper plane only. `open-close-ripple` triggers on qualified lifecycle events. `focus-mood` changes a global palette based on focus state, not app content. All default off until enabled.

Compute forces from signed distance fields or nearby spatial bins rather than testing every cell against every historical window. Bound observed windows, event rate, influence radius, and simulation cost. A resize storm should update the latest shape, not spawn thousands of particles.

## Occlusion and pause

Estimate covered area from the union of eligible rectangles clipped to the output. Summing areas double-counts overlaps. Bounds do not describe transparency, rounded corners, holes, shadows, or actual occlusion precisely, so label the result estimated coverage.

Suggested default: pause after at least 98 percent estimated coverage sustained for 1000 ms; resume below 90 percent for 300 ms. These are proposed tunable heuristics, not OS guarantees. Unknown geometry must not be interpreted as fully visible or fully covered; use a user-selected conservative policy.

Fullscreen pause is a separate signal and may work without rectangles. Support per-output pause and a separate 'pause all outputs when an app rule matches' option. Never resume over a manual pause or lock reason.

## Permission and state transitions

When a window feed disappears, remove its capability, decay obstacles over a short interval, and keep the underlying animation functional. On lock, clear the snapshot immediately. On workspace switch, rebuild membership before generating move effects to avoid a burst of fake window motion.

## Test scenarios

Overlapping windows, maximized versus borderless fullscreen, translucent terminal, always-on-top app, desktop switch, rapid minimize/restore, drag across outputs, destroyed/reused handles, monitor rotation, stale data, and permission revocation. Pixel-perfect detection of every third-party application's shape is explicitly not promised.
