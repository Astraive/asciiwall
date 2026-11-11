# AsciiWall - research and engineering documentation

**Research date:** 2026-11-11. **Document edition:** 1.0. **Product status:** proposed; not implemented or benchmarked by this documentation delivery.

AsciiWall is an open-source, local-first desktop application with an original Rust ASCII conversion and animation engine. It supports procedural scenes, image-to-ASCII, live video-to-ASCII, precomputed character clips, and pre-rendered ASCII video. It integrates with native desktops through capability-aware adapters, rather than assuming every desktop permits the same hooks.

This pack specifies two future repositories: `asciiwall` for the application/engine and `asciiwall-library` for curated wallpaper source recipes, metadata, previews, and release packaging. It does not create either remote repository, publish releases, ship wallpaper artwork, or include the app itself.

Start with [start-here.md](start-here.md), then use the [documentation index](documentation-index.md). All documentation filenames and folders use lowercase kebab-case. JSON, Rust, TOML, and shell blocks are proposed contracts or examples unless explicitly described as observed source metadata. Commands referring to `asciiwall` do not work until the application is implemented.

## What is settled in this proposal

- Rust owns conversion, scene generation, effects, character grids, scheduling, package validation, and rendering orchestration.
- `wgpu` is the default glyph presenter; `egui` supplies the control UI; native hosts own wallpaper surfaces, with separately tested shell-native presenters where required.
- Media codecs are dependencies, not algorithms we reinvent. The image-to-character mapping remains ours.
- Normal use requires no account, hosted service, database server, or internet connection. Optional catalog downloads are explicit and separable from playback.
- Packages are declarative data, not installers, executable plugins, JavaScript, shell commands, or arbitrary shaders.
- Platform capabilities are reported individually; unavailable features are disabled with an explanation and a fallback.

## Important boundaries

This is a detailed design and research pack, not proof that a Windows shell embedding strategy, macOS Spaces behavior, or Linux compositor adapter works on a particular machine. Native behavior, energy use, dependency interoperability, and packaging must pass the experiments and acceptance gates defined here. Performance numbers are proposed budgets or arithmetic examples, never measured results.

The terms MUST/SHOULD/MAY express requirements of this proposed project format and architecture, not an external published standard. Schema version `1.0` is a draft project contract and should not be advertised as stable before the format freeze gate.

Public sources are linked near the relevant claims and collected in `docs/research/source-register.md`. Source facts, design recommendations, implementation hypotheses, and unresolved issues are kept separate. No third-party fonts or downloaded art are included.

## Pack inventory

100 Markdown documents; 62 primary-source reference entries; 172 proposed settings (80 portable v1 visual defaults); three embedded JSON Schemas (manifest, scene, catalog); 40 implementation work units across four plans; 120 planned failure/edge scenarios; and 36 starter wallpaper recipe briefs.

This is a researched engineering blueprint, not an executable application, populated art collection, or native-platform certification. The [verification report](quality/documentation-verification.md) distinguishes actual documentation checks from planned product tests.
