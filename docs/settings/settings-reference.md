# Complete proposed settings registry

## Status and scope

This is the proposed control registry, not a list of settings already implemented. Defaults are design choices. `P` means a portable visual default that a package may suggest; `A` means application/user policy; `O` means local output assignment; `S` means temporary session state. No package can set A/O/S controls. Later controls are explicitly excluded from the stable v1 portable schema.

All numbers are finite. JSON rejects NaN/Infinity; UI and TOML parsing must reject them too. Bounds are validation limits, not promises of affordable performance at the maximum. Cross-field and global resource constraints still apply. A disabled capability retains the requested value but reports the effective fallback.

Change classes describe invalidation: uniform, scene/runtime, conversion, atlas, decoder, scheduler, surface, observer, controller, UI, job, or restart. Expensive changes are prepared off-thread and swapped transactionally. No dynamic control writes directly to an unsafe native handle.


## Grid

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `grid.columns` | `240` | integer; 32 to 1024 | P / conversion / v1 | Requested columns; policy may reduce effective density. Capability: none. |
| `grid.rows` | `0` | integer; 0 to 1024 | P / conversion / v1 | Zero derives rows from image aspect and measured cell metrics. Capability: none. |
| `grid.cell-aspect` | `0` | number; 0 to 4 | P / conversion / v1 | Zero uses actual font advance/line height; nonzero is explicit width/height. Capability: none. |
| `grid.fit` | `"contain"` | string; contain, cover, stretch | P / conversion / v1 | Stretch is an explicit distortion choice. Capability: none. |
| `grid.anchor-x` | `0.5` | number; 0 to 1 | P / conversion / v1 | Normalized crop/letterbox anchor. Capability: none. |
| `grid.anchor-y` | `0.5` | number; 0 to 1 | P / conversion / v1 | Normalized crop/letterbox anchor. Capability: none. |
| `grid.padding-cells` | `0` | integer; 0 to 64 | P / conversion / v1 | Inset within output; content cannot allocate beyond grid caps. Capability: none. |

## Font

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `font.face` | `"builtin-mono"` | string; <= 80 characters; semantic checks | P / atlas / v1 | Approved built-in ID; package font is a manifest asset, never a system path. Capability: none. |
| `font.pixel-size` | `16` | integer; 6 to 128 | P / atlas / v1 | Atlas raster size, independent from final display scaling. Capability: none. |
| `font.line-spacing` | `1` | number; 0.5 to 3 | P / atlas / v1 | Multiplier on measured line advance. Capability: none. |
| `font.letter-spacing` | `0` | number; -2 to 8 | P / atlas / v1 | Extra logical pixels between cell advances; must retain positive width. Capability: none. |
| `font.antialias` | `true` | boolean; true / false | P / atlas / v1 | Grayscale coverage; no subpixel RGB assumptions. Capability: none. |

## Charset

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `charset.mode` | `"ascii"` | string; ascii, blocks, braille, custom | P / atlas / v1 | Unicode modes are labelled distinctly from ASCII. Capability: none. |
| `charset.custom` | `" .:-=+*#%@"` | string; <= 4096 characters; semantic checks | P / atlas / v1 | Validated glyph repertoire; duplicates normalized, controls rejected. Capability: none. |
| `charset.invert` | `false` | boolean; true / false | P / conversion / v1 | Reverse measured density ordering. Capability: none. |

