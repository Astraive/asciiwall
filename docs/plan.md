# AsciiWall implementation plan

> **Planning baseline:** All backlog records, milestones, work units, settings, edge cases, and starter wallpaper briefs described here are **proposed planning data only**. Neither `Astraive/asciiwall` nor `Astraive/asciiwall-library` contains an implemented Rust Cargo workspace, application binary, native wallpaper host, or packaged artwork yet, and **0** GitHub issues or Project items have been published (`local-only` status).

This guide synthesizes the implementation strategy from the 100 original specification documents, the external local backlog workspace (`../.issues/`), and the four detailed execution plans ([engine plan](plans/engine-implementation-plan.md), [desktop plan](plans/desktop-implementation-plan.md), [packages plan](plans/packages-implementation-plan.md), and [hardening plan](plans/hardening-implementation-plan.md)).

## Planning inventory and current counts

Observed counts across the verified external backlog (`../.issues/reports/backlog-audit.json`) and original specification pack ([documentation verification](quality/documentation-verification.md)):

| Artifact | Verified count | Current state and boundary |
|---|---|---|
| Local backlog tickets | **356** (`278` `asciiwall`, `78` `asciiwall-library`) | `300` tasks, `38` epics, `16` gates, `2` roadmaps (`ROADMAP-001`, `LIB-ROADMAP-001`); all `Backlog` |
| Remote GitHub objects | **0** | No GitHub issues, project items, or native relationship edges have been created |
| Original source documents | **100** | Frozen design pack across 17 topic folders in `docs/` |
| Requirement mappings | **2,915** | Mapped across 8 coverage slices in `../.issues/coverage/*.json` (`0` validation errors) |
| Milestone gates | **16** | `QA-M0` through `QA-M10` (11 app) and `LQA-L0` through `LQA-L4` (5 library) |
| Hard dependency edges | **576** (`38` cross-repo) | Acyclic, transitively reduced (`../.issues/reports/backlog-dependency-graph.md`) |
| Effort model | **1,352** atomic + **163** container person-days | Container coordination effort is tracked separately and excluded from execution paths |
| Weighted longest path | **85 person-days** (`80` to 1.0) | Non-calendar lower bound assuming unlimited parallelism and zero contention |
| Implementation work units | **40** (`E01`–`E12`, `D01`–`D12`, `P01`–`P08`, `H01`–`H08`) | Detailed in [implementation backlog](delivery/implementation-backlog.md) |
| Proposed settings / ECs / recipes | **172** settings / **120** ECs / **36** briefs | Planning targets only (`80` portable v1 defaults; `73` P0 / `47` P1 ECs; `36` starter briefs) |

Detailed ticket bodies, derived inventory tables, creation ledgers, and coverage JSON registers live exclusively in `../.issues/` outside both Git repositories and are never committed or pushed. See [backlog validation](backlog-validation.md) and [backlog publication](backlog-publication.md) for regeneration and offline dry-run procedures.

## Implementation strategy and execution order

As recommended in [start here](start-here.md) and [milestones and dependencies](delivery/milestones-and-dependencies.md), implementation intentionally separates early high-risk technical proof points from large UI and ecosystem investments:

