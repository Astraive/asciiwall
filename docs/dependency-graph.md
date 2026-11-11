# Dependency graph and critical path

> **Model assumptions and non-calendar caveat:** Effort figures are estimated engineering person-days, **not measured duration or calendar dates**. Critical-path calculations cover **hard `depends_on` edges only** (`576` edges), exclude container coordination estimates (`163` person-days across `40` roadmaps/epics), assume unlimited parallel workers, zero handoff or hardware-lab contention, and stable scope. They represent a planning lower bound derived from `../.issues/reports/backlog-dependency-graph.md`.

## Graph summary

| Metric | Value | Notes |
|---|---:|---|
| Total vertices | **356** (`278` main, `78` library) | `316` atomic tasks/gates (`1,352` person-days) + `40` containers (`163` person-days excluded from paths) |
| Hard blocker edges | **576** (`blocker -> dependent`) | Strictly acyclic and transitively reduced (`0` redundant transitive edges; `0` parent/epic cycles) |
| Cross-repository hard edges | **38** | All flow `asciiwall -> asciiwall-library`; none block `L0` foundation |
| Effort-weighted longest chain (`QA-M10`) | **85 person-days** (`19` tickets) | Tracks full lifecycle from `PRJ-001` through post-launch `QA-M10` |
| Effort-weighted chain to 1.0 (`QA-M9`) | **80 person-days** (`17` tickets) | `QA-M9` precedes `REL-009` and `QA-M10`, which block only `post-1.0` |
| Library longest chain (`LQA-L3` / `LQA-L4`) | **69** / **74** person-days | Driven by upstream package packer readiness (`PKG-010 -> LIB-003 -> CAT-003 -> LQA-L1 -> LQA-L2 -> LREL-004 -> LQA-L3`) |

Parent containment (`roadmap -> epic -> task/gate`) and `related` links do not impose hard execution order; only explicit `depends_on` edges constrain execution readiness.

## The 85-person-day weighted critical path

Every member of the 19-ticket critical chain carries the derived `path:critical` label in [`.github/project-management/project.json`](../.github/project-management/project.json) (`source`: `effort-weighted longest hard-dependency chain; estimates not dates`):

| Step | Ticket | Repo / Milestone | Priority | Effort (days) | Cumulative (days) | Critical role |
|---:|---|---|---|---:|---:|---|
| 1 | `PRJ-001` | `asciiwall` / `M0` | `P0` | `5` | `5` | Exercised Rust workspace and crate boundaries |
| 2 | `COR-001` | `asciiwall` / `M0` | `P0` | `5` | `10` | Bounded frame, raster, budget, and error contracts |
| 3 | `CFG-001` | `asciiwall` / `M0` | `P0` | `5` | `15` | Typed settings registry and reversible migrations |
| 4 | `PKG-001` | `asciiwall` / `M2` | `P1` | `5` | `20` | Draft v1 manifest and scene schema types |
| 5 | `PKG-005` | `asciiwall` / `M4` | `P0` | `5` | `25` | Reject unsafe/overbudget ZIP transport pre-extract |
| 6 | `PKG-006` | `asciiwall` / `M4` | `P0` | `5` | `30` | Race-safe bounded private archive extraction |
| 7 | `PKG-007` | `asciiwall` / `M4` | `P0` | `5` | `35` | Restricted-worker resource validation before install |
| 8 | `PKG-008` | `asciiwall` / `M4` | `P1` | `5` | `40` | Atomic immutable import commits and rollback receipts |
| 9 | `LIBAPP-001` | `asciiwall` / `M5` | `P1` | `5` | `45` | Offline local import receipt indexing |
| 10 | `LIBAPP-002` | `asciiwall` / `M5` | `P1` | `5` | `50` | Local variants, exact version pins, safe deletion |
| 11 | `UIX-004` | `asciiwall` / `M5` | `P1` | `5` | `55` | Local library and immutable-package management UI |
| 12 | `ACC-001` | `asciiwall` / `M8` | `P1` | `5` | `60` | Keyboard-complete controls and native accessibility |
| 13 | `ACC-004` | `asciiwall` / `M8` | `P0` | `5` | `65` | Assistive-technology evidence gate |
| 14 | `REL-006` | `asciiwall` / `M8` | `P1` | `5` | `70` | Release manifest, signatures, notices, install receipt |
| 15 | `REL-007` | `asciiwall` / `M9` | `P1` | `5` | `75` | Signed update rollback and data-safe removal |
| 16 | `REL-008` | `asciiwall` / `M9` | `P1` | `3` | `78` | Vulnerability handling and emergency release drill |
| 17 | `QA-M9` | `asciiwall` / `M9` | `P0` | `2` | **`80`** | **1.0 Release acceptance gate** |
| 18 | `REL-009` | `asciiwall` / `M10` | `P1` | `3` | `83` | Post-launch dependency freshness & regressions |
| 19 | `QA-M10` | `asciiwall` / `M10` | `P0` | `2` | **`85`** | **Post-Launch maintenance gate** |

