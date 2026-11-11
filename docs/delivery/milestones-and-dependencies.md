# Milestones and dependency-aware delivery

## Recommended sequence

| Milestone | Deliverable | Evidence to advance |
|---|---|---|
| M0: risk spikes | Native Windows surface, graphics dependency family, codec build, Linux bridge experiments | Written pass/fail and fallback decisions; no production support claims |
| M1: conversion tool | Local image -> deterministic ASCII text/PNG through original Rust converter | Core/property/golden tests; CLI runs with network denied |
| M2: visual engine | GPU glyph rendering, scenes, bounded effects, preview | Stable frame contract; static idle; measured baseline |
| M3: first real wallpaper | One verified Windows output, pause/stop/recovery | Behind icons, no input stealing, Explorer restart/lock/hotplug tests |
| M4: editable product | Native editor, settings/presets, transitions, capability-aware pointer/windows | Requested/effective inspector; permission-denial and transaction tests |
| M5: media and portability | Live video, character clips, baked export, safe .asciiwall import/export | VFR/seek/loop tests, restricted workers, deterministic pack validation |
| M6: community library | Local gallery plus optional signed static catalog and second-repo workflow | No backend requirement; rights review; trusted release separation |
| M7: platform expansion | macOS, named X11/layer-shell hosts; measured Plasma/GNOME pilots | Per-adapter release evidence rather than universal claims |
| M8: hardened release | Installer/signing, accessibility, fuzzing, performance controls, docs | Release gates and all in-scope P0 cases pass |

This is dependency order, not a calendar estimate. Some M0/M1 work can run independently. Do not wait until the UI is finished to test desktop hosting; do not enable broad community imports before worker isolation and archive safety are proven.

## Minimum useful first release

A useful first alpha can offer static image conversion, several original procedural scenes, one tested Windows host, a small editor, pause/stop, and safe local package export. It must clearly label missing video, other hosts, and advanced observers rather than promise all planned features.

A feature-rich v1 adds the specified modes and library once their gates pass. 'Later' settings and recipe ideas remain disabled/absent until implemented. Shipping the complete 172-control surface on day one is not the recommended development strategy.

## Critical path

Core frame/limits -> font metrics and conversion -> GPU preview -> tested host -> transactional settings/assignment -> safe package format and isolated media -> media scheduling/export -> curated library -> broad distribution.

Cross-cutting security and native spikes start early. They are not cleanup tasks after feature completion. The format should remain draft until packer/importer/catalog and all source-kind examples interoperate.

## Parallel work boundaries

Conversion and host spikes can proceed with a small agreed frame contract. Package/schema work and UI mock workflows can proceed while video integration is isolated behind a fake decoder. Native adapters can be developed separately once capability/event/surface contracts stabilize.

Do not parallelize two teams rewriting the same frame/settings/IPC schema without ownership and version review. Cross-repo library work consumes a pinned engine release, not a moving internal API.

## Stop/reconsider gates

If Windows placement cannot remain below icons safely, ship preview/export while fixing the adapter. If a shell bridge stalls the compositor, reduce scope or keep it experimental. If worker restrictions fail, do not call untrusted import secure. If structural conversion misses budgets, reduce density/candidate count or postpone compute acceleration based on profiling.

See the [implementation backlog](implementation-backlog.md) and [risks and spikes](risks-and-spikes.md) for task-level ownership and exit evidence.
