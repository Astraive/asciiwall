# GPU glyph rendering and compositing

## Render design

Rasterize glyph masks into an atlas, upload a compact cell buffer, and draw instanced quads. The shader derives the cell position from instance index or explicit layout data and samples the matching glyph mask. A full scene is not thousands of UI text widgets.

CPU cells can use a 12-byte explicit record, while GPU buffers may use padded/aligned records such as 16 bytes according to the selected shader/storage layout. Never assume Rust struct layout matches WGSL. Add layout-size and roundtrip tests.

## Passes

A baseline frame performs background fill, character rendering, optional layer composition, and optional post-processing. Static scenes keep cached resources and only redraw after invalidation. Effects that need offscreen targets request them explicitly; a trivial wallpaper must not pay for bloom, trails, multiple full-resolution buffers, or unnecessary readbacks.

The shared engine renders both wallpaper and editor preview. A preview may use a smaller grid or quality setting for responsiveness, but it must disclose that difference and offer a final-quality preview.

## Resource ownership

The host owns the native surface and its lifetime. The renderer queries surface formats, present modes, adapter limits, and feature support. Select conservative supported defaults; do not assume all backends offer the same capabilities. Handle resize, zero-sized/minimized surfaces, lost/outdated surfaces, GPU device loss, and monitor migration explicitly. [S24](../research/source-register.md#s24)

Avoid creating a graphics device for every frame or slider change. Share immutable atlases where compatible. Across physical GPUs, share semantic frame data rather than assuming textures can be shared trivially.

## Color and sampling

Use mask coverage consistently and avoid double-applying gamma. Atlas packing needs padding and appropriate filtering to prevent adjacent-glyph bleeding. Pixel-snapped layouts can improve legibility; fractional scaling needs a deliberate policy rather than rounded positions that jitter each frame.

A cell can show foreground and background colors. Transparent layering requires premultiplied-alpha rules shared with the compositor. Unicode symbols are glyphs in the engine's table, not terminal escape sequences.

## Presentation and power

Cap render frequency independently of display refresh. A 144 Hz display should not force a slow ambient scene to simulate at 144 Hz. Request redraw only for deadlines or invalidations. Use compositor feedback/present pacing where available but keep a robust monotonic scheduler. Do not busy-wait for frame deadlines.

## Fallback and validation

The CPU converter and static CPU rasterizer remain available for text/PNG exports and diagnostics. A missing GPU does not imply animated software rendering at acceptable cost. Fall back explicitly to static wallpaper or preview-only mode.

GPU golden tests use perceptual tolerances and known fonts; CPU character-grid tests can use exact results under a pinned implementation. Capture device/backend and shader versions with benchmark evidence. User packages cannot submit arbitrary WGSL in the stable format.

## Host-native presentation bridges

A shell-owned QML/GJS background is not automatically a native wgpu surface. Such adapters may present bounded offscreen frames or a compact glyph grid through host-native drawing. Treat these as separately tested presenters implementing the same visual contract, not as evidence of zero-copy cross-API texture sharing. [S56](../research/source-register.md#s56) [S57](../research/source-register.md#s57)