Notice that `P0` priority does **not** automatically equal critical-path membership (several `P1` package/UI/release steps serialize the longest chain), and `P0` gates/risks off the longest chain (such as `WND-001 -> WPH-001 -> WND-002 -> QA-M3` or `IMG-001 -> VID-001..004 -> SEC-005`) can still delay milestone completion if neglected.

## Highest-impact blocking nodes

From `../.issues/reports/backlog-dependency-graph.md`, these foundation contracts unlock the largest downstream subgraphs:

| Blocker ID | Milestone | Direct dependents | All reachable dependents | Key unlocked workstreams |
|---|---|---:|---:|---|
| `PRJ-001` | `M0` | `11` | `295` | Workspace crates, core runtime, security spikes, QA evidence tooling |
| `COR-001` | `M0` | `8` | `256` | Settings, fonts, images, conversion, Windows probe, performance metrics |
| `FNT-001` | `M1` | `4` | `165` | Density conversion, `wgpu` rendering, procedural scenes, glyph repertoires |
| `CFG-001` | `M0` | `23` | `157` | All `CFG-002`–`016` bindings, draft v1 package schema, power/visibility/input policies |
| `WND-001` | `M0` | `1` | `138` | Native capability leases (`WPH-001`) and downstream desktop host work |
| `WPH-001` | `M2` | `19` | `137` | Transactional primitives, IPC auth, Windows/macOS/Linux adapters, Apply |
| `PKG-001` | `M2` | `10` | `122` | Archive security (`PKG-005`–`008`), scene validator (`FX-001`), library tooling (`LIB-002`, `AST-037`, `CAT-001`) |
| `IMG-001` | `M1` | `2` | `107` | Video decoder boundary (`VID-001`), animation composition (`IMG-003`) |
| `VID-001` | `M4` | `2` | `103` | FFmpeg state machine (`VID-002`–`006`), media-worker confinement (`SEC-005`) |
| `SEC-001` | `M3` | `7` | `95` | Media worker confinement, CLI IPC client, Linux shell bridge, diagnostics (`QA-001`) |
| `CNV-001` | `M1` | `8` | `91` | Conversion stages, standalone CLI (`CLI-001`), export & preview paths |
| `SCN-001` | `M2` | `13` | `81` | All v1/later procedural generators (`SCN-002`–`010`), scene graph execution (`FX-001`), `QA-M2` |

## Cross-repository dependencies

**38** transitively reduced hard edges connect `asciiwall` milestones to `asciiwall-library` deliverables (`L0` remains unblocked on app shipping; `L1`–`L4` pin exact upstream contracts):

| Upstream `asciiwall` blocker | Downstream `asciiwall-library` dependent(s) | Technical coupling |
|---|---|---|
| `PKG-001` (`M2`) | `LIB-002` (`L0`), `AST-037` (`L1`), `CAT-001` (`L1`) | Pin reviewed manifest/scene schema version before locking recipe/catalog schemas |
| `CLI-001` (`M1`), `PKG-010` (`M6`) | `LIB-003` (`L1`) | Reproducible library packaging consumes real `ascii-cli` packer commands |
| `VID-012` (`M6`) | `LQA-002` (`L2`) | Deterministic recipe budget verification requires real engine bake evaluation |
| `PKG-012` (`M6`) | `LQA-004` (`L3`) | Library 1.0 conformance consumes frozen draft-v1 cross-platform package checks |
| `SCN-002`–`006` (`M2`), `FX-007` (`M4`), `FX-003`–`005` (`M5`) | `AST-001`–`AST-020` (`L2`) | Each of the 20 declarative v1 starter recipes depends on its exact built-in generator and optional reaction node |
| `VID-009` (`M4`), `VID-013` (`M6`) | `AST-027` (`ember-loop`), `AST-028` (`ink-loop`) (`L3`) | Precomputed clip and baked-video media recipes require real clip/video export support |
| `SCN-008`, `SCN-009`, `SCN-010` (`M10`) | `AST-033`, `AST-030`, `AST-036` (`L4`) | Later-capability recipes (`ascii-fire`, `boid-garden`, `day-night-field`) remain gated behind post-1.0 engine capabilities |

## Parallel startable roots

**15** atomic tasks have zero hard `depends_on` blockers at project start:
- **Immediate `M0` / `L0` starting set:** `PRJ-001`, `PRJ-003`, `ARC-004`, `DOC-001`, `DOC-003`, `LEG-001`, `QA-007`, `QA-008`, `REL-011`, and (independently in `asciiwall-library`) `LIB-001`.
- **Milestone-gated unblocked leaves/later prep:** `REL-002` (`M8` license audit), `QA-006` (`M8` benchmark publication), `SEC-008` (`M6` key rotation review), `FX-015` (`M10` research), `SCN-011` (`M10` research) have zero ticket-to-ticket hard edges, though their execution respects milestone entry criteria ([roadmap](roadmap.md)).

To inspect all 576 edges or regenerate the graph after future local backlog updates (kept outside Git):

```bash
python scripts/validate-backlog.py --write-reports
```
