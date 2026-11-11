# System architecture overview

> **Design baseline:** This document summarizes the proposed architecture across [system overview](architecture/system-overview.md), [crate boundaries](architecture/crate-boundaries.md), [repository structure](architecture/repository-structure.md), [ADR 0001–0004](decisions/0001-native-rust-engine.md), and [stack decisions](tech-stack/stack-decisions.md). No Rust workspace, executable, or native host integration has been built yet.

## Two-repository and offline-core separation

AsciiWall splits application engineering from curated artwork governance across two MIT-licensed public repositories:

- **`Astraive/asciiwall` (main application & engine):** Owns the portable Rust engine, `egui` desktop controller, native host adapters, restricted media worker, local CLI, `.asciiwall` validator/packer, and compatibility tests (`278` local backlog tickets).
- **`Astraive/asciiwall-library` (community artwork catalog):** Owns declarative starter recipes, asset rights/attribution ledgers, previews, and signed static catalog metadata (`78` local backlog tickets). It consumes pinned engine CLI contracts (`PKG-001`, `CLI-001`, `PKG-010`, `PKG-012`) rather than duplicating conversion or validation algorithms ([ADR 0004](decisions/0004-offline-core-and-static-library.md)).

Normal playback, image/video conversion, scene simulation, preset editing, local gallery indexing, and package import/export run **100% offline** with zero account, telemetry, database server, or hosted backend (`EC-091`). Optional static catalog browsing and downloads (`library.online-catalog`) default to `"disabled"` and require explicit user consent.

## Local process topology and trust boundaries

As specified in [system overview](architecture/system-overview.md) and [process model and IPC](architecture/process-model-and-ipc.md), runtime privileges and failure domains are separated into local same-user processes:

```text
Desktop UI (egui) / CLI (asciiwall) / local scheduler
                 |
       versioned local IPC (<= 1 MiB, same-user/session auth)
                 v
   Controller & persistent state (settings, overrides, local library index)
        |                    |
        |                    +-- policy, capabilities & requested/effective resolution
        v
  Native host process per adapter / GPU group (surface lease + bounded OS event stream)
        |                                  ^
        v                                  | (validated owned frames / generations)
  Scene simulation / conversion / effects <--> Isolated media-worker (local handles only)
        |
        v
  wgpu glyph compositor (or host-native shell bridge on Plasma/GNOME) --> Display surface
```

Key architectural invariants:
- **No network listeners or shell injection:** Same-user named pipes (Windows) or Unix-domain sockets (macOS/Linux) authenticate peer identity (`SEC-001`, `EC-103`) and enforce a `1 MiB` control cap. No HTTP server, TCP socket, Explorer injection, or root service is used.
- **Worker confinement:** Untrusted media decoding (`IMG-*`, `VID-*`) and staged archive resource validation (`PKG-007`) execute in `media-worker` under OS restrictions and explicit byte/pixel/time budgets (`SEC-002`–`SEC-005`). A worker crash retains the last safe frame/poster without hanging the controller.
- **Generation discipline:** Every source load, seek, Apply, or surface resize increments a monotonic generation ID so stale asynchronous frames or resize events are dropped safely (`COR-002`, `EC-024`, `EC-084`).
- **Host-native presenter exception:** `wgpu` is the default standalone presenter, whereas KDE Plasma (Qt Quick) and GNOME Shell (GJS/Mutter) own their desktop scene graphs and require explicit, separately benchmarked shell bridges (`LNX-004`–`LNX-008`) without decoding untrusted media inside the shell process.

## Proposed Cargo workspace and crate boundaries

Once `PRJ-001` establishes the workspace, crates are introduced only when exercised by real behavior ([crate boundaries](architecture/crate-boundaries.md)):