## Conversion

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `conversion.matcher` | `"density"` | string; density, structural | P / conversion / v1 | Structure-aware mode has a larger compute budget. Capability: none. |
| `conversion.sample` | `"area"` | string; area, bilinear | P / conversion / v1 | Downsampling strategy. Capability: none. |
| `conversion.brightness` | `0` | number; -1 to 1 | P / conversion / v1 | Luminance offset before matching. Capability: none. |
| `conversion.contrast` | `1` | number; 0 to 4 | P / conversion / v1 | Contrast gain in normalized working space. Capability: none. |
| `conversion.gamma` | `1` | number; 0.1 to 5 | P / conversion / v1 | Artistic tone adjustment, not implicit profile conversion. Capability: none. |
| `conversion.saturation` | `1` | number; 0 to 3 | P / conversion / v1 | Color intensity; zero is grayscale. Capability: none. |
| `conversion.edge-strength` | `0.35` | number; 0 to 2 | P / conversion / v1 | Structural edge contribution. Capability: none. |
| `conversion.tone-weight` | `1` | number; 0 to 4 | P / conversion / v1 | Matcher tone weighting. Capability: none. |
| `conversion.shape-weight` | `1` | number; 0 to 4 | P / conversion / v1 | Matcher mask weighting. Capability: none. |
| `conversion.candidate-count` | `12` | integer; 1 to 128 | P / conversion / v1 | Shortlisted glyphs per structural cell. Capability: none. |
| `conversion.dither` | `"none"` | string; none, ordered, error-diffusion | P / conversion / v1 | Error diffusion has sequential dependencies. Capability: none. |
| `conversion.dither-strength` | `0.25` | number; 0 to 1 | P / conversion / v1 | Bounded tone perturbation. Capability: none. |
| `conversion.temporal-stability` | `0.3` | number; 0 to 1 | P / conversion / v1 | Penalty against gratuitous glyph changes; resets at cuts. Capability: none. |
| `conversion.cut-threshold` | `0.35` | number; 0 to 1 | P / conversion / v1 | Heuristic scene-cut reset threshold. Capability: none. |
| `conversion.alpha` | `"composite"` | string; composite, preserve | P / conversion / v1 | Preserve requires a presenter supporting correct alpha. Capability: none. |

## Color

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `color.mode` | `"monochrome"` | string; monochrome, source, ansi16, ansi256, duotone | P / conversion / v1 | Source mode retains sampled color. Capability: none. |
| `color.foreground` | `"#d7fbe8ff"` | string; RGBA #rrggbbaa | P / uniform / v1 | RGBA hexadecimal; final blending uses explicit color semantics. Capability: none. |
| `color.background` | `"#07110bff"` | string; RGBA #rrggbbaa | P / uniform / v1 | Safe opaque background by default. Capability: none. |
| `color.secondary` | `"#58aa80ff"` | string; RGBA #rrggbbaa | P / uniform / v1 | Duotone/effect secondary color. Capability: none. |
| `color.opacity` | `1` | number; 0 to 1 | P / uniform / v1 | Visual alpha; presenter support may restrict effectiveness. Capability: none. |
| `color.palette-dither` | `false` | boolean; true / false | P / conversion / v1 | Palette quantization dithering, separate from glyph dithering. Capability: none. |

## Playback

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `playback.fps` | `30` | integer; 1 to 120 | P / scheduler / v1 | Requested ceiling, not guaranteed refresh rate. Capability: none. |
| `playback.speed` | `1` | number; 0.1 to 4 | P / scheduler / v1 | Changes media timeline scaling, not decoder timestamp order. Capability: none. |
| `playback.loop` | `true` | boolean; true / false | P / scheduler / v1 | Loop only within valid source bounds. Capability: none. |
| `playback.loop-start-ms` | `0` | integer; 0 to 86400000 | P / decoder / v1 | Start bound; validate against actual duration. Capability: none. |
| `playback.loop-end-ms` | `0` | integer; 0 to 86400000 | P / decoder / v1 | Zero means natural end; nonzero must exceed start. Capability: none. |
| `playback.resume` | `"continue"` | string; continue, restart, wall-clock | P / scheduler / v1 | Wall-clock skips missed media rather than replaying it. Capability: none. |
| `playback.loop-transition-ms` | `0` | integer; 0 to 2000 | P / runtime / v1 | Optional blend; may require additional buffered frames. Capability: none. |

## Scene

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `scene.seed` | `1` | integer; 0 to 4294967295 | P / scene / v1 | Deterministic seed for procedural scenes. Capability: none. |
| `scene.time-scale` | `1` | number; 0 to 4 | P / scene / v1 | Zero freezes simulation without changing package data. Capability: none. |
| `scene.max-particles` | `2000` | integer; 0 to 20000 | P / scene / v1 | Requested cap further bounded by user power policy. Capability: none. |
| `scene.simulation-hz` | `60` | integer; 15 to 120 | P / scene / v1 | Fixed-step target with bounded catch-up. Capability: none. |
| `scene.background-motion` | `true` | boolean; true / false | P / scene / v1 | Static alternative can disable continuous scene evolution. Capability: none. |