1. **Foundation contracts and early spikes (`M0`, `L0`):** Establish the exercised Cargo workspace, core `AsciiFrame` / `RasterFrameView` / `Budget` / `CancellationToken` types (`PRJ-001`, `COR-001`), typed settings registry (`CFG-001`), and run non-production feasibility probes for Windows desktop hosting (`WND-001`), platform worker confinement (`SEC-002`, `SEC-003`, `SEC-004`), and graphics/FFmpeg dependency coherence (`PRJ-002`, `REL-010`). In parallel without waiting for app shipping, `asciiwall-library` establishes MIT/asset licensing rules (`LLEG-001`), repository layout (`LIB-001`), and contributor intake (`LCOM-001`).
2. **Headless ASCII conversion engine and CLI (`M1`):** Build deterministic font rasterization and glyph descriptors (`FNT-001`, `FNT-002`), bounded PNG/JPEG decoding (`IMG-001`), color/alpha normalization (`CNV-002`), density and aspect-aware conversion (`CNV-001`, `CNV-003`), plain-text/restricted-ANSI export (`VID-010`), and standalone CLI workflows (`CLI-001`).
3. **Native `wgpu` presenter and v1 procedural scenes (`M2`, `L1`):** Deliver the atlas-based `wgpu` character-frame presenter (`RND-001`–`RND-005`), deterministic v1 generator contracts (`SCN-001`–`SCN-007`), draft v1 manifest/scene schemas (`PKG-001`–`PKG-003`), and stage instrumentation (`PERF-001`). Once `PKG-001` and `CLI-001` stabilize, `asciiwall-library` builds secretless package/catalog tooling (`L1`).
4. **Windows-first vertical slice (`M3`):** Deliver capability contracts (`WPH-001`), authenticated same-user IPC (`SEC-001`), transactional assignment/pause/topology primitives (`COR-003`–`COR-007`, `MON-001`, `PWR-001`), and the verified Windows desktop adapter (`WND-002`) behind desktop icons without focus stealing.
5. **Media pipelines, safe packaging, and reactive effects (`M4`):** Integrate bounded FFmpeg video decoding (`VID-001`–`VID-006`), `.ascii-clip` reading/writing (`VID-007`–`VID-009`), restricted media-worker isolation (`SEC-005`), race-safe staged ZIP archive import (`PKG-005`–`PKG-008`), structural conversion (`CNV-004`–`CNV-011`), and draft v1 character/post-process effects and transitions (`FX-001`, `FX-002`, `FX-006`–`FX-019`).
6. **Full desktop editor, settings ownership, and local library (`M5`):** Bind all v1 settings sections (`CFG-002`–`CFG-015`), build the `egui` editor with requested-vs-effective inspection and undo (`UIX-001`–`UIX-006`), complete multi-output hotplug and power/session recovery (`MON-002`, `MON-003`, `PWR-002`–`PWR-004`), enable passive pointer/click/window reactions (`INT-001`–`INT-005`, `WIN-001`, `WIN-002`), and index immutable local imports (`LIBAPP-001`, `LIBAPP-002`).
7. **Package format freeze and optional static catalog (`M6`, `L2`):** Freeze draft v1 `.asciiwall` conformance (`PKG-012`), deliver deterministic video/clip bake exports (`VID-012`–`VID-014`), optional explicit-consent catalog verification (`LIBAPP-004`–`LIBAPP-006`), and curate the 20 v1 declarative recipes (`AST-001`–`AST-020`) in `asciiwall-library` `L2`.
8. **Cross-platform experimental adapters (`M7`):** Independently evaluate and gate macOS (`MAC-001`, `MAC-002`), Linux X11 (`LNX-001`, `LNX-002`), Wayland `layer-shell` (`LNX-003`), KDE Plasma (`LNX-005`, `LNX-006`), and GNOME Shell (`LNX-007`, `LNX-008`) by named environment tuples (`WPH-005`).
9. **Beta hardening, 1.0 release, and post-launch (`M8`–`M10`, `L3`–`L4`):** Complete fuzzing (`QA-002`), accessibility evidence (`ACC-001`–`ACC-004`), aggregate budgets (`PERF-004`–`PERF-006`), installers/signing (`REL-003`–`REL-008`), compatible library 1.0 (`L3`), and scoped post-1.0 maintenance (`M10`, `L4`).

## Workstream and work-unit mapping

The 40 implementation work units from [implementation backlog](delivery/implementation-backlog.md) map onto the 38 domain epics across both repositories (`ROADMAP-001` and `LIB-ROADMAP-001`):

