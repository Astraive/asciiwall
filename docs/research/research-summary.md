# Research findings and revised engineering direction

**Research checked:** 2026-11-11. Primary documentation and project sources were used; no desktop adapter was executed in this environment.

## Conclusions

The core engine is feasible with ordinary local media decoding, an original Rust character matcher, and GPU glyph rendering. Existing symbol-rendering tools and structure-based ASCII research establish substantial prior art; the distinctive product opportunity is integrated editing, desktop behavior, and portable interactive artwork, not claiming to invent image-to-text rendering. [S24](source-register.md#s24) [S26](source-register.md#s26) [S27](source-register.md#s27) [S38](source-register.md#s38) [S39](source-register.md#s39)

The hard parts are OS integration, permissions, energy discipline, and community-package safety. Treat these as first-class subsystems instead of adding them after the visual engine is finished.

## Corrections to the preliminary architecture

| Earlier simplification | Refined recommendation |
| --- | --- |
| All desktop backgrounds work similarly | Publish separate host and event capabilities per OS/compositor |
| A click-through wallpaper can receive every click | Passive observation and direct input are different features, with different permissions |
| `winit` creates all background surfaces | Use it for ordinary UI windows; special hosts may own native surface/event loops |
| One process is always simplest | Prototype in one process, then isolate native hosts and untrusted decoders behind local IPC before broad community import |
| `ffmpeg-next` is simply obsolete | Research found current published documentation; evaluate both wrappers behind a replaceable decoder boundary |
| Every Wayland environment supports layer-shell | Negotiate protocol availability; KDE and GNOME need distinct host strategies |
| Pre-rendered ASCII remains fully editable | Keep baked raster video separate from editable character frames |
| No networking in the app is incompatible with a public library | Local playback can coexist with explicit static-catalog download support |

The dependency snapshot showed `egui-wgpu 0.37.0` aligned with `wgpu ^30.0`; it also showed published versions for both FFmpeg Rust wrapper candidates. These observations are research inputs, not a working lockfile or a build verification. [S25](source-register.md#s25) [S29](source-register.md#s29) [S30](source-register.md#s30)

## Highest-risk assumptions to test first

Windows desktop attachment must survive Explorer restart, Show Desktop, wallpaper changes, mixed DPI, and shell variants. Project issue evidence shows this area evolves. macOS background ordering must be tested with Finder, Mission Control, Spaces, Stage Manager, and separate displays. Wayland availability must be negotiated from actual advertised protocols rather than inferred solely from a desktop name. [S03](source-register.md#s03) [S09](source-register.md#s09) [S15](source-register.md#s15) [S16](source-register.md#s16) [S50](source-register.md#s50)

Window-aware collision is plausible from coarse geometry on some hosts. It is not exact per-pixel occlusion, and generic Wayland toplevel listing is not a global geometry feed. The product must accept `unknown` visibility rather than manufacturing precision. [S05](source-register.md#s05) [S13](source-register.md#s13) [S17](source-register.md#s17) [S18](source-register.md#s18)

## Research limits

Vendor pages can lag shipping software and project issues can describe individual failures. No benchmark in this pack is an observed measurement. No published source proves that AsciiWall's future use of a given API is bug-free. The release matrix must be based on named OS builds, compositor versions, GPUs, and explicit manual/automated evidence.

See the source register for the scope and strength of each reference, and the spike plan for what is still unresolved.