## Effects

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `effects.glitch.enabled` | `false` | boolean; true / false | P / runtime / v1 | Off by default, especially for reduced-motion users. Capability: none. |
| `effects.glitch.strength` | `0.1` | number; 0 to 1 | P / uniform / v1 | Bounded visual distortion. Capability: none. |
| `effects.glitch.interval-ms` | `3000` | integer; 100 to 60000 | P / runtime / v1 | Deterministic scheduled glitches, not unbounded random callbacks. Capability: none. |
| `effects.scanlines.enabled` | `false` | boolean; true / false | P / uniform / v1 | Subtle optional raster postprocess. Capability: none. |
| `effects.scanlines.strength` | `0.1` | number; 0 to 0.5 | P / uniform / v1 | Darkening amplitude. Capability: none. |
| `effects.bloom.enabled` | `false` | boolean; true / false | P / runtime / v1 | Optional higher GPU cost, not available in every presenter. Capability: none. |
| `effects.bloom.strength` | `0.2` | number; 0 to 2 | P / uniform / v1 | Clamp output; HDR not implied. Capability: none. |
| `effects.trails.enabled` | `false` | boolean; true / false | P / runtime / v1 | Retains bounded history textures or cell state. Capability: none. |
| `effects.trails.decay` | `0.8` | number; 0 to 0.99 | P / uniform / v1 | Decay below one prevents immortal trails. Capability: none. |
| `effects.vignette` | `0` | number; 0 to 1 | P / uniform / v1 | Optional edge shading. Capability: none. |
| `effects.noise` | `0` | number; 0 to 0.5 | P / uniform / v1 | Seeded noise; disabled for deterministic screenshots unless requested. Capability: none. |

## Interaction

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `interaction.pointer.mode` | `"none"` | string; none, attract, repel, parallax, reveal | P / runtime / v1 | Requested reaction; does not enable observation permission. Capability: pointer-position. |
| `interaction.pointer.radius-cells` | `10` | number; 0.5 to 100 | P / uniform / v1 | Measured in scene cells rather than raw desktop pixels. Capability: pointer-position. |
| `interaction.pointer.strength` | `0.5` | number; 0 to 5 | P / uniform / v1 | Clamped force multiplier. Capability: pointer-position. |
| `interaction.pointer.smoothing-ms` | `80` | integer; 0 to 1000 | P / runtime / v1 | Latency/smoothness tradeoff. Capability: pointer-position. |
| `interaction.pointer.max-offset-cells` | `4` | number; 0 to 32 | P / uniform / v1 | Protects legibility and bounds displacement. Capability: pointer-position. |
| `interaction.click.mode` | `"none"` | string; none, ripple, burst, reveal, pulse | P / runtime / v1 | Requires passive button feed and scope consent. Capability: pointer-buttons. |
| `interaction.click.radius-cells` | `15` | number; 1 to 100 | P / uniform / v1 | Maximum effect radius. Capability: pointer-buttons. |
| `interaction.click.duration-ms` | `700` | integer; 50 to 5000 | P / runtime / v1 | Bounded lifetime per effect. Capability: pointer-buttons. |
| `interaction.click.strength` | `0.6` | number; 0 to 3 | P / uniform / v1 | Reduced-motion policy may override. Capability: pointer-buttons. |
| `interaction.click.max-active` | `8` | integer; 1 to 64 | P / runtime / v1 | Drops/coalesces events beyond budget. Capability: pointer-buttons. |
| `interaction.click.cooldown-ms` | `60` | integer; 0 to 2000 | P / runtime / v1 | Rate limit independent of OS double-click behavior. Capability: pointer-buttons. |
| `interaction.window.mode` | `"none"` | string; none, avoid, wake, edge-glow | P / runtime / v1 | Requires geometry capability, not just a window list. Capability: window-rectangles. |
| `interaction.window.margin-cells` | `2` | number; 0 to 32 | P / uniform / v1 | Inflates obstacles for reaction. Capability: window-rectangles. |
| `interaction.window.strength` | `0.5` | number; 0 to 5 | P / uniform / v1 | Force/glow strength. Capability: window-rectangles. |
| `interaction.window.decay-ms` | `250` | integer; 0 to 3000 | P / runtime / v1 | Stale obstacle fade, except immediate clear on lock. Capability: window-rectangles. |

