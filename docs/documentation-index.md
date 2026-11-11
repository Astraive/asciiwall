# Documentation index

**100 Markdown documents**, organized for implementation rather than one enormous specification. All delivered filenames/folders are lowercase kebab-case. Begin with [start here](start-here.md); the application and library repositories are proposed, not created by this pack.

## Fast paths

For feasibility, read research and platform. For coding, read architecture/tech-stack, then engine and the four plans. For sharing, read package and library. For product behavior, read settings/interaction/UI. For release readiness, read quality/security/delivery.

## Root guides

- [Documentation index](documentation-index.md)
- [AsciiWall - research and engineering documentation](readme.md)
- [Start here](start-here.md)

## Architecture (10)

Shared contracts, process ownership, IPC, cache, capabilities, lifecycle, and repository boundaries.

- [Capabilities and graceful fallbacks](architecture/capabilities-and-fallbacks.md)
- [Crate boundaries and dependency contracts](architecture/crate-boundaries.md)
- [Data model and engine contracts](architecture/data-model-and-contracts.md)
- [Events and coordinate spaces](architecture/event-model-and-coordinate-spaces.md)
- [Processes, threads, and local IPC](architecture/process-model-and-ipc.md)
- [Application repository structure](architecture/repository-structure.md)
- [Resource ownership, concurrency, and shutdown](architecture/resource-ownership-and-shutdown.md)
- [Runtime state machine and lifecycle](architecture/runtime-state-machine.md)
- [Local storage, cache keys, and persistence](architecture/storage-and-cache.md)
- [System architecture](architecture/system-overview.md)

## Cli (3)

Proposed commands, scripting, local IPC, output formats, and stable errors.

- [CLI automation and local IPC protocol](cli/automation-and-ipc.md)
- [Command-line interface reference](cli/command-reference.md)
- [Exit codes and structured errors](cli/exit-codes-and-errors.md)

## Decisions (4)

Four architecture decisions with alternatives, rationale, and consequences.

- [ADR 0001 - Own Rust engine and shared frame model](decisions/0001-native-rust-engine.md)
- [ADR 0002 - Capability-aware native wallpaper hosts](decisions/0002-capability-aware-native-hosts.md)
- [ADR 0003 - Declarative .asciiwall packages](decisions/0003-declarative-portable-packages.md)
- [ADR 0004 - Offline core and static community library](decisions/0004-offline-core-and-static-library.md)

## Delivery (6)

Dependency-aware milestones, 40 work units, packaging, CI, contribution, and risks.

- [Continuous integration and release engineering](delivery/ci-and-release.md)
- [Contributor workflow and engineering conventions](delivery/contributor-workflow.md)
- [Implementation backlog and ownership](delivery/implementation-backlog.md)
- [Milestones and dependency-aware delivery](delivery/milestones-and-dependencies.md)
- [Packaging, signing, installation, and removal](delivery/packaging-and-signing.md)
- [Risk register and proof-of-concept gates](delivery/risks-and-spikes.md)

## Engine (11)

Original conversion algorithms, fonts/colors, media timing, rendering, scenes, clips, exports, quality, and budgets.

- [Precomputed ASCII clip format - draft version 1](engine/ascii-clip-format.md)
- [Color, alpha, image normalization, and quality](engine/color-and-image-processing.md)
- [Original Rust ASCII conversion pipeline](engine/conversion-pipeline.md)
- [Effects and declarative scene graph](engine/effects-and-scene-graph.md)
- [Glyph matching, fonts, and character sets](engine/glyph-matching-and-fonts.md)
- [GPU glyph rendering and compositing](engine/gpu-rendering.md)
- [Performance budgets and power policy](engine/performance-and-power.md)
- [Pre-rendering, baking, and export behavior](engine/pre-rendering-and-export.md)
- [Procedural scene system](engine/procedural-scenes.md)
- [ASCII quality evaluation and algorithm tradeoffs](engine/quality-evaluation.md)
- [Video decoding, timing, and animated image behavior](engine/video-timing-and-decoding.md)

## Interaction (5)

Cursor/click/window reactions, motion transitions, refresh semantics, and local rules.

- [Local signals and rule-based automation](interaction/automation-and-local-signals.md)
- [Pointer and click reactions](interaction/pointer-and-click-reactions.md)
- [Refresh semantics and live customization](interaction/refresh-and-live-editing.md)
- [Entrance, exit, and change animations](interaction/transitions-and-motion.md)
- [Reactions to windows above the wallpaper](interaction/window-aware-behavior.md)

## Library (6)

Second-repository structure, signed static catalog, review, licensing, releases, and 36 recipe briefs.

- [Static catalog schema and trust model](library/catalog-and-index-schema.md)
- [Wallpaper contribution and review policy](library/contribution-and-review.md)
- [Code, media, fonts, and attribution](library/licensing-and-attribution.md)
- [Library build and release pipeline](library/release-pipeline-and-distribution.md)
- [The asciiwall-library repository](library/repository-structure.md)
- [Starter wallpaper catalog and recipe briefs](library/starter-wallpaper-catalog.md)

## Package (7)

Draft .asciiwall transport, full schemas, import/export, compatibility, and validated examples.

