# Entrance, exit, and change animations

## Transition types

Support fade, ordered glyph dissolve, random-seeded reveal, scanline wipe, cell cascade, radial reveal, noise dissolve, and a restrained glitch transition. Entrance/exit animation is different from a continuous scene effect. Transitions have bounded duration, easing, direction, seed, and optional background-color behavior.

Use render-layer transitions to blend final outputs when sources have different grid sizes or glyph tables. Per-cell morphs are allowed only with compatible grids and explicitly remapped glyph IDs. A glyph index from one font atlas must never be interpreted against another.

## Transactional Apply

Prepare the new source and first frame without disturbing the current wallpaper. If loading succeeds, allocate a bounded overlap budget and transition from current to candidate. Commit the assignment only after a valid presented frame. On failure, keep the old wallpaper and show the error.

Repeated Apply requests use newest-wins cancellation. At most two active presentation states are retained; do not chain unbounded transitions. If a new request arrives mid-transition, take a current composite snapshot or finish/cut according to policy, then release obsolete decoders and textures.

## Timeline rules

The transition clock is monotonic. Pause/resume can freeze the current transition or complete immediately based on policy; lock/suspend chooses the safety path. Cross-output transitions share a phase where possible, but independent compositors/refresh rates are not guaranteed scanout-synchronized.

A stopped source should not continue decoding just because its exit opacity is zero. Drop outgoing resources as soon as their last contribution ends. Bake/export uses deterministic time and a fixed seed rather than wall-clock event timing.

## In and out boundaries

Entrance may run on Apply, user-start, or optional resume. Exit runs on a normal stop/change only while the host is healthy. Crash, forced termination, shutdown, session lock, or a broken surface cannot guarantee an exit animation. Never delay logout or retain a topmost window to finish one.

The desktop behind an outgoing wallpaper is the existing OS background unless the user selected a static fallback. Do not set the OS wallpaper to black solely to make a fade easier.

## Reduced motion and customization

Default duration 350 ms, configurable 0-5000 ms; zero is immediate. Reduced-motion preference replaces large displacement/flash/glitch effects with a short opacity change or no motion. Platform accessibility preferences can be observed, including macOS reduced motion. [S48](../research/source-register.md#s48)

High-contrast flashes and aggressive full-screen changes are disabled in safe presets. Do not claim a medically safe flash threshold without a dedicated assessment. Preview transitions independently before applying them to every output.

## Acceptance tests

Different grid/font/source combinations, cancel during preparation, memory pressure during overlap, rapid repeated Apply, exit while paused, force-kill, export at nondefault FPS, and reduced-motion changes mid-transition. The renderer must never reference a freed glyph atlas or a stale surface generation.