## Transition

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `transition.in` | `"fade"` | string; none, fade, dissolve, scanline, cascade, radial, glitch | P / runtime / v1 | Safe default; user motion policy wins. Capability: none. |
| `transition.out` | `"fade"` | string; none, fade, dissolve, scanline, cascade, radial, glitch | P / runtime / v1 | Not guaranteed on forced termination. Capability: none. |
| `transition.duration-ms` | `350` | integer; 0 to 5000 | P / runtime / v1 | Zero makes the change immediate. Capability: none. |
| `transition.easing` | `"ease-in-out"` | string; linear, ease-in, ease-out, ease-in-out | P / runtime / v1 | Bounded monotonic easing. Capability: none. |
| `transition.direction` | `"left-to-right"` | string; left-to-right, right-to-left, top-to-bottom, bottom-to-top | P / runtime / v1 | Used only by directional transitions. Capability: none. |
| `transition.on-resume` | `false` | boolean; true / false | P / runtime / v1 | Avoid repeated cosmetic animation after brief pauses. Capability: none. |

## Privacy

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `privacy.pointer-observation` | `false` | boolean; true / false | A / observer / v1 | Consent gate; packages cannot enable it. Capability: pointer-position. |
| `privacy.button-observation` | `false` | boolean; true / false | A / observer / v1 | Consent gate independent from position. Capability: pointer-buttons. |
| `privacy.window-observation` | `false` | boolean; true / false | A / observer / v1 | Bounds/state only, no screenshots or titles. Capability: window-state. |
| `privacy.interaction-scope` | `"desktop-only"` | string; desktop-only, global-observed | A / observer / v1 | Broader scope requires explicit user choice. Capability: pointer-position. |
| `privacy.observe-right-click` | `false` | boolean; true / false | A / observer / v1 | Context menu must remain untouched. Capability: pointer-buttons. |
| `privacy.observe-middle-click` | `false` | boolean; true / false | A / observer / v1 | Optional button signal only. Capability: pointer-buttons. |
| `privacy.observe-wheel` | `false` | boolean; true / false | A / observer / v1 | Avoid animation driven by scrolling another app. Capability: pointer-buttons. |
| `privacy.network-enabled` | `false` | boolean; true / false | A / controller / v1 | Optional catalog/download module only. Capability: none. |
| `privacy.diagnostics-detail` | `"redacted"` | string; minimal, redacted, explicit-full | A / controller / v1 | Full diagnostics requires a fresh export confirmation. Capability: none. |
| `privacy.crash-upload` | `false` | boolean; true / false | A / controller / v1 | Reserved false in v1: no automatic crash-upload implementation. Capability: none. |

## Display

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `display.assignment-mode` | `"independent"` | string; independent, mirror, span, synchronized | O / surface / v1 | Span/mirror are explicit, never inferred from equal resolution. Capability: none. |
| `display.on-disconnect` | `"park"` | string; park, move-to-primary, stop | O / surface / v1 | Park preserves assignment without active surface. Capability: none. |
| `display.missing-source` | `"keep-last"` | string; keep-last, static-fallback, stop | O / runtime / v1 | No repeated unsafe reimports. Capability: none. |
| `display.output-scale` | `"auto"` | string; auto, manual | O / surface / v1 | Manual override is diagnostic, not a portable setting. Capability: none. |
| `display.manual-scale` | `1` | number; 0.5 to 4 | O / surface / v1 | Applied only in manual mode. Capability: none. |
| `display.safe-margin-px` | `0` | integer; 0 to 256 | O / surface / v1 | Diagnostic clipping; avoid guessing taskbar sizes. Capability: none. |

