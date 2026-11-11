# Effects and declarative scene graph

## Stable v1 graph

The draft v1 portable graph has character-frame outputs, a fixed built-in node registry, and explicit input slots. Raster source nodes include conversion internally; generators have no inputs, unary effects use slot 0, and the binary composite uses slots 0 and 1. The [scene schema](../package/scene-schema.md) is the authoritative portable contract. The loader validates IDs, arity, bounds, acyclicity, and output reachability.

## Later typed-graph architecture

The internal engine may evolve toward typed ports for raster, character grid, scalar/vector fields, event impulses, palettes, and presentation layers. Those richer ports are not accepted by the v1 package schema. Publishing them requires a versioned schema/feature extension and explicit type checks. Cycles remain forbidden unless a future bounded-feedback node owns its own state.

A package contains data selecting built-in node kinds and validated parameters. It does not contain Rust, Lua, JavaScript, shell commands, arbitrary mathematical expressions, or user shaders. This enables rich customization without making every wallpaper an executable extension.

## Effect stages

**Before conversion:** crop, resampling, exposure, tone adjustment, edge extraction, and palette preparation.

**Character space:** glyph substitution, density modulation, cell displacement, color waves, bounded glitch, ripple distortion, character trails, and particle overlays.

**Pixel space:** opacity, tint, vignette, scanline simulation, restrained bloom, crossfade, and approved compositing.

Do not claim equivalent results when the same visual label operates in different spaces. A pixel-space ripple on baked video is not a character-grid ripple.

## Ordering and state

Graph order is explicit, stable, and included in cache keys. A temporal effect owns history tagged by source generation, grid size, font table, and timeline epoch. Any incompatible change resets or migrates that history deliberately. Trail buffers must decay while active and be cleared on privacy/session boundaries.

## Parameter binding

In v1, built-in reaction nodes own their parameterized signal mapping, and exposed UI controls refer only to the approved portable settings registry. There is no arbitrary graph-binding expression field in the v1 schema.

A later explicit binding schema may map approved signals through bounded scale, offset, clamp, easing, smoothing, and dead-zone operators. A pointer x coordinate could drive parallax or window-move speed could drive wave amplitude, subject to a safe maximum. That is an extension proposal, not an already specified portable v1 language.

Bindings declare a missing-signal fallback, such as zero force or a slow autonomous oscillation. A requested optional capability must not turn into an automatic permission prompt or a silent fallback to broad capture.

## Resource accounting

The graph compiler estimates intermediate textures, buffer counts, work per cell, particles, and history size. It rejects impossible dimensions and warns when the selected quality profile cannot fit the declared budget. Runtime monitors actual cost and can reduce only settings explicitly marked adaptive. It must not silently alter artistic constants such as palette or random seed.

## Extensibility

New node types require engine support, a versioned parameter schema, deterministic tests, resource limits, and reduced-motion behavior. A future plugin system is a separate security project, not an unofficial escape hatch hidden in a JSON field.
