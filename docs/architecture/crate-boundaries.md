# Crate boundaries and dependency contracts

| Crate | Owns | Must not own |
| --- | --- | --- |
| `ascii-core` | frame ids, packed colors, grid invariants, timestamps | I/O, native hooks, global singletons |
| `ascii-convert` | preprocessing, glyph descriptors, matching, temporal state | FFmpeg setup, windows, user permissions |
| `ascii-media` | demux/decode abstraction, orientation/color metadata | wallpaper placement, artwork algorithms |
| `ascii-render` | atlas, GPU resources, render plans, offscreen export | file browsing, arbitrary scripts, policy decisions |
| `ascii-scene` | typed nodes, deterministic simulation, effect ordering | direct OS reads or networking |
| `ascii-runtime` | clocks, queues, budgets, pause reasons, task cancellation | app-specific UI widgets |
| `ascii-platform` | surfaces, monitor topology, permitted event sources | package parsing or source conversion |
| `ascii-package` | manifests, safe archives, compatibility, transactions | shell execution or automatic permission grants |
| `ascii-library` | install registry, static catalog, attribution | core rendering or hidden telemetry |
| `ascii-config` | defaults, schema, validation, provenance | policy bypasses or host detection |
| `ascii-ipc` | versioned messages, authenticated local channel helpers | trust in arbitrary peer-provided paths |
| `ascii-ui` | controls, user flows, accessible feedback | alternate copies of conversion algorithms |

## Public interface design

Use opaque IDs for installed packages, monitors, resources, and running sessions. Avoid exposing raw native handles beyond the narrow unsafe adapter boundary. Native surface handles must carry an owner whose lifetime outlives the GPU surface. Dropping a raw handle while the renderer still refers to it is a correctness failure, not an acceptable recoverable event.

Converters accept borrowed normalized image views and reusable scratch buffers. They return structured errors and cancellation progress. A reusable engine does not call `process::exit`, open dialogs, or log user filenames by default. Resource limits are explicit inputs, not hidden constants scattered across codecs.

Separate `SourceCapabilities` from `PlatformCapabilities`. A Windows host might supply global pointer observation while a baked video still cannot change its character grid. Conversely an editable scene might support a window-collision node but run on a host without window geometry; that node needs a fallback.

## API evolution

Keep internal Rust types flexible before the first stable library release. The package schema and CLI machine-readable output are separate compatibility surfaces with explicit versions. Avoid promising stable Rust ABI between separately compiled plugins. First-party adapters are built together with the engine; external code plugins are not part of the stable package contract.

## Testing boundaries

Each crate has unit tests for its invariants. Contract tests ensure fake and real decoders present consistent timestamps, platform adapters express unavailable data as unavailable, and package validation is identical in UI and CLI. A test-only deterministic clock and PRNG source make scene tests reproducible without patching production algorithms.