## Performance

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `performance.profile` | `"balanced"` | string; quiet, balanced, performance, battery, custom | A / runtime / v1 | Chooses requested limits; does not guarantee measured FPS. Capability: none. |
| `performance.auto-quality` | `true` | boolean; true / false | A / runtime / v1 | Can reduce density/FPS after sustained overload. Capability: none. |
| `performance.max-cells` | `65536` | integer; 1024 to 1048576 | A / runtime / v1 | Total per-frame hard maximum is additionally format bounded. Capability: none. |
| `performance.max-live-decoders` | `2` | integer; 1 to 8 | A / runtime / v1 | Global limit across outputs and transitions. Capability: none. |
| `performance.frame-queue-depth` | `3` | integer; 2 to 3 | A / runtime / v1 | Bounded decoded frame queue. Capability: none. |
| `performance.cpu-workers` | `0` | integer; 0 to 64 | A / restart / v1 | Zero derives a conservative system-dependent worker count. Capability: none. |
| `performance.cpu-fallback` | `"static"` | string; disabled, static, limited-animation | A / runtime / v1 | CPU presenter is a separately tested fallback, not promised feature parity. Capability: none. |
| `performance.gpu-preference` | `"power-saving"` | string; power-saving, high-performance, auto | A / surface / v1 | Device changes recreate renderer resources. Capability: none. |
| `performance.memory-budget-mib` | `512` | integer; 128 to 4096 | A / runtime / v1 | Global target; validated hard worker budgets remain independent. Capability: none. |
| `performance.max-catchup-steps` | `4` | integer; 1 to 8 | A / runtime / v1 | Drops simulation backlog rather than spiralling. Capability: none. |

## Power

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `power.on-battery` | `"reduce"` | string; continue, reduce, static, pause | A / runtime / v1 | No power sensor means unknown, not battery zero. Capability: battery-state. |
| `power.battery-fps` | `10` | integer; 1 to 30 | A / scheduler / v1 | Applies only under the battery policy. Capability: battery-state. |
| `power.low-battery-percent` | `20` | integer; 5 to 50 | A / runtime / v1 | Use hysteresis to avoid rapid toggles. Capability: battery-state. |
| `power.low-battery-hysteresis` | `5` | integer; 1 to 15 | A / runtime / v1 | Percentage points above threshold before clearing low state. Capability: battery-state. |
| `power.pause-on-lock` | `true` | boolean; true / false | A / runtime / v1 | True is required by v1 policy; false is rejected semantically. Capability: lock-state. |
| `power.pause-on-display-sleep` | `true` | boolean; true / false | A / runtime / v1 | Stop wasteful decode/render while display sleeps. Capability: none. |
| `power.resume-policy` | `"revalidate"` | string; revalidate, manual | A / runtime / v1 | Always revalidate surfaces and capabilities before resume. Capability: none. |

## Visibility

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `visibility.pause-fullscreen` | `true` | boolean; true / false | A / runtime / v1 | Best effort with capability explanation. Capability: window-state. |
| `visibility.pause-covered` | `true` | boolean; true / false | A / runtime / v1 | Uses estimated rectangle union, not exact pixel occlusion. Capability: window-rectangles. |
| `visibility.covered-threshold` | `0.98` | number; 0.5 to 1 | A / runtime / v1 | Must be greater than resume threshold. Capability: window-rectangles. |
| `visibility.resume-threshold` | `0.9` | number; 0 to 0.99 | A / runtime / v1 | Must be below pause threshold. Capability: window-rectangles. |
| `visibility.pause-delay-ms` | `1000` | integer; 0 to 10000 | A / runtime / v1 | Sustained coverage before pausing. Capability: window-rectangles. |
| `visibility.resume-delay-ms` | `300` | integer; 0 to 5000 | A / runtime / v1 | Hysteresis avoids flicker during window dragging. Capability: window-rectangles. |
| `visibility.unknown-policy` | `"continue-reduced"` | string; continue, continue-reduced, pause | A / runtime / v1 | Explicit behavior when confidence is unavailable. Capability: none. |
| `visibility.global-app-rule` | `false` | boolean; true / false | A / runtime / v1 | Applies user-selected rules to all outputs, not default behavior. Capability: window-state. |

## Accessibility

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `accessibility.reduced-motion` | `"system"` | string; system, on, off | A / runtime / v1 | System preference may be overridden explicitly by user. Capability: none. |
| `accessibility.disable-flashes` | `true` | boolean; true / false | A / runtime / v1 | Conservative effect policy, not a medical safety guarantee. Capability: none. |
| `accessibility.ui-scale` | `1` | number; 0.75 to 2.5 | A / ui / v1 | Scales controls independently from character density. Capability: none. |
| `accessibility.high-contrast` | `"system"` | string; system, on, off | A / ui / v1 | UI theme, not forced artwork recoloring. Capability: none. |
| `accessibility.preview-motion` | `false` | boolean; true / false | A / ui / v1 | Avoid animated grid of many library previews by default. Capability: none. |