| Component | Proposed path | Primary responsibility | Negative boundary (must NOT own) |
|---|---|---|---|
| Core primitives | `crates/ascii-core` | `AsciiFrame`, `RasterFrameView`, packed colors, `Budget`, `Error`, `CancellationToken`, opaque IDs | File I/O, FFmpeg, UI, native windows, networking, singletons |
| Conversion engine | `crates/ascii-convert` | `fontdue` glyph masks/tables, aspect/fit math, color/alpha normalization, density & structural matchers | FFmpeg setup, OS hooks, permissions, desktop surfaces |
| Media decoder | `crates/ascii-media` | `ReadSeek`, `DecodeLimits`, image/FFmpeg adapters, `.ascii-clip` codec, worker runner | Wallpaper placement, artistic conversion algorithms |
| GPU & offscreen renderer | `crates/ascii-render` | Padded `wgpu` glyph atlases, character/pixel effect passes, offscreen PNG/frame bake | File browsing, policy decisions, arbitrary user shaders |
| Scene & effects | `crates/ascii-scene` | Fixed-step seeded generators (`SCN-002`–`007`), draft v1 scene graph, transitions | Direct OS queries, filesystem scans, network calls |
| Runtime & policy | `crates/ascii-runtime` | Clocks, `2`–`3` frame bounded queues, pause-reason arbitration, aggregate budgets | UI widgets, platform FFI |
| Platform adapters | `crates/ascii-platform` | Capability discovery, `SurfaceLease`, Windows/macOS/X11/Wayland adapters & bounded feeds | Package parsing, conversion logic |
| Package contracts | `crates/ascii-package` | Draft v1 manifest/scene schemas, ZIP bomb/traversal validation, staged atomic import/export | Shell execution, permission grants |
| Local library | `crates/ascii-library` | Immutable import receipts, search index, optional signed static catalog client | Core rendering, telemetry, background downloads |
| Typed settings | `crates/ascii-config` | 172-key registry, layered precedence (`P`/`A`/`O`/`S`), TOML persistence, reversible migrations | Policy bypasses, host detection |
| Local IPC | `crates/ascii-ipc` | Versioned envelopes, peer authentication, validated buffer handoff | Unvalidated peer file paths |
| Control UI | `crates/ascii-ui` | `egui` navigation, live preview, requested/effective inspector, undo/Apply flows | Duplicate conversion/matching implementations |
| Executables & bridges | `apps/{desktop,host,media-worker,cli}`, `integrations/{plasma,gnome}`, `xtask` | Process composition, CLI (`ascii-cli`), shell companions, repo automation | Re-implementing core library invariants |

## Five source representations and pipeline

AsciiWall separates five distinct source kinds ([mode comparison](product/mode-comparison.md)) rather than pretending baked video and editable scenes share identical capabilities:

1. **`scene`:** Deterministic procedural generator/simulation (`matrix-rain`, `starfield`, `waves`, `noise-field`, `cube-wireframe`, `solid`) evaluated directly into character frames with fixed-step seeded reproducibility (`SCN-001`–`007`).
2. **`image`:** Bounded PNG/JPEG/still-WebP decoded once in a worker (`IMG-001`, `IMG-002`), preprocessed with single-pass EXIF orientation and sRGB-to-linear luminance (`0.2126R + 0.7152G + 0.0722B`), converted to glyphs, and held idle without continuous redraw unless time/event effects are enabled.
3. **`video`:** Live FFmpeg demux/decode (`VID-001`–`VID-006`) scheduled by rational presentation timestamps (`PTS`), bounded `2`–`3` frame queues, and per-frame conversion with temporal stabilization (`CNV-010`).
4. **`ascii-clip`:** Precomputed `.ascii-clip` binary frame stream (`64`-byte header, `40`-byte index entries, `12`-byte cell structs, optional `zstd` compression, `VID-007`–`VID-009`) supporting glyph/palette restyling without re-running source conversion.
5. **`rendered-video`:** Baked raster video already displaying ASCII pixels (`VID-013`). Supports presentation overlays, crop, and opacity, while explicitly disabling grid/conversion controls in the UI (`EC-071`–`EC-080`, mode honesty).

## Core architecture decisions and package safety

- **[ADR 0001 — Own Rust engine](decisions/0001-native-rust-engine.md):** Own glyph analysis, conversion, scenes, and scheduling in Rust; rely on `image`, `fontdue`, `wgpu`/`egui`, and a replaceable FFmpeg wrapper (`ffmpeg-the-third` candidate vs `ffmpeg-next`) for codecs and GPU abstraction.
- **[ADR 0002 — Capability-aware native hosts](decisions/0002-capability-aware-native-hosts.md):** Separate `SourceCapabilities` from `PlatformCapabilities` (`available`, `permission-required`, `unavailable`, `experimental`, `temporarily-unavailable`). Never fake click-through observation or cover desktops with topmost fullscreen windows.
- **[ADR 0003 — Declarative `.asciiwall` packages](decisions/0003-declarative-portable-packages.md):** Bounded ZIP archives (`<= 1 GiB` compressed, `<= 2 GiB` expanded, `<= 4,096` entries, lowercase ASCII relative paths, strict Draft 2020-12 manifest/scene schemas). Stable v1 excludes executable plugins, arbitrary shaders, scripts, and WebAssembly; packages contain only `P` visual defaults and cannot alter `A` privacy/permission settings.
- **[ADR 0004 — Offline core and static library](decisions/0004-offline-core-and-static-library.md):** Local operation never requires internet access; optional catalog distribution uses immutable versioned release assets and exact-byte Ed25519 detached signatures ([source S60](research/source-register.md#s60)).
