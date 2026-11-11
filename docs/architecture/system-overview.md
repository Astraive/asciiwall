# System architecture

## Components and ownership

The `asciiwall` repository contains the engine, application, CLI, host adapters, package validator, and creator tooling. `asciiwall-library` contains artwork source, previews, contribution metadata, and static catalog/release inputs. The library repository consumes a pinned engine CLI release; it does not fork or duplicate the conversion implementation.

```text
Desktop UI / CLI / local scheduler
               |
        typed control commands
               v
  Controller and persistent state
      |         |           |
      |         |           +-- local package and catalog store
      |         +-------------- policy and capability evaluation
      v
 Native host process per adapter / GPU group
      |                 |
      |                 +-- bounded event stream from OS adapter
      v
 Scene simulation / image conversion / effects
      ^                 |
      |                 v
 Isolated media     AsciiFrame or raster layer
 worker                 |
      ^                 v
 local handles      wgpu compositor --> native desktop surface
```

This is a local process architecture. No HTTP server, TCP listener, cloud worker, or database service is needed. Same-user named pipes or Unix-domain sockets are transport for local commands only.

## Pipeline

A source produces either a raster frame, a character frame, or a procedural field. Raster content is normalized and converted when needed. A typed scene graph applies character-space effects before glyph rendering and pixel-space effects after rendering. The compositor combines layers and transitions. Presentation targets can be native wallpaper, UI preview, offscreen export, or a static fallback image.

A source-independent `FrameContext` carries monotonic time, fixed simulation step, seed, settings generation, and an approved snapshot of input/window signals. The engine must not query operating-system hooks or arbitrary files while rendering. It receives already-authorized data through contracts.

## Design choices

Use native Rust UI integration for the control application and isolate special desktop-surface ownership in platform hosts. `wgpu` is the common renderer, not a promise that every native surface works identically. UI integration versions must align with the chosen graphics version. [S24](../research/source-register.md#s24) [S25](../research/source-register.md#s25)

The first vertical slice may run in one process to reduce setup cost. Before general community-package support, establish a separate media-worker boundary and host crash recovery. Do not present process separation alone as a security sandbox: OS restrictions, handle discipline, budgets, and network denial are separate requirements.

## Failure containment

If a decoder exits, the host retains the last safe frame or a poster and the controller stays responsive. If the host exits, the controller can reattach with a bounded retry budget. If the controller exits, child hosts must detect control-channel loss and stop or honor a short configured grace period; they must not become immortal orphan wallpapers.

The OS remains the ultimate authority for focus, input dispatch, stacking, and lock state. A wallpaper must never override it to preserve an effect.

## Shell-hosted presenter exception

The default standalone presenter is wgpu. Plasma and GNOME may require a small host-native presenter because Qt Quick/GNOME Shell owns the desktop scene graph. Keep conversion, timing, package semantics, and data models shared; prototype the bridge before promising pixel/performance parity. See [Plasma](../platform/linux-kde-plasma.md) and [GNOME](../platform/linux-gnome.md). [S56](../research/source-register.md#s56) [S57](../research/source-register.md#s57)