## Ui

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `ui.theme` | `"system"` | string; system, dark, light | A / ui / v1 | Editor chrome only. Capability: none. |
| `ui.language` | `"system"` | string; <= 40 characters; semantic checks | A / ui / v1 | Validate supported locale; fallback is visible. Capability: none. |
| `ui.advanced-settings` | `false` | boolean; true / false | A / ui / v1 | Controls discoverability, not capability. Capability: none. |
| `ui.close-behavior` | `"background"` | string; background, quit | A / ui / v1 | Explain tray-less desktop recovery path. Capability: none. |
| `ui.preview-fps` | `20` | integer; 1 to 60 | A / scheduler / v1 | Separate preview budget from desktop renderer. Capability: none. |
| `ui.preview-auto-update` | `true` | boolean; true / false | A / ui / v1 | Expensive updates are debounced and cancellable. Capability: none. |
| `ui.preview-debounce-ms` | `120` | integer; 0 to 1000 | A / ui / v1 | Avoid rebuilding atlas on every intermediate slider tick. Capability: none. |
| `ui.confirm-multi-output-apply` | `true` | boolean; true / false | A / ui / v1 | Prevents accidental replacement on every screen. Capability: none. |

## Storage

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `storage.copy-imported-media` | `false` | boolean; true / false | A / controller / v1 | Reference local sources by default; packages are managed immutable copies. Capability: none. |
| `storage.cache-budget-mib` | `2048` | integer; 64 to 32768 | A / controller / v1 | LRU-derived data only, never source-media deletion. Capability: none. |
| `storage.cache-max-age-days` | `30` | integer; 1 to 365 | A / controller / v1 | Unused derived entries eligible for eviction. Capability: none. |
| `storage.keep-import-original` | `true` | boolean; true / false | A / controller / v1 | Retain archive provenance separately from expanded cache. Capability: none. |
| `storage.watch-local-sources` | `false` | boolean; true / false | A / controller / v1 | Only chosen files; no recursive home-directory watching. Capability: none. |
| `storage.watch-debounce-ms` | `300` | integer; 50 to 5000 | A / controller / v1 | Handle partial writes and duplicate file-system events. Capability: none. |
| `storage.log-retention-days` | `7` | integer; 1 to 30 | A / controller / v1 | Redacted bounded logs only. Capability: none. |
| `storage.log-budget-mib` | `20` | integer; 1 to 100 | A / controller / v1 | Rotate before unbounded growth. Capability: none. |

## Library

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `library.online-catalog` | `"disabled"` | string; disabled, manual | A / controller / v1 | No automatic background catalog fetch in v1. Capability: none. |
| `library.show-incompatible` | `true` | boolean; true / false | A / ui / v1 | Explain missing features instead of hiding content mysteriously. Capability: none. |
| `library.download-confirmation` | `true` | boolean; true / false | A / controller / v1 | Show source, size, license, and compatibility first. Capability: none. |
| `library.retain-previous-version` | `true` | boolean; true / false | A / controller / v1 | Allows rollback without rewriting immutable package content. Capability: none. |
| `library.apply-after-import` | `false` | boolean; true / false | A / controller / v1 | Import is not activation. Capability: none. |

## Startup

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `startup.autostart` | `false` | boolean; true / false | A / controller / v1 | Per-user opt-in. Capability: none. |
| `startup.restore-last` | `true` | boolean; true / false | A / controller / v1 | Disabled automatically after a crash-loop threshold. Capability: none. |
| `startup.delay-ms` | `1500` | integer; 0 to 15000 | A / controller / v1 | Wait for shell readiness with bounded retries. Capability: none. |
| `startup.open-settings` | `false` | boolean; true / false | A / controller / v1 | Tray-less recovery still available by relaunching CLI/app. Capability: none. |

## Recovery

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `recovery.max-retries` | `3` | integer; 0 to 5 | A / controller / v1 | Per failure window, then safe mode. Capability: none. |
| `recovery.window-seconds` | `300` | integer; 30 to 1800 | A / controller / v1 | Bounds automatic recovery attempts. Capability: none. |
| `recovery.static-fallback` | `true` | boolean; true / false | A / runtime / v1 | Uses consented fallback, not arbitrary OS setting rewrite. Capability: none. |

