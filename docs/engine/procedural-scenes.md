# Procedural scene system

## Scene contract

A built-in generator receives a deterministic seed, a typed parameter block, a fixed-step time context, dimensions, and approved input snapshots. It produces character cells or an intermediate field consumed by conversion. It must not open files, query native windows, access the network, or execute a package expression as code.

Generators advertise parameter schema, resource estimate, supported signal inputs, serialization version, reduced-motion behavior, and whether they support deterministic offline export. The same generator runs in preview, wallpaper, thumbnail, and export contexts.

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

These are proposed first-party engines, not third-party downloadable code. A community package selects a generator and parameters or composes allowed nodes. More complex art can be imported as an image/video or proposed as a reviewed Rust contribution.

## Simulation

Use a fixed simulation step with a maximum catch-up count and render interpolation where appropriate. Clamp exceptional elapsed time after resume. Scene randomness uses an explicitly specified deterministic generator and seed; unrelated effects should have independent streams so adding one node does not change every other random sequence.

For particles, enforce maximum counts, spawn rate, lifetime, velocity, and acceleration. Recycle buffers and limit collision iterations. A window rectangle update can modify a coarse obstacle field, but missing geometry must fade constraints away rather than delete particles.

## Determinism

A timeline export fixes seed, start time, time step, settings, font identity, and event track. A scene that reads real wall-clock time uses a chosen export timestamp. Scenes with live signals either require recorded synthetic input tracks or use a declared idle fallback. Never imply that an unrecorded interactive desktop session can be reproduced exactly.

## Creator affordances

Expose named parameter groups, useful bounds, reset actions, and tooltips. An artist should see how an effect responds in a local interaction preview without granting global input access. Include preset variations and per-generator performance estimates, clearly labeled as estimates until measured.
