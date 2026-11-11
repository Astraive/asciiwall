# Backlog and issue conventions

> **Publication state:** All **356** backlog tickets exist **only** in the external local workspace `../.issues/` (`publication_status: "local-only"`). **0** GitHub issues, pull-request linkages, or Project items have been created remotely. Stable ticket IDs (such as `PRJ-001`, `AST-001`, `QA-M0`) are **not** GitHub issue numbers—never invent placeholder `#123` issue URLs or fabricated `-SourceRef` commit SHAs.

## External backlog isolation and publication status

Planning records (`issue.json`, `CONTENT.md`, coverage slices, and derived reports) live outside both Git repositories in `../.issues/` so raw backlog inventories are never committed or pushed to Git. Tracked repository files hold only static registry definitions under [`.github/project-management/`](../.github/project-management/project.json), validation/publication tooling, and these summary guides.

| Surface | Repository tracked (`.github/project-management/`) | External workspace (`../.issues/`) |
|---|---|---|
| Purpose | Static labels, milestone goals/gates, Project 4 field & view schema | Canonical `356` ticket records, `8` coverage slices, `5` derived reports, reference audit |
| Ticket bodies & membership | Excluded intentionally | Stored in `../.issues/asciiwall/<ID>/` (`278`) and `../.issues/asciiwall-library/<ID>/` (`78`) |
| Publication status | Applied only to repo settings/labels/milestones/fields/views via [configure-github.ps1](../scripts/configure-github.ps1) | Issue/relationship/item creation via [publish-issues.ps1](../scripts/publish-issues.ps1) `-Apply` remains **withheld** |

See [backlog validation](backlog-validation.md), [backlog publication](backlog-publication.md), and [GitHub setup](github-setup.md) for isolation rules, validation guarantees, and Project configuration details.

## Stable ticket identifiers and hierarchy

Tickets use globally unique uppercase `PREFIX-NNN` (or `PREFIX-EPIC` / `ROADMAP-001` / `QA-M*` / `LQA-L*`) identifiers organized into a three-level tree (`roadmap -> epic -> task | gate`):

- **Roadmaps (`2`, `kind:roadmap`):** `ROADMAP-001` (`asciiwall`, `M0`, `P0`) and `LIB-ROADMAP-001` (`asciiwall-library`, `L0`, `P1`).
- **Epics (`38`, `kind:epic`):** `31` main-app domain epics under `ROADMAP-001` and `7` library epics (`LIB-EPIC`, `CAT-EPIC`, `AST-EPIC`, `LCOM-EPIC`, `LLEG-EPIC`, `LQA-EPIC`, `LREL-EPIC`) under `LIB-ROADMAP-001`.
- **Atomic implementation tasks (`300`, `kind:task`):** `235` in `asciiwall` and `65` in `asciiwall-library`; sized `S`/`M`/`L` (`<= 5` person-days each).
- **Milestone gates (`16`, `kind:gate`):** `QA-M0`–`QA-M10` (`2` days each) and `LQA-L0`–`LQA-L4` (`3` days each), one evidence-bearing gate per milestone.

Every local `CONTENT.md` validated by [validate-backlog.py](../scripts/validate-backlog.py) follows 20 mandatory headings (`Metadata`, `Summary`, `Context`, `Why It Matters`, `Scope`, `In Scope`, `Out of Scope`, `Where to Look`, `Assets and Dependencies`, `Requirements`, `Technical Notes`, `Acceptance Criteria`, `Test Plan`, `Negative Cases and Edge Cases`, `References`, `Dependencies`, `Depends On`, `Blocks / Unblocks`, `Related Issues`, `Tracking`). Hard execution blockers use explicit ticket Markdown links under `Depends On` (`depends_on` in `issue.json`), while `Related Issues` captures non-blocking soft context only.

## Shared label taxonomy and derived labels

Both repositories share **78** canonical labels defined in [`.github/project-management/labels.json`](../.github/project-management/labels.json) (matching `../.issues/labels.json`):