| Track | Work units | Primary domain epics | Detailed specification |
|---|---|---|---|
| Engine (`E01`–`E12`) | `12` units | `COR-EPIC`, `FNT-EPIC`, `CNV-EPIC`, `IMG-EPIC`, `RND-EPIC`, `SCN-EPIC`, `FX-EPIC`, `VID-EPIC`, `PERF-EPIC` | [Engine plan](plans/engine-implementation-plan.md) |
| Desktop (`D01`–`D12`) | `12` units | `WND-EPIC`, `MAC-EPIC`, `LNX-EPIC`, `WPH-EPIC`, `INT-EPIC`, `WIN-EPIC`, `MON-EPIC`, `PWR-EPIC`, `UIX-EPIC` | [Desktop plan](plans/desktop-implementation-plan.md) |
| Packages & library (`P01`–`P08`) | `8` units | `CFG-EPIC`, `PKG-EPIC`, `LIBAPP-EPIC`, `CLI-EPIC`, plus `LIB-EPIC`, `CAT-EPIC`, `AST-EPIC`, `LCOM-EPIC`, `LLEG-EPIC`, `LREL-EPIC` | [Packages plan](plans/packages-implementation-plan.md) |
| Hardening & governance (`H01`–`H08`) | `8` units | `SEC-EPIC`, `ACC-EPIC`, `QA-EPIC`, `REL-EPIC`, `ARC-EPIC`, `DOC-EPIC`, `PRJ-EPIC`, `COM-EPIC`, `LEG-EPIC`, `LQA-EPIC` | [Hardening plan](plans/hardening-implementation-plan.md) |

## Planned scope registries (not implemented features)

- **172 proposed settings (`CFG-001`–`CFG-016`):** Defined in [settings reference](settings/settings-reference.md) across 26 sections (`80` portable `P`, `83` application policy `A`, `6` output `O`, `3` session `S`; `164` v1 + `8` `later` keys for audio/signals/playlists). Every key has an assigned backlog owner (`CFG-004`–`CFG-016`) as planned configuration architecture, not active runtime controls.
- **120 edge-case scenarios (`EC-001`–`EC-120`):** Defined in [edge-case matrix](quality/edge-case-matrix.md) (`73` P0, `47` P1) across 12 failure domains and verified through `QA-101`–`QA-124`. None have been executed yet.
- **36 starter wallpaper recipe briefs (`AST-001`–`AST-038`):** Defined in [starter wallpaper catalog](library/starter-wallpaper-catalog.md) (`20` v1 declarative scenes in `L2`, `8` rights-cleared media recipes in `L3`, `8` later-engine recipes in `L4`). These are design briefs, not generated or distributed artwork files.

## Pending research and risk spikes

The backlog tracks **15** `type:research` tickets and **11** unresolved open engineering questions from [assumptions and open questions](research/assumptions-and-open-questions.md) and [risks and spikes](delivery/risks-and-spikes.md):

| Research focus | Owning tickets | Unresolved question |
|---|---|---|
| Windows hosting strategy | `WND-001`, `QA-008` | Verified attach/detach placement below icons across current Windows shell builds (`Progman`/`WorkerW`/`DefView`) |
| Worker confinement primitives | `SEC-002`, `SEC-003`, `SEC-004` | OS-specific restriction guarantees per platform/package format before advertising untrusted media/archive decode |
| Rust MSRV, graphics & FFmpeg family | `PRJ-002`, `REL-010` | Coherent compiled `wgpu`/`egui`/`winit`/`raw-window-handle` lockfile and `ffmpeg-the-third` vs `ffmpeg-next` ABI/licensing |
| Quality comparison baseline | `QA-009` | Controlled comparative evaluation of original conversion against prior art under matched fonts/repertoires |
| Plasma & GNOME presentation bridges | `LNX-005`, `LNX-007`, `QA-010` | Measured frame-copy vs glyph-grid overhead and shell-main-thread responsiveness without breaking isolation |
| Post-1.0 scene & signal research | `FX-015`, `INT-006`, `SCN-011`, `AST-038` | Versioned typed ports, local load/audio signal consent, mathematical pattern bounds, and upstream readiness |

## Regenerating and auditing external backlog data

Local planning data and derived reports reside externally in `../.issues/` so unpublished ticket details never enter Git. From `asciiwall/`:

```bash
python scripts/validate-backlog.py
python scripts/validate-backlog.py --backlog-root ../.issues --source-root . --write-reports --json
```

```powershell
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -DryRun
```

See [roadmap](roadmap.md), [dependency graph](dependency-graph.md), [requirements traceability](requirements-traceability.md), and [quality gates](quality-gates.md) for detailed schedules, chains, mappings, and acceptance criteria.
