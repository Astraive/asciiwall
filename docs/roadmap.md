# AsciiWall master roadmap

> **Status:** All **356** tickets across both repositories are in `Backlog` (`local-only`; **0** remote GitHub issues or Project items exist). Effort figures are estimated engineering person-days, **not calendar dates or delivery schedules**. No native platform or release artifact is currently built or verified.

This roadmap defines the expanded **M0–M10** application sequence (`ROADMAP-001`, 278 tickets) and independent **L0–L4** community library cadence (`LIB-ROADMAP-001`, 78 tickets) reconciled in `../.issues/milestones.json`, alongside the 18 declarative views defined for GitHub Project 4 (`PVT_kwDOEF_JvM4BrI0s`).

## Crosswalk from original M0–M8 to expanded M0–M10

The original blueprint in [milestones and dependencies](delivery/milestones-and-dependencies.md#recommended-sequence) proposed nine numbered stages (`M0`–`M8`). Local backlog reconciliation preserves every scope item while decomposing overloaded phases and adding explicit post-launch governance (`M10`):

| Original source proposal | Expanded local milestone (`gate`) | Key scope transition |
|---|---|---|
| `M0: risk spikes` | `M0 — Foundation` (`QA-M0`) | Folds foundation contracts (`PRJ-001`, `COR-001`, `CFG-001`) and `M0` risk spikes (`WND-001`, `SEC-002`–`004`, `PRJ-002`, `REL-010`) into explicit pass/fallback decisions |
| `M1: conversion tool` | `M1 — ASCII Engine Prototype` (`QA-M1`) | Preserved: headless conversion, glyph descriptors, PNG/JPEG decode, text/ANSI export, standalone `CLI-001` |
| `M2: visual engine` | `M2 — Native Renderer` (`QA-M2`) | Preserved: `wgpu` presenter (`RND-001`–`005`), v1 procedural scenes (`SCN-001`–`007`), draft schemas (`PKG-001`–`003`), stage metrics (`PERF-001`), host capability contract (`WPH-001`) |
| `M3: first real wallpaper` | `M3 — Windows First Wallpaper` (`QA-M3`) | Preserved: one verified Windows tuple below icons (`WND-002`), authenticated IPC (`SEC-001`), transactional/pause/scaling primitives |
| `M4: editable product` | **Moved to `M5`** (`QA-M5`) | Runtime media/effects/archive foundations precede full native editor investment |
| `M5: media and portability` | **Split into `M4` and `M6`** | Media decoding/timing/clips/effects/archive-import isolation move to `M4 — Media and Effects` (`QA-M4`); portable package freeze, bake exports, and catalog client move to `M6 — Package Ecosystem` (`QA-M6`) |
| `M6: community library` | `M6` + **Independent `L0`–`L4`** | App-side indexing/catalog client stays in `M5`/`M6` (`LIBAPP-*`); artwork repository follows `L0`–`L4` pinned to compatible engine contracts (`LQA-L0`–`LQA-L4`) |
| `M7: platform expansion` | `M7 — Cross-Platform Alpha` (`QA-M7`) | Preserved: independent macOS, X11, Wayland `layer-shell`, KDE Plasma, and GNOME adapters evaluated per named tuple |
| `M8: hardened release` | **Split into `M8` and `M9`** | Hardening, fuzzing, accessibility, packaging (`M8 — Beta`, `QA-M8`) separated from clean-machine release certification & rollback drills (`M9 — 1.0 Release`, `QA-M9`) |
| *(no post-launch stage)* | `M10 — Post-Launch` (`QA-M10`) | Explicit maintenance gate for `REL-009` and later-capability research (`FX-015`, `INT-006`, `CFG-016`, `SCN-008`–`011`); does **not** block `1.0.0` |

## Application milestones M0–M10

Container effort (`99` days across main app roadmaps/epics) covers coordination only and is shown separately from atomic engineering effort (`1,028` days across `M0`–`M10`; `1,352` total atomic days across both repositories). "Planned platforms" reflect ticket validation targets, **not** support claims.

| Milestone | Target release | Tickets (tasks / epics / roadmaps / gates) | Atomic effort (days) | Gate (`tests/evidence/gates/<M>/`) | Core exit focus |
|---|---|---:|---:|---|---|
| `M0 — Foundation` | `prototype` | `37` (`25` / `10` / `1` / `1`) | `113` | `QA-M0` | Pinned Rust/workspace decisions, core frame/settings contracts, Windows/confinement/codec spike decisions |
| `M1 — ASCII Engine Prototype` | `prototype` | `17` (`12` / `4` / `0` / `1`) | `54` | `QA-M1` | Bounded PNG/JPEG decode, deterministic density conversion, glyph masks, offline CLI, `EC-001`–`010` |
| `M2 — Native Renderer` | `prototype` | `23` (`20` / `2` / `0` / `1`) | `80` | `QA-M2` | Bounded `wgpu` glyph presenter, v1 generators `SCN-002`–`007`, draft schemas `PKG-001`–`003`, `WPH-001`, `EC-011`–`020` |
| `M3 — Windows First Wallpaper` | `windows-alpha` | `14` (`11` / `2` / `0` / `1`) | `55` | `QA-M3` | Named Windows tuple below icons (`WND-002`), Stop/rollback, Explorer/lock/hotplug recovery, `EC-041`–`050` |
| `M4 — Media and Effects` | `alpha` | `50` (`47` / `2` / `0` / `1`) | `191` | `QA-M4` | Restricted FFmpeg video & `.ascii-clip`, staged ZIP security (`PKG-005`–`008`), structural conversion, v1 effects, `EC-021`–`030` |
| `M5 — Full Desktop Application` | `alpha` | `61` (`54` / `6` / `0` / `1`) | `229` | `QA-M5` | Native `egui` editor, 164 v1 settings (`CFG-002`–`015`), multi-display, passive reactions, local index, `EC-031`–`040`, `051`–`060`, `081`–`090`, `101`–`110` |
| `M6 — Package Ecosystem` | `alpha` | `27` (`24` / `2` / `0` / `1`) | `110` | `QA-M6` | Draft v1 format freeze (`PKG-012`), deterministic bake/video export, optional signed catalog download, `EC-061`–`080`, `091`–`100` |
| `M7 — Cross-Platform Alpha` | `alpha` | `15` (`12` / `2` / `0` / `1`) | `58` | `QA-M7` | Independent macOS, X11, layer-shell, Plasma, and GNOME adapter evidence per named tuple (`WPH-005`) |
| `M8 — Beta` | `beta` | `22` (`20` / `1` / `0` / `1`) | `96` | `QA-M8` | Accessibility (`ACC-001`–`004`), fuzzing (`QA-002`), aggregate budgets (`PERF-006`), platform installers & notices, `EC-111`–`120` |
| `M9 — 1.0 Release` | `1.0.0` | `3` (`2` / `0` / `0` / `1`) | `10` | `QA-M9` | Clean-machine offline install/upgrade/downgrade/uninstall, signed release receipts, rollback & emergency drills (`REL-007`, `REL-008`) |
| `M10 — Post-Launch` | `post-1.0` | `9` (`8` / `0` / `0` / `1`) | `32` | `QA-M10` | Maintenance (`REL-009`), later scenes/signals/settings research (`SCN-008`–`011`, `FX-015`, `INT-006`, `CFG-016`) |

## Independent library cadence L0–L4

`asciiwall-library` operates on five milestones (`78` tickets total; `324` atomic person-days + `64` separate container person-days) coordinated under `LIB-ROADMAP-001`. Foundation work (`L0`) is **not** blocked on shipping the full desktop app, while `L1`–`L4` pin real compatible engine contracts (`38` cross-repository edges in [dependency graph](dependency-graph.md#cross-repository-dependencies)):

| Library milestone | Target release | Tickets (tasks / epics / roadmaps / gates) | Atomic effort (days) | Gate (`libraryreviews/gates/<L>/`) | Core exit focus and upstream pins |
|---|---|---:|---:|---|---|
| `L0 — Library Foundation` | `library-foundation` | `10` (`4` / `4` / `1` / `1`) | `17` | `LQA-L0` | Separate repo layout (`LIB-001`), license layers (`LLEG-001`), intake rules (`LCOM-001`), engine pin contract (`LIB-002` pins `PKG-001`) |
| `L1 — Package and Catalog Tooling` | `library-alpha` | `15` (`11` / `3` / `0` / `1`) | `54` | `LQA-L1` | Reproducible packer (`LIB-003` pins `CLI-001`/`PKG-010`), digest-pinned assets (`LIB-004`), closed static catalog + detached signatures (`CAT-001`–`005`), secretless PR checks (`LQA-001`) |
| `L2 — Starter Collection Alpha` | `library-alpha` | `24` (`23` / `0` / `0` / `1`) | `116` | `LQA-L2` | 20 reviewed declarative v1 recipes (`AST-001`–`020`) consuming `SCN-002`–`006` and `FX-003`–`005,007`; budget & deduplication checks (`LQA-002`–`003`) |
| `L3 — Compatible Library 1.0` | `library-1.0.0` | `16` (`15` / `0` / `0` / `1`) | `76` | `LQA-L3` | 8 rights-cleared media recipes (`AST-021`–`028`), font/FFmpeg rights audit (`LLEG-003`), cross-platform conformance (`LQA-004` pins `PKG-012`), immutable 1.0 catalog & key rotation (`LREL-001`, `003`, `004`) |
| `L4 — Maintenance` | `library-post-1.0` | `13` (`12` / `0` / `0` / `1`) | `61` | `LQA-L4` | 8 later-capability recipes (`AST-029`–`036`) + upstream readiness review (`AST-038`), takedown ledger (`LLEG-004`), maintenance cycles (`LREL-006`); does **not** block `library-1.0.0` |

## GitHub Project 4 views and pending configuration

Shared organization Project 4 (`PVT_kwDOEF_JvM4BrI0s`, private, initially `0` items) defines **20** managed fields (native `Title`, `Labels`, `Repository`, `Milestone`, `Assignees`, `Parent issue`, `Sub-issues progress`, `Updated` plus configurable `Status`, `Priority`, `Size`, `Work type`, `Kind`, `Area`, `Platform` [`MULTI_SELECT`], `Repo role`, `Phase`, `Blocked`, `Target release`, `Effort`) and **18** declarative views stored in [static project definition](../.github/project-management/project.json) and external `../.issues/project.json`. GitHub reserves `Type` for native issue types; `Work type` preserves the full 14-way planning classification without shadowing that native field:

- **Portfolio & dependency views:** `Master Roadmap` (`BOARD_LAYOUT` by `Phase`/`Status`), `All Backlog Items`, `Milestone Roadmap` (explicit `TABLE_LAYOUT` fallback because no calendar dates or iteration fields exist), `Epics`, `P0 Critical Path` (`priority:P0` + `path:critical`), `Ready for Development`, `Currently Blocked`, `Completed Work`.
- **Platform & subsystem views:** `Windows Development`, `macOS Development`, `Linux Development`, `Rust Engine`, `UI and Customization`, `Media Processing`, `QA and Verification`, `Security`, `Library Contributions`, `Release Readiness`.

Remote issue and project-item creation remains **withheld**. In addition, GitHub's `ProjectV2ViewConfigurationInput` GraphQL schema currently exposes `visibleFieldIds` without grouping/sorting inputs, so UI grouping and sorting require manual interactive configuration after authenticated sign-in. See [GitHub setup](github-setup.md) and [backlog publication](backlog-publication.md).