- [Package compatibility and migrations](package/compatibility-and-migrations.md)
- [Package examples and conformance vectors](package/examples-and-test-vectors.md)
- [Export, portability, and reproducible packages](package/export-and-reproducibility.md)
- [The .asciiwall package format](package/format-specification.md)
- [Secure import and validation pipeline](package/import-and-validation.md)
- [Manifest JSON Schema and semantic contract](package/manifest-schema.md)
- [Declarative scene schema and execution rules](package/scene-schema.md)

## Plans (4)

Four task-by-task implementation plans with owned paths, target interfaces, and acceptance tests.

- [Desktop Implementation Plan](plans/desktop-implementation-plan.md)
- [Engine Implementation Plan](plans/engine-implementation-plan.md)
- [Hardening Implementation Plan](plans/hardening-implementation-plan.md)
- [Packages Implementation Plan](plans/packages-implementation-plan.md)

## Platform (10)

Windows, macOS, X11, Wayland, Plasma, GNOME, outputs, permissions, and sessions.

- [GNOME integration and shell-safety boundary](platform/linux-gnome.md)
- [KDE Plasma integration and presentation bridge](platform/linux-kde-plasma.md)
- [Wayland hosting and capability limits](platform/linux-wayland.md)
- [Linux X11 adapter](platform/linux-x11.md)
- [macOS desktop host and permissions](platform/macos.md)
- [Multi-monitor geometry, identity, and scaling](platform/multi-monitor-and-scaling.md)
- [Permissions and privacy by capability](platform/permissions-and-privacy.md)
- [Power, lock, session, and shutdown policy](platform/power-lock-and-session.md)
- [Platform support policy](platform/support-policy.md)
- [Windows wallpaper and interaction adapter](platform/windows.md)

## Product (4)

Vision, representation modes, feature catalog, and requirements traceability.

- [Feature catalog and release scope](product/feature-catalog.md)
- [Wallpaper modes and editing capabilities](product/mode-comparison.md)
- [Requirements traceability matrix](product/requirements-traceability.md)
- [Vision, scope, and product boundaries](product/vision-and-scope.md)

## Quality (6)

Test strategy, 120 failure scenarios, benchmarks, release gates, diagnostics, and this bundle QA.

- [Release acceptance criteria](quality/acceptance-criteria.md)
- [Benchmark plan and performance methodology](quality/benchmark-plan.md)
- [Diagnostics and troubleshooting playbook](quality/diagnostics-and-troubleshooting.md)
- [Documentation verification report](quality/documentation-verification.md)
- [Edge-case and failure-mode matrix](quality/edge-case-matrix.md)
- [Testing strategy and evidence levels](quality/test-strategy.md)

## Reference (1)

Shared vocabulary and naming conventions.

- [Glossary and naming conventions](reference/glossary.md)

## Research (5)

Findings, feasibility, prior art, assumptions, and 62 primary-source references.

- [Assumptions, uncertainty, and remaining research](research/assumptions-and-open-questions.md)
- [Feasibility and capability matrix](research/feasibility-matrix.md)
- [Prior art and differentiation](research/prior-art-and-differentiation.md)
- [Research findings and revised engineering direction](research/research-summary.md)
- [Research source register](research/source-register.md)

## Security (5)

Threat boundaries, restricted media, offline privacy, supply chain, and data-only extension policy.

- [Plugin, script, and shader policy](security/plugin-and-shader-policy.md)
- [Offline operation and network policy](security/privacy-and-network-policy.md)
- [Supply chain, signatures, and updates](security/supply-chain-and-updates.md)
- [Security threat model](security/threat-model.md)
- [Untrusted parser and worker confinement](security/untrusted-media-and-packages.md)

## Settings (3)

172 proposed controls, 80 portable v1 visual defaults, precedence, validation, and migrations.

- [Settings precedence, presets, and customization](settings/settings-precedence-and-presets.md)
- [Complete proposed settings registry](settings/settings-reference.md)
- [Configuration validation and migrations](settings/validation-and-migrations.md)

## Tech Stack (3)

Selected technologies, dependency compatibility policy, and build environment.

- [Build environment and toolchain](tech-stack/build-environment-and-toolchain.md)
- [Dependency selection and version policy](tech-stack/dependency-selection-and-version-policy.md)
- [Technology stack and alternatives](tech-stack/stack-decisions.md)

## Ui (4)

Navigation, preview/editor transactions, library flows, accessibility, and localization.

- [Accessibility, motion, and localization](ui/accessibility-and-localization.md)
- [Editor, preview, and customization UX](ui/editor-and-preview.md)
- [Desktop application information architecture](ui/information-architecture.md)
- [Library, import, and export user flows](ui/library-and-import-flows.md)

## Representation and status reminders

A character clip is not a baked raster video. Optional desktop reactions depend on actual host capabilities. Schemas and CLI commands are proposed contracts. Performance values are budgets/arithmetic unless explicitly measured in a future release. The library catalog here contains design examples and recipe briefs, not downloadable published artwork.

See [documentation verification](quality/documentation-verification.md) for what was actually checked in this delivery and what remains implementation work.
