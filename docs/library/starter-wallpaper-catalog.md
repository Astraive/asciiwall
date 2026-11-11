# Starter wallpaper catalog and recipe briefs

## Status

These are **36 proposed original recipe briefs**, not 36 generated wallpapers or downloaded artworks. The first 20 can be expressed using the draft built-in node set; eight require rights-cleared media; eight require later engine capabilities. Each becomes publishable only after assets, preview, attribution, package validation, and performance review exist.

| Slug | Collection | Source / generator | Design and fallback | Readiness |
|---|---|---|---|---|
| matrix-rain | Rain | matrix-rain | Optional click ripple; no observer needed for base scene | v1 |
| quiet-waves | Minimal | waves | Slow, low-density monochrome, static reduced-motion fallback | v1 |
| deep-starfield | Space | starfield | Optional pointer parallax with bounded offset | v1 |
| wireframe-cube | Geometry | cube-wireframe | Slow rotation, adjustable scale, no global inputs | v1 |
| mist-field | Abstract | noise-field | Soft density motion, no high-contrast flashes | v1 |
| amber-terminal | Minimal | matrix-rain | Amber palette and sparse streams | v1 |
| paper-waves | Minimal | waves | Dark glyphs on light background | v1 |
| blue-hour | Abstract | noise-field | Duotone low-frequency drift | v1 |
| constellation-drift | Space | starfield | Lower star count and slow depth | v1 |
| forest-rain | Rain | matrix-rain | Green palette and long trails | v1 |
| magnetic-stars | Reactive | starfield | Pointer attraction optional; neutral without feed | v1 |
| desktop-ripples | Reactive | waves | Click ripples optional, desktop-only scope | v1 |
| window-wake | Reactive | noise-field | Window avoidance optional, explicitly limited hosts | v1 |
| monochrome-orbit | Geometry | cube-wireframe | White line geometry on black | v1 |
| solar-haze | Abstract | noise-field | Warm tones with constrained contrast | v1 |
| ocean-lines | Minimal | waves | Layered waves via binary composites | v1 |
| rain-duet | Rain | matrix-rain | Two independently seeded layers within budget | v1 |
| still-night | Low power | starfield | Frozen seed with event-driven redraw | v1 |
| paper-cube | Low power | cube-wireframe | Static geometric composition | v1 |
| slow-tide | Low power | waves | 10 FPS preset; no trails/bloom | v1 |
| mountain-study | Image | image | Original or CC0 landscape, edge-aware conversion | asset-needed |
| botanical-study | Image | image | Original leaf photograph, attribution reviewed | asset-needed |
| city-outline | Image | image | Original architectural silhouette, crop-safe | asset-needed |
| portrait-study | Image | image | Consented original portrait, privacy reviewed | asset-needed |
| cloud-loop | Video | video | Original short cloud clip, loop boundary tested | asset-needed |
| ocean-loop | Video | video | Original/authorized waves with muted playback | asset-needed |
| ember-loop | Video | ascii-clip | Precomputed glyph clip, editable palette only | asset-needed |
| ink-loop | Video | rendered-video | Baked pixel video; limits clearly labelled | asset-needed |
| lorenz-attractor | Math | future generator | Bounded deterministic ODE integration | later |
| boid-garden | Simulation | future generator | Particle cap and pointer influence | later |
| reaction-diffusion | Simulation | future generator | Explicit compute budget and stable timestep | later |
| cellular-life | Simulation | future generator | Fixed grid and deterministic seed | later |
| ascii-fire | Simulation | future generator | No flashing default, static fallback | later |
| wireframe-torus | Geometry | future generator | 3D glyph rasterization research | later |
| audio-spectrum | Audio | future signal | Chosen-file analysis; no microphone default | later |
| day-night-field | Schedule | future rule preset | Local clock palette with timezone semantics | later |

## Collection strategy

Launch with a small high-quality, low-power set rather than a large gallery of near-duplicates. Every featured scene should have portrait/wide layouts, restrained defaults, no dependency on optional global observation, and clear advanced controls. Reactive variants should still look intentional with their feeds disabled.

For image/video entries, create or obtain source material with explicit redistribution permission. Do not fill the gallery with movie/game clips simply to increase its count. Keep the original source and conversion settings in the recipe when rights permit, so contributors can reproduce and improve the result.

## Required metadata per recipe

Name, short description, author, license, source provenance, tags, minimum engine feature set, optional capabilities, quality presets, low-power alternative, reduced-motion alternative, preview, and measured resource notes from a named reference machine. Never invent benchmark badges before measuring.

## Acceptance examples

`desktop-ripples` must continue as ordinary waves without pointer permission. `window-wake` must disclose that plain Wayland may not supply window rectangles. `ink-loop` must not show editable glyph controls that cannot affect its baked pixels. `still-night` must stop continuous redraw when unchanged.