| Label family | Count | Values and conventions |
|---|---:|---|
| `priority:*` | `4` | `P0` (`FF6476` critical/milestone-blocking), `P1` (`FFC65A` required), `P2` (`B7FF36` standard), `P3` (`66776C` optional/deferred) |
| `size:*` | `4` | `S` (`<= 1d`), `M` (`> 1–3d`), `L` (`> 3–5d`), `XL` (`<= 8d`, **containers only**; atomic tasks must be `<= L`) |
| `type:*` | `14` | `epic`, `feature`, `task`, `bug`, `enhancement`, `chore`, `documentation`, `test`, `research`, `security`, `refactor`, `performance`, `maintenance`, plus `type:duplicate` retained for triage |
| `platform:*` | `10` | `all`, `windows`, `macos`, `linux`, `x11`, `wayland`, `kde`, `gnome`, `cli`, `headless` (each ticket carries exact applicable targets; never collapsed to one OS) |
| `kind:*` | `4` | `roadmap`, `epic`, `task`, `gate` — kept distinct from `type:*` so roadmaps (`type:epic`, `kind:roadmap`) are separable from domain epics |
| `path:*` | `1` | `path:critical` — derived label declared in `project.json` marking the **19** tickets on the 85-day effort-weighted longest hard-dependency chain |
| `area:*` | `41` | Canonical domain ownership tags (`accessibility`, `architecture`, `ascii-conversion`, `catalog`, `cli`, `community`, `diagnostics`, `documentation`, `fonts`, `image-decoding`, `interactions`, `ipc`, `library-foundation`, `library-quality`, `library-release`, `licensing`, `linux-host`, `local-library`, `macos-host`, `media`, `multi-monitor`, `native-packaging`, `native-ui`, `package-format`, `performance`, `platform-hosting`, `power-lifecycle`, `privacy`, `procedural-scenes`, `provenance`, `quality`, `release-engineering`, `rendering`, `runtime`, `scene-effects`, `scene-graph`, `security`, `settings`, `starter-artwork`, `transitions`, `windows-host`) |

## Milestone and project field mapping

The managed Project 4 schema (`PVT_kwDOEF_JvM4BrI0s`) has **20** definitions: **8** native field roles (`Title`, `Labels`, `Repository`, `Milestone`, `Assignees`, `Parent issue`, `Sub-issues progress`, `Updated`) and **12** configurable fields: the existing `Status` plus `Priority`, `Size`, `Work type`, `Kind`, `Area`, `Platform` [`MULTI_SELECT`], `Repo role`, `Phase` [`16` options matching `M0`–`M10` / `L0`–`L4`], `Blocked` [`Yes`/`No` computed during publication], `Target release` [`TEXT`], and `Effort` [`NUMBER`]. Other existing GitHub native fields are preserved. Native Repository, Milestone, and Assignees are never shadowed by custom copies.

GitHub reserves `Type` for native issue types. The detailed 14-way classification therefore uses **Work type**, sourced from unchanged `issue.json.type` and `type:*` labels; native issue types remain Task, Bug, or Feature. No alias or duplicate native Type field is created.

- **Status mapping:** Desired workflow states are `Backlog`, `Ready`, `In Progress`, `In Review`, `Blocked`, `Done`. Historical Project 4 initially observed `Todo`, `In Progress`, `Done` (`0` items).
- **Definition of Ready:** Acceptance criteria and ownership are clear, required assets/fixtures are available, **all** hard blockers in `depends_on` are `Done`, and atomic size is `<= L`.
- **Definition of Done:** All acceptance evidence is reviewed and attached; milestone gates additionally require named owner sign-offs and zero unresolved P0/security/destructive-data defects.

## PR title and commit conventions

Enforced by [check-conventions.py](../scripts/check-conventions.py) and `.githooks/commit-msg`:

- **Required format:** `type(scope): concise subject [TICKET-ID]`
  - Allowed types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
  - Lowercase scope required (e.g., `docs(planning): add central guides [DOC-001]` or `feat(convert): implement density matcher [CNV-001]`).
  - Bootstrap exception: the initial externally merged docs PR #1 (`docs: add product and engineering specifications`, merge SHA `3801d374bd4b879105e7d6e2d08179eeed0695e4`) predates repository convention checks and is preserved without history rewriting.
- **No `Co-authored-by` trailers:** Checked case-insensitively across commit messages, PR descriptions, and merge bodies.
- **Partial work vs closure:** Reference the ticket ID (`Refs [PRJ-001]`) with exact observed command evidence and affected platforms; do not claim full ticket or gate completion from partial or mocked checks.
- **Contributor identity:** The maintainer identity (`Neeraj Sathish Kumar <neerajcodz@gmail.com>`) is used for bootstrap repository setup commits only; future contributors commit under their own Git identity.

## Reference provenance and boundaries

Backlog structure adapts conventions inspected read-only from five issues (`#2`, `#3`, `#24`, `#30`, `#675`), label/milestone registries, and `.github/pull_request_template.md` on `Astraive/voltshift`, plus live Project 4 / org issue-type introspection (`../.issues/reference-audit.md`):
- **Adopted:** Stable ticket ID hierarchy, epic goal/ownership/exit structure, milestone acceptance gates (`#675` evidence-row model), PR evidence checklists, supersession crosswalks (`#2`), and semantic label color families (`priority:P0 = FF6476`, `priority:P1 = FFC65A`, `priority:P2 = B7FF36`, `priority:P3 = 66776C`, shared `AABBB0`, platform `73CFFF`).
- **Explicitly excluded / tightened:** Zero Voltshift game content, assets, characters, gameplay, Unity/Convex/mobile/web domains, or banned-reference/vehicle checklists were copied. `size:XL` is tightened from the reference's 6–10 days to `<= 8 days, containers only`, and security/destructive-data failures are explicitly non-waivable.
