# Pointer and click reactions

## Event contract

Normalize events into `PointerMoved`, `ButtonPressed`, `ButtonReleased`, `PointerEnteredOutput`, `PointerLeftOutput`, and `FeedUnavailable`. Each contains monotonic timestamp, output/space, position when known, button where relevant, and confidence. Do not retain a raw device log.

The wallpaper remains non-focusable and pass-through. Input observation is passive. An observation feed is not permission to consume or synthesize input, and a pass-through surface is not automatically an observer. [S07](../research/source-register.md#s07) [S08](../research/source-register.md#s08) [S11](../research/source-register.md#s11) [S16](../research/source-register.md#s16)

## Reaction repertoire

| Reaction | Parameters | Stability guard |
|---|---|---|
| Attraction/repulsion | Radius, force, falloff, damping | Clamp acceleration and maximum speed |
| Ripple | Radius, propagation speed, amplitude, decay | Maximum concurrent ripples and duration |
| Glyph reveal | Brush radius, density, hold/fade | Restore underlying immutable frame |
| Magnetic text | Displacement, spring stiffness, friction | Maximum cell displacement |
| Particle burst | Count, spread, speed, seed | Global particle budget |
| Trail | Lifetime, spacing, opacity | Bounded ring buffer |
| Parallax | Depth, smoothing, maximum offset | Clamp outside output bounds |
| Click color pulse | Palette, duration, strength | Reduced-motion/flash-sensitive alternative |

Effects are built-in data-driven operators. The package chooses defaults within allowed bounds; users can disable or tune them. No arbitrary event callback script is loaded from a package.

## Scope

Offer `desktop-only` and explicit `global-observed` scopes only where supported. The default is desktop-only. A reliable classification is required: clicks over icons, menus, or other apps should not generate wallpaper responses unless the user specifically chooses broader observation. Unknown classification suppresses the reaction.

Do not interfere with double-click timing, drag-selection, icon dragging, context menus, taskbars, stylus gestures, or accessibility tools. Left/right/middle buttons are independently selectable. Wheel reaction is off by default because scrolling an application should not unexpectedly animate the desktop.

## Timing and event floods

Coalesce pointer motion to the latest position for each simulation step. Preserve bounded button transitions but rate-limit effect creation. At high polling rates, input volume must not increase physics updates above the configured simulation rate. Ignore stale events after pause, lock, or output reattachment.

Use timestep-based integration, not one force impulse per received mouse message. Limit maximum delta after stalls and reset trail interpolation when the pointer teleports between displays or a feed resumes.

## Preview and diagnostics

The editor preview supplies synthetic local pointer events without enabling global observation. This lets every platform demonstrate the scene even when desktop-wide reactions are unavailable. Label preview-only behavior clearly.

A debug overlay may show coordinate transforms and hit-policy result only after the user enables diagnostics. It is never written into exported artwork or logs by default. Acceptance tests include a 1000 Hz input source, negative monitor origins, missed releases, rapid toggle, and denied observer access.
