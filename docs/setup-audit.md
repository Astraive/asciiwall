# Repository and project setup audit

Publication audit: **2026-11-12**, after full 356-issue backlog publication and independent REST/GraphQL audit; view layout verification: **2026-11-13**, after the authorized UI save/reload and fresh GraphQL readback. The **2026-11-11** pre-publication infrastructure baseline is preserved below. This records observed setup and planning-publication state, not application readiness. Subsequent audit-only commits must pass the same protected-branch checks before merge.

## Current scope and full publication audit (2026-11-12)

- Application: [`Astraive/asciiwall`](https://github.com/Astraive/asciiwall) (**278** published canonical issues).
- Community library: [`Astraive/asciiwall-library`](https://github.com/Astraive/asciiwall-library) (**78** published canonical issues).
- Shared private project: [AsciiWall — Master Development Roadmap](https://github.com/orgs/Astraive/projects/4), `PVT_kwDOEF_JvM4BrI0s` (**356** real issue items, **0** drafts).
- Both repositories remain public with their original MIT licenses and history preserved. Project visibility remains private.
- Detailed tickets, coverage, publication ledger (`../.issues/publication-ledger.json`), and generated reports remain in the workspace sibling `../.issues`, outside both Git repositories. Only static configuration definitions and summary documentation are version controlled.

### Publication and independent REST/GraphQL audit results

- **Published issue inventory:** All **356** planned backlog tickets are published across both repositories (`278` `Astraive/asciiwall` + `78` `Astraive/asciiwall-library`). The initial **39**-issue foundation batch (`37` `asciiwall/M0` members plus parent containers `CFG-EPIC` in `M5` and `WND-EPIC` in `M3`) stayed protected read-only, while **317** remaining tickets (`239` `asciiwall`, `78` `asciiwall-library`, including all `17` canonical `asciiwall/M1` tickets in `M1 — ASCII Engine Prototype`) were created and verified via `-Apply -CreateMissing` with creator login `NeerajCodz`.
- **Independent native graph and preservation audit:** Fresh independent REST + GraphQL verification (`../.issues/reports/full-publication-native-graph.json` and `../.issues/reports/remaining-project-after.json`) passed with:
  - **356** canonical issues and **356** real Project 4 issue items (**0** drafts).
  - **354** native parent/sub-issue edges (`sub_issues`).
  - **576** native hard-blocker edges (`dependencies/blocked_by`), including **38** cross-repository blockers.
  - **317** newly created issues all authored by `NeerajCodz`.
  - All **39** protected issue bodies, titles, states, milestones, label names, and assignees verified **exact unchanged**; all **39** project `Status` values and **all** custom project field IDs and values unchanged.
- **Exact ledger preservation:** All **39** protected ticket records now match their original snapshot exactly, including timestamp strings. PowerShell's default JSON reader had rewritten only their `verification.checked_at` UTC offsets without changing the instants; the original strings were restored under the exclusive ledger lock. An actual publisher JSON-read/atomic-write smoke passed, and independent comparison confirmed that all **317** new records and the mutation journal were unchanged. The publisher now requires PowerShell 7.5+ and uses `ConvertFrom-Json -DateKind String` for lossless ledger and opaque text values. Combined evidence: `../.issues/reports/full-publication-audit.json`.
- **Merged publication PRs:** [Application PR #46](https://github.com/Astraive/asciiwall/pull/46) (`ef3097520db6af902d213ff3b12d3c16f0c74957`, all required CI checks green) and [Application PR #47](https://github.com/Astraive/asciiwall/pull/47) (`8d7878b`, schema-guard fix, all required CI checks green).

## Local inventory and coverage

| Repository | Roadmaps | Epics | Tasks | Gates | Total |
|---|---:|---:|---:|---:|---:|
| `asciiwall` | 1 | 31 | 235 | 11 | 278 |
| `asciiwall-library` | 1 | 7 | 65 | 5 | 78 |
| **Combined** | **2** | **38** | **300** | **16** | **356** |

The current validator reports **zero errors**, **2,915 requirement mappings**, eight coverage slices, and coverage of all 100 original source documents. Planning crosswalks include all 172 settings keys, 120 edge-case scenarios, 36 recipe briefs, and 40 implementation work units. These are traced requirements, not implemented features or passed native tests.

The hard-dependency graph contains **576** edges, including 38 cross-repository edges. It is acyclic and transitively reduced. The effort-weighted longest chain spans 19 tickets and 85 estimated person-days; this is a planning lower bound, not a calendar delivery commitment. Atomic effort totals 1,352 estimated person-days; 163 container-coordination days are reported separately. See [dependency analysis](dependency-graph.md) and [requirements traceability](requirements-traceability.md).

Regenerate local evidence with:

```sh
python scripts/validate-backlog.py --write-reports --json
```

The five derived backlog reports remain under `../.issues/reports/`; do not copy them into Git.

## Historical pre-publication baseline (2026-11-11)

The table and summary below preserve the **2026-11-11** zero-issue infrastructure snapshot taken after live repository/project configuration and before foundation PR merge or issue publication:

| Surface | Application | Library |
|---|---:|---:|
| Managed canonical labels | 78 | 78 |
| Total labels, including preserved GitHub defaults | 87 | 87 |
| Milestones with goals, entry/exit evidence, gates, and sign-off roles | 11 (`M0`–`M10`) | 5 (`L0`–`L4`) |
| Published issues | 0 | 0 |

Both repositories have distinct descriptions/topics, enabled Discussions with suitable native categories, squash-only merges, automatic merged-branch deletion, private vulnerability reporting, Dependabot alerts/security updates, secret scanning, and push protection. Default workflow tokens have read-only permissions and cannot approve PRs. Repository and organization-visible Actions secrets and configured environments were queried: all counts were zero at this snapshot; this setup installs no signing credentials.

Both `main` branches were updated and independently re-fetched with:

- Required checks: **Repository quality** and **Python security analysis**.
- Strict up-to-date status checks and administrator enforcement; no administrative CI bypass.
- Required pull requests, resolved conversations, and linear history.
- Force pushes and branch deletion disabled.
- Zero required approvals for the documented single-maintainer bootstrap; stale approvals are dismissed. This is not a claim of independent human review. Signing and mandatory code-owner approval remain unenforced until their prerequisites exist.

The authoritative external infrastructure report is `../.issues/reports/github-config-protected.json`: **233 verified operations, 18 manual-only view operations, zero apply failures**. Earlier failed attempts remain separate diagnostic reports; they are not the current completion state.

## Project fields and views

All **20 managed field definitions** were read back successfully. The project has **24 total native/custom fields**, retaining GitHub's four additional native fields, and both repositories are linked.

- Six statuses: Backlog, Ready, In Progress, In Review, Blocked, Done. The original Todo option was renamed to Backlog while the project had zero items; its ID and the original In Progress/Done IDs were preserved.
- Priority, Size, Kind, 41 Area choices, native multi-select Platform, Repo role, 16 Phase choices, Blocked, text Target release, and numeric Effort are configured.
- GitHub reserves `Type` for native issue types. Detailed classification uses **Work type** with all 14 planned values, sourced from unchanged ticket `type` metadata. Native Task/Bug/Feature types remain separate; no alias or shadow native Type field was created.
- Repository, Milestone, Assignees, Title, Labels, Parent issue, Sub-issues progress, and Updated remain native.
- All **18 views** have their declared names, supported layouts, filters, and visible-field IDs configured and re-fetched. The original View 1 ID was reused for `Master Roadmap`. `Milestone Roadmap` uses the documented table fallback because no schedule dates were approved.

### Project view layout cutover and API-vs-UI verification

GitHub Projects tables support a single `group_by` field (`groupByFields`) and expose the approved secondary dimension through the interactive **Slice by** sidebar (`slice_by`), whereas boards support both Columns (`verticalGroupByFields`) and Swimlanes (`groupByFields`). Because `ProjectV2View` exposes `groupByFields`, `verticalGroupByFields`, and ordered `sortByFields` for readback—and exposes **no** public GraphQL read or write field for `Slice by`—the view registry and `configure-github.ps1 -VerifyOnly` distinguish API-verifiable layout axes from UI-only `Slice by` state:

| View scope | Registry axes | Native GraphQL readback (`view-api:<view>`) | `Slice by` (`view-slice:<view>`) | Current observed state |
|---|---|---|---|---|
| `Master Roadmap` (`BOARD_LAYOUT`) | `group_by: "Phase"`, `secondary_group_by: "Status"`, `slice_by: null`, `sort_by: ["Priority", "Milestone"]` | `verticalGroupByFields` -> `primary_group_by` (`Phase`); `groupByFields` -> `secondary_group_by` (`Status`); ordered `sortByFields` (`Priority`, `Milestone`) | `null` | **Saved in UI and API-verified** |
| 17 table views (`TABLE_LAYOUT`) | Single `group_by`, `secondary_group_by: null`, approved secondary moved to `slice_by`, two ordered `sort_by` fields | `groupByFields` -> `primary_group_by`; `secondary_group_by: null`; ordered `sortByFields` | `manual-only-unverifiable-by-api` (`observed: null`) | **Saved and reloaded in authenticated Orca UI; all 17 slices observed in UI; all native group/sort fields API-verified** |

1. All 17 `TABLE_LAYOUT` views are configured, saved, and reloaded in the authenticated Orca embedded browser session. The reloaded UI showed each registry `slice_by`; ProjectV2 API readback matched the registry's layout-specific grouping and ordered sorts.
2. Per-view screenshot paths, SHA-256 hashes, sizes, slice listbox reads, and API grouping/sort results are recorded outside the repository in `../.issues/reports/project-view-ui-verification.json`; PNGs are in `../.issues/reports/project-view-ui-verification/`.
3. Run the read-only API audit below. `view-api:<view>` checks native grouping and ordered sorts; table `view-slice:<view>` remains `manual-only-unverifiable-by-api` because GitHub exposes no public Slice by read/write. The manual-only result is deliberate, and `-VerifyOnly` exits nonzero while such operations or other mismatches remain.

```powershell
pwsh -NoProfile -File scripts/configure-github.ps1 -VerifyOnly
```

Do not re-run full `configure-github.ps1 -Apply` over populated milestones or manually saved views, as it overwrites native milestone descriptions with static summaries.

## Exercised verification

- Actual Windows Git-for-Windows `pre-commit` and `commit-msg` hooks passed. Both checkouts opted into local hooks explicitly; author/global configuration was not rewritten. An isolated existing-custom-hook scenario was rejected without changing that hook path.
- Local repository checks passed; all workflow/template YAML parsed, and four application/two library PowerShell files parsed without execution.
- The complete **356-object offline issue-publication plan** passed with a deliberately nonexistent `gh` executable, including after the Work type mapping cutover. All parent/blocker creation precedences were checked; project Platform values retained zero/one/many cardinalities. No ledger or lock was written.
- Isolated publisher scenarios exercised body-size boundaries, immutable source references, fenced examples, edited-body/closed-issue preservation, interrupted initial-to-final body recovery, manual progress preservation, native numeric relationship direction, and conflicting relationship rejection. These were isolated scenarios, not live GitHub issue publication.
- Both repository checkers reject broken same-file heading links while ignoring inline-code link examples. Missing ticket IDs and actual mixed-case coauthor trailers fail; ordinary policy prose is accepted.
- Library art/package/catalog inputs fail closed even if prerequisite-shaped files exist. Real pinned-engine builds, integrity/rights validation, previews, and catalog/signing gates remain blocked on implementation, not represented by skipped-green workflows.
- Bandit scanned the actual application-repository Python tooling without findings. The installed workflow dependency audit found zero known vulnerabilities after the pinned pip bootstrap was upgraded; the license inventory was emitted.

### Historical GitHub CI evidence (2026-11-11 baseline)

| PR / head | Observed passing checks |
|---|---|
| [Application PR #2](https://github.com/Astraive/asciiwall/pull/2), `c789960` | [Repository quality](https://github.com/Astraive/asciiwall/actions/runs/43046714547/job/130612476996), [Python security analysis](https://github.com/Astraive/asciiwall/actions/runs/43046714634/job/130612476859) |
| [Library PR #1](https://github.com/Astraive/asciiwall-library/pull/1), `916ed70` | [Repository quality](https://github.com/Astraive/asciiwall-library/actions/runs/43036768898/job/130577522630), [Python security analysis](https://github.com/Astraive/asciiwall-library/actions/runs/43036768843/job/130577522843), [CodeQL](https://github.com/Astraive/asciiwall-library/runs/130577938554) |

The original documentation [PR #1](https://github.com/Astraive/asciiwall/pull/1) was merged before this setup. Navigation repair `c321475` is carried by application PR #2. Setup commits are authored as **Neeraj Sathish Kumar <neerajcodz@gmail.com>**, with no coauthor trailers; future contributors retain their own identities.

No Cargo workspace, native desktop wallpaper host, package assets, binary release, or signing workflow exists yet. Native platform, GPU/media, package/export, accessibility, fuzz/performance, clean-machine/offline, and release-signing acceptance have **not** been exercised. The source specifications and local milestone gates retain those requirements explicitly.
