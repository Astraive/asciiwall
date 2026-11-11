# Vision, scope, and product boundaries

## Product promise

Make text a living desktop medium. A person should be able to choose a procedural artwork, convert a favorite image or local clip, tune its character/color style, make it respond to approved local events, and share the result without an account or hosted backend.

The audience includes people who simply want a quiet wallpaper, enthusiasts tuning detailed effects, artists building reusable packages, and developers contributing Rust scenes or desktop adapters. The default experience must serve the first group; advanced power must not turn every install into a permissions or configuration exercise.

## Five source kinds, two execution strategies

`scene` generates character frames or intermediate fields procedurally. `image` converts raster content on demand and caches the unchanged result. `video` decodes and converts media during playback. `ascii-clip` plays precomputed character frames. `rendered-video` plays raster video that already contains ASCII-looking artwork.

Live generation/conversion preserves the most flexibility. Preprocessing reduces repeated conversion work but can consume storage or bake away editing freedom. Every source kind must advertise supported editor controls, supported exports, and whether it needs decoding, conversion, or only presentation.

## Scope commitments

The target product includes multiple displays, static fallbacks, cursor/click effects, window-geometry reactions where permitted, configurable transitions, refresh/reload/reattach operations, a local library, presets, a creator workflow, a CLI, safe package import/export, diagnostics, and offline operation. The second repository contains source artwork recipes and curated distribution metadata.

Optional later features include audio-reactive signals, advanced timeline editing, additional compositor bridges, richer text shaping, and tightly sandboxed extensions. These are tracked as future work, not implied by a version-one package.

## Non-goals

Do not implement codecs from scratch, replace the OS shell, draw on the login/lock screen, inject code into other applications, capture passwords, run as administrator/root, require a web service, or guarantee behavior across every window manager. Do not silently record screenshots to infer window geometry. Avoid a marketplace, social accounts, analytics ingestion, remote media URLs in wallpaper packages, or a general web-page runtime.

Open-source does not mean an unlimited free hosting promise. Release signing and third-party distribution services can have external operational costs even when the app has no backend. Treat those as maintainer decisions, not runtime dependencies.

## Success criteria

A static artwork stops unnecessary rendering; an animated scene meets documented frame-time and energy budgets on named hardware; input effects never steal normal desktop interactions; unsupported capabilities degrade visibly; malformed packages cannot write outside staging; offline import and playback work with network access denied; uninstall leaves no background processes or unexplained wallpaper changes.

The project is successful when its small initial scope is reliable and extensible, not when every toggle appears in a screenshot.