## Diagnostics

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `diagnostics.overlay` | `false` | boolean; true / false | S / runtime / v1 | Temporary visual diagnostics, never in package export. Capability: none. |
| `diagnostics.log-level` | `"warn"` | string; error, warn, info, debug | S / controller / v1 | Debug remains redacted unless explicit export consent. Capability: none. |
| `diagnostics.record-metrics` | `false` | boolean; true / false | S / controller / v1 | Local counters only; no media/input payloads. Capability: none. |

## Export

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `export.columns` | `240` | integer; 32 to 1024 | A / job / v1 | Independent export target, not live assignment mutation. Capability: none. |
| `export.width-px` | `1920` | integer; 64 to 8192 | A / job / v1 | Validated against target pixel/memory caps. Capability: none. |
| `export.height-px` | `1080` | integer; 64 to 8192 | A / job / v1 | Total area also checked; per-axis bounds alone insufficient. Capability: none. |
| `export.fps` | `30` | integer; 1 to 120 | A / job / v1 | Use rational timestamps; supported encoders vary by build. Capability: none. |
| `export.include-original` | `false` | boolean; true / false | A / job / v1 | Warn about file size and rights; only explicitly selected sources. Capability: none. |
| `export.overwrite` | `false` | boolean; true / false | A / job / v1 | Atomic temporary output, never overwrite without consent. Capability: none. |
| `export.deterministic` | `true` | boolean; true / false | A / job / v1 | Fixed seed/time/ordering; GPU cross-device pixels may differ. Capability: none. |

## Audio

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `audio.playback-enabled` | `false` | boolean; true / false | A / runtime / later | Later capability; wallpapers are muted by default. Capability: none. |
| `audio.volume` | `0` | number; 0 to 1 | A / runtime / later | One audio owner per source; avoid duplicated sound on multiple outputs. Capability: none. |
| `audio.analysis-source` | `"none"` | string; none, selected-file, system-loopback, microphone | A / runtime / later | Different sources need different permissions and platform support. Capability: none. |
| `audio.smoothing-ms` | `120` | integer; 0 to 2000 | A / runtime / later | Smooth and clamp missing/noisy analysis samples. Capability: none. |

## Signals

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `signals.cpu.enabled` | `false` | boolean; true / false | A / runtime / later | Optional local coarse utilization, not process-content inspection. Capability: none. |
| `signals.cpu.interval-ms` | `1000` | integer; 250 to 10000 | A / runtime / later | Bound observer overhead. Capability: none. |

## Playlist

| Key | Default | Type / constraints | Scope / change / phase | Meaning / capability |
|---|---|---|---|---|
| `playlist.shuffle` | `false` | boolean; true / false | A / runtime / later | Seeded shuffle avoids immediate repeats where possible. Capability: none. |
| `playlist.entry-seconds` | `300` | integer; 10 to 86400 | A / runtime / later | Scene schedule; invalid entries skipped with bounded retries. Capability: none. |

## Cross-field constraints

The product of grid rows and columns must fit all format, engine, user, and device budgets. Explicit cell width must remain positive after letter spacing. Loop end must exceed loop start and fit the selected source. At least one matcher weight must be positive. The coverage resume threshold must be lower than the pause threshold. Font IDs must resolve to an approved built-in or declared package font. Capability denial always wins over visual defaults.

`privacy.crash-upload` is reserved and must remain false in v1; `power.pause-on-lock` must remain true. Do not advertise these as functioning opt-outs. Later audio/system-signal settings are planning entries, not portable v1 controls. Presets may expose a smaller friendly subset, but the effective-settings inspector shows every applied value and why it changed.

See [precedence](settings-precedence-and-presets.md), [validation](validation-and-migrations.md), and [manifest schema](../package/manifest-schema.md).

## Font and representation resolution

`font.face=package-font` selects the manifest's declared font asset; it is invalid without that asset. Other face IDs resolve only through the engine's approved built-in registry. A package font is never resolved through an arbitrary system path. Export preserves the selected font's rights/notice rules or explicitly substitutes a built-in font after user approval.

Conversion controls are active only for raster-to-character stages. They are not falsely exposed as editable glyph controls for baked video or direct procedural generators. Shared styling, scene timing, and supported presentation overlays can remain available across representations. The UI shows why a control does not apply to the current source.
