# Quality gates and acceptance policy

> **Evidence boundary:** All **16** milestone gates (`QA-M0`–`QA-M10` and `LQA-L0`–`LQA-L4`) and **120** edge-case scenarios (`EC-001`–`EC-120`) are **unexecuted planning gates**. Current verification covers only repository hygiene, Markdown/JSON checks, and external backlog validation—never Rust builds, native wallpaper hosts, media confinement, fuzzing, or accessibility sign-off ([documentation verification](quality/documentation-verification.md)).

## Gate model and non-waivable rules

Defined across [acceptance criteria](quality/acceptance-criteria.md), [test strategy](quality/test-strategy.md), and `../.issues/milestones.json`, every milestone closes only through its named gate ticket—never merely because child tickets are closed or because a build compiles:

- **Explicit outcome per in-scope item:** Every in-scope criterion and `EC-*` row must record `Pass` or an explicitly approved **non-security waiver** with a linked follow-up ticket.
- **Zero unresolved P0 or critical failures:** Security vulnerabilities, sandbox/archive path escape (`EC-061`–`EC-067`), unauthorized input/window observation (`EC-031`–`EC-046`, `EC-083`), destructive file/override loss (`EC-070`, `EC-089`, `EC-090`), and unrecoverable topmost/focus-stealing desktop states **cannot be waived**—including by labelling a release `alpha` or `beta`.
- **Required sign-offs:**
  - `M0` / `L0`: Responsible implementation maintainer, Independent QA/evidence reviewer, Project owner milestone approval.
  - `M1`–`M10` / `L1`–`L4`: Adds **Security reviewer** for changed trust boundaries and **Rights/provenance reviewer** for distributed assets, fonts, or dependencies.
- **Reproducible evidence paths:** Proposed gate evidence directories (`tests/evidence/gates/<M>/` in `asciiwall` and `libraryreviews/gates/<L>/` in `asciiwall-library`) record sign-off files, exact source/binary hashes, fixture digests, and named OS/GPU/compositor environments. No fabricated evidence artifacts exist in either repository today.

## Application gates QA-M0 through QA-M10

| Gate | Milestone | Target release | Blocking scope | Key exit & acceptance evidence required |
|---|---|---|---|---|
| `QA-M0` | `M0 — Foundation` | `prototype` | `1.0.0` | Pinned Rust/workspace & graphics/codec choices (`PRJ-001`–`002`), frame/settings/IPC contracts (`COR-001`, `CFG-001`), Windows/Linux host spikes (`WND-001`) & worker confinement feasibility (`SEC-002`–`004`) |
| `QA-M1` | `M1 — ASCII Engine Prototype` | `prototype` | `1.0.0` | Bounded PNG/JPEG decode (`IMG-001`), deterministic density conversion & glyph masks (`CNV-001`–`003`, `FNT-001`–`002`), offline CLI (`CLI-001`), `EC-001`–`010` (`QA-101`–`102`) |
| `QA-M2` | `M2 — Native Renderer` | `prototype` | `1.0.0` | Bounded `wgpu` presenter & surface recovery (`RND-001`–`005`), v1 generators `SCN-001`–`007` p50/p95/p99 + static idle, `WPH-001`, `EC-011`–`020` (`QA-103`–`104`) |
| `QA-M3` | `M3 — Windows First Wallpaper` | `windows-alpha` | `1.0.0` | Named Windows tuple below icons without focus steal (`WND-002`), Stop/rollback, Explorer/lock/hotplug recovery, `EC-041`–`050` (`QA-109`–`110`) |
| `QA-M4` | `M4 — Media and Effects` | `alpha` | `1.0.0` | FFmpeg VFR/seek/drain (`VID-001`–`006`), `.ascii-clip` validation (`VID-007`–`009`), media-worker confinement (`SEC-005`), staged ZIP import safety (`PKG-005`–`008`), `EC-021`–`030` (`QA-105`–`106`) |
| `QA-M5` | `M5 — Full Desktop Application` | `alpha` | `1.0.0` | Requested/effective inspector, draft/Apply/undo rollback, multi-output hotplug, local library index, `EC-031`–`040`, `051`–`060`, `081`–`090`, `101`–`110` (`QA-107`–`108`, `111`–`112`, `117`–`118`, `121`–`122`) |
| `QA-M6` | `M6 — Package Ecosystem` | `alpha` | `1.0.0` | Cross-platform v1 package conformance freeze (`PKG-012`), deterministic bake/video export (`VID-012`–`014`), explicit signed catalog download (`LIBAPP-004`–`006`), `EC-061`–`080`, `091`–`100` (`QA-113`–`116`, `119`–`120`) |
| `QA-M7` | `M7 — Cross-Platform Alpha` | `alpha` | `1.0.0` | Separate per-adapter promotion (`WPH-005`, `QA-010`) on named macOS, X11, layer-shell, Plasma, and GNOME tuples; unverified hosts stay experimental |
| `QA-M8` | `M8 — Beta` | `beta` | `1.0.0` | Isolated fuzz corpus (`QA-002`), fault stress (`QA-005`), aggregate budgets (`PERF-006`), assistive-tech evidence (`ACC-004`), platform installers (`REL-003`–`006`), `EC-111`–`120` (`QA-123`–`124`) |
| `QA-M9` | `M9 — 1.0 Release` | `1.0.0` | `1.0.0` | Clean-machine offline install/upgrade/downgrade/uninstall on every claimed tuple (`REL-007`), artifact hashes/SBOM/signing receipts, rollback & emergency drill (`REL-008`) |
| `QA-M10` | `M10 — Post-Launch` | `post-1.0` | `post-1.0` only | Maintenance regression/advisory review (`REL-009`), later scenes/signals/settings gating (`CFG-016`, `FX-015`, `INT-006`, `SCN-008`–`011`); **does not block `1.0.0`** |

