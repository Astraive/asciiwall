# Feature catalog and release scope

## Product modes

Procedural built-in scenes; static image-to-ASCII with optional animation; live video-to-ASCII; precomputed character clips; and baked ASCII video playback. All run locally, with a shared editor and explicit representation-aware controls. Static text/ANSI/PNG and clip/video exports use the same engine contracts.

## Rich customization target

The [settings registry](../settings/settings-reference.md) defines 172 proposed controls, including 80 portable v1 visual defaults. It covers fonts/charsets, grid/aspect/crop, luminance/color/dithering/structural matching, playback, scenes/effects, pointer/click/window reactions, transitions, power/visibility, privacy, storage, UI/accessibility, startup, recovery, diagnostics, and export. Later-phase audio/playlist/system-signal entries are labelled rather than implied implemented.

A large registry does not mean every control should appear at first launch. Basic presets, searchable advanced controls, and the requested/effective inspector are core usability requirements.

## Interaction target

Passive cursor attraction/repulsion, reveal, parallax, click ripples/bursts/pulses, optional window avoidance/wake/glow, estimated-coverage pause, fullscreen policy, and local time rules. Desktop input remains untouched. Features depend on actual native observer capabilities; a preview simulation is not proof of desktop support.

## Lifecycle target

Transactional Apply, bounded entrance/exit/change transitions, named refresh operations, source watching, live editing, undo/redo, per-output assignments, mirror/span/synchronization, hotplug, suspend/resume, lock handling, shell/GPU/worker recovery, and a safe-mode startup path.

## Library and portability

Separate `asciiwall-library` source repository, local installed gallery, favorites/tags/compatibility/license filters, original recipe briefs, `.asciiwall` pack/import/export/inspect/validate, immutable versions, user variants, and optional signed static-catalog downloads. No account or owned backend.

## Explicit later research

Audio playback/analysis and system capture; richer procedural math/physics generators; custom executable plugins; arbitrary shader authoring; exact per-pixel window masks; universal Wayland observation; lock-screen integration; automatic binary self-update. These are not silently promised by the word 'customization'.

## Quality requirements

Offline core, no telemetry, explicit observation consent, safe untrusted data boundaries, bounded memory/CPU/GPU work, low idle work, accessibility, honest support status, reproducible package builds, attribution, and a tested removal/recovery path. These matter as much as visual features.

## Delivery meaning

This catalog describes the intended product and phased roadmap. None of the feature labels implies the app or community collection has already been implemented. Use the [traceability matrix](requirements-traceability.md) to connect requirements to specs, tasks, and failure tests.
