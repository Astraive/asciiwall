# Start here

## The recommendation

Build a **Rust engine with native desktop hosts**, not a browser wallpaper wrapper. Make the core usable from the desktop application and a local CLI. Treat live ASCII, cached character animation, and rendered video as distinct representations with explicit editing capabilities.

Start implementation with two parallel proof areas: image conversion in a normal GPU window, and a minimal native wallpaper-surface spike. This avoids spending months building an editor before discovering an integration blocker. Windows is the first product target; macOS and selected Linux adapters follow feature-by-feature acceptance gates, not a blanket cross-platform promise.

## Reading order

1. [Research summary](research/research-summary.md) and [feasibility matrix](research/feasibility-matrix.md): what is possible, restricted, or still experimental.
2. [System architecture](architecture/system-overview.md), [repository layout](architecture/repository-structure.md), and [stack decisions](tech-stack/stack-decisions.md): what to build and where it belongs.
3. [Settings reference](settings/settings-reference.md), [interactions](interaction/pointer-and-click-reactions.md), and [window awareness](interaction/window-aware-behavior.md): the behavior users actually experience.
4. [Package specification](package/format-specification.md) and [library repository](library/repository-structure.md): how artwork is shared safely.
5. [Milestones](delivery/milestones-and-dependencies.md), [implementation backlog](delivery/implementation-backlog.md), and [acceptance criteria](quality/acceptance-criteria.md): the execution order and evidence needed before release.

## Decisions that avoid expensive mistakes

**Desktop integration is not just a window flag.** Windows WorkerW embedding is shell-sensitive; a Wayland background surface does not gain global pointer or window access; a Plasma wallpaper plugin is not the same as a generic wgpu window. [S02](research/source-register.md#s02) [S03](research/source-register.md#s03) [S15](research/source-register.md#s15) [S16](research/source-register.md#s16) [S20](research/source-register.md#s20)

**Keep appearance settings separate from permissions.** Imported art may suggest ripple strength; it must never silently turn on global input observation, microphone capture, autostart, network access, or arbitrary file access.

**A rendered video is no longer an editable grid.** Its glyph shapes, grid density, and source conversion are baked in. You can still apply supported presentation effects and live overlays, but re-editing the conversion requires the source or a character clip.

**No owned backend does not mean no distribution infrastructure.** GitHub can host source and downloadable artifacts, while installation and playback remain local. Offline installations can import a package or catalog snapshot from disk. [S41](research/source-register.md#s41) [S42](research/source-register.md#s42)

**Do not ship dangerous extension power merely to say full customization.** A declarative scene/effect graph can expose extensive controls without executing community code. More powerful extension models are a later security decision.

## First concrete outcome

A user selects a local image, chooses a font and character grid, compares density and structure-aware conversion, saves a preset, previews the GPU result, applies it on one verified Windows display, and imports/exports the same work as a validated `.asciiwall` package. The app restores or leaves the underlying desktop cleanly on stop.