## Library gates LQA-L0 through LQA-L4

| Gate | Milestone | Target release | Blocking scope | Key exit & acceptance evidence required |
|---|---|---|---|---|
| `LQA-L0` | `L0 — Library Foundation` | `library-foundation` | `library-1.0.0` | Recipe/source/artifact separation (`LIB-001`), license layers (`LLEG-001`), intake rules (`LCOM-001`), reviewed engine pin (`LIB-002`) |
| `LQA-L1` | `L1 — Package and Catalog Tooling` | `library-alpha` | `library-1.0.0` | Secretless untrusted PR validation (`LQA-001`), pinned packer (`LIB-003`), digest-pinned assets (`LIB-004`), closed static catalog & detached signatures (`CAT-001`–`005`) |
| `LQA-L2` | `L2 — Starter Collection Alpha` | `library-alpha` | `library-1.0.0` | All 36 briefs explicitly triaged (`v1` vs `asset-needed` vs `later`); 20 v1 declarative recipes (`AST-001`–`020`) pass budgets (`LQA-002`), deduplication (`LQA-003`), and human review (`LCOM-002`) |
| `LQA-L3` | `L3 — Compatible Library 1.0` | `library-1.0.0` | `library-1.0.0` | 8 rights-cleared media recipes (`AST-021`–`028`), font/codec audit (`LLEG-003`), cross-platform conformance (`LQA-004`), immutable release & key-rotation rehearsal (`LREL-001`, `003`, `004`) |
| `LQA-L4` | `L4 — Maintenance` | `library-post-1.0` | `library-post-1.0` only | Compatibility/rights maintenance cycles (`LREL-006`), takedown ledger (`LLEG-004`), upstream readiness for 8 later recipes (`AST-029`–`036`, `AST-038`); **does not block `library-1.0.0`** |

## Evidence levels and current verification scope

Following [test strategy](quality/test-strategy.md), reports never collapse distinct evidence classes into a generic "tested" claim:

- **Currently available repository & backlog checks:**
  - `python scripts/check-repository.py` — validates tracked Markdown fences/links/fragments, JSON syntax, and exclusion of forbidden external backlog/ledger files.
  - `python scripts/check-conventions.py` and `pwsh -NoProfile -File scripts/check-powershell.ps1` — validates commit/PR format, no-coauthor rule, and PowerShell scripts.
  - `python scripts/validate-backlog.py` — verifies external `../.issues/` schema, links, acyclic graphs, 16 gates, and coverage across 100 docs, 172 settings, 120 ECs, 36 recipes, and 40 work units.
  - `pwsh -NoProfile -File ./scripts/publish-issues.ps1 -DryRun` — verifies offline publication planning without remote mutations.
  - Design-pack schema/arithmetic checks summarized in [documentation verification](quality/documentation-verification.md) (JSON Schema Draft 2020-12 fixtures, 64B/40B/12B `.ascii-clip` arithmetic).
- **Not yet run (requires future implementation milestones):** Rust unit/property/golden tests, fake-host/fake-decoder model tests, `media-worker` sandbox denial tests, `cargo-fuzz` campaigns, named-machine GPU/energy benchmarks ([benchmark plan](quality/benchmark-plan.md)), assistive-technology checks, manual Windows/macOS/Linux host cycles, and signed clean-machine install/uninstall drills.

## Edge-case matrix ownership

All **120** failure scenarios in [edge-case matrix](quality/edge-case-matrix.md) (`73` `P0`, `47` `P1`) are assigned in five-scenario blocks to `QA-101`–`QA-124` alongside their subsystem implementation tickets:

| Scenario range | Domain | P0 / P1 count | QA verification owner | Target milestone |
|---|---|---:|---|---|
| `EC-001`–`EC-010` | Input and conversion | `4` P0 / `6` P1 | `QA-101`, `QA-102` | `M1` |
| `EC-011`–`EC-020` | Glyphs and rendering | `5` P0 / `5` P1 | `QA-103`, `QA-104` | `M2` |
| `EC-021`–`EC-030` | Video and clocks | `5` P0 / `5` P1 | `QA-105`, `QA-106` | `M4` |
| `EC-031`–`EC-040` | Pointer and windows | `5` P0 / `5` P1 | `QA-107`, `QA-108` | `M5` |
| `EC-041`–`EC-050` | Desktop lifecycle | `9` P0 / `1` P1 | `QA-109`, `QA-110` | `M3` |
| `EC-051`–`EC-060` | Displays and resources | `4` P0 / `6` P1 | `QA-111`, `QA-112` | `M5` |
| `EC-061`–`EC-070` | Archives and packages | `9` P0 / `1` P1 | `QA-113`, `QA-114` | `M6` |
| `EC-071`–`EC-080` | Scenes and clips | `9` P0 / `1` P1 | `QA-115`, `QA-116` | `M6` |
| `EC-081`–`EC-090` | Settings and transactions | `6` P0 / `4` P1 | `QA-117`, `QA-118` | `M5` |
| `EC-091`–`EC-100` | Catalog and distribution | `8` P0 / `2` P1 | `QA-119`, `QA-120` | `M6` |
| `EC-101`–`EC-110` | CLI and diagnostics | `7` P0 / `3` P1 | `QA-121`, `QA-122` | `M5` |
| `EC-111`–`EC-120` | UX and accessibility | `2` P0 / `8` P1 | `QA-123`, `QA-124` | `M8` |
