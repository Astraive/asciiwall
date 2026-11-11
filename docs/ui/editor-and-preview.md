# Editor, preview, and customization UX

## Preview fidelity

The preview uses the same conversion algorithms, glyph tables, settings resolver, and scene time model as desktop rendering. It may use a lower temporary grid/FPS for responsiveness, but must show that approximation and offer a full-quality still frame.

A preview canvas simulates local cursor/click/window obstacles without enabling global observation. Provide an example draggable rectangle to demonstrate window avoidance. Clearly label this as simulation, not evidence that the current OS adapter can supply window geometry.

## Control grouping

Basic: source, preset, colors, character density, speed, Apply. Advanced: sampling, calibrated glyph density, structural weights, dithering, temporal stability, alpha, font metrics, reaction force/radius, transition policy, and per-node graph properties.

Each slider has a typed numeric entry, unit, bounds, reset button, default marker, and effective-value explanation. A control disabled by a capability still exposes help text. Expensive controls debounce changes and show a generation-aware preview state.

## Comparison tools

Offer original raster versus ASCII, density versus structural matching, before/after settings, and a paused frame inspector. Inspect a cell's chosen glyph, luminance, foreground/background, and matching score in developer mode without logging source pixels.

Keep color-space/crop/grid choices visible; comparing two different crop or scaling setups is not an honest algorithm comparison. Use fixed seeds and identical time when comparing procedural variants.

## Editing transactions

Draft changes are separate from the active wallpaper. Apply validates, prepares resources, transitions, then commits. Undo/redo manipulates the draft. Revert loads the saved revision. Simultaneous CLI edits create a conflict resolution prompt instead of silent overwrite.

Offer 'Apply to selected output' by default. 'Apply to all' requires an explicit choice and explains independent fitting versus spanning. Customizations should be saved as local variants, not destructive edits to signed package bytes.

## Source repair

A moved local file shows its previous basename and a Locate action. Do not recursively scan the user's drive or upload hashes to find it. A font missing on another machine falls back only with clear notice; portable export should include an approved font or resolve to a built-in choice.

## Export preview

Show the exact exported resolution/grid, frame rate, duration, color mode, source kind, included assets, and license summary. Text export may lose foreground/background richness; baked video loses editable character structure. Warn before changing modes and keep source originals untouched.
