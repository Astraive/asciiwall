# Repository and project setup audit

Snapshot: **2026-11-11**, after live infrastructure configuration and before the foundation PR merges. This records observed setup state, not application readiness. Subsequent audit-only commits must pass the same protected-branch checks before merge.

## Scope and publication hold

- Application: [`Astraive/asciiwall`](https://github.com/Astraive/asciiwall).
- Community library: [`Astraive/asciiwall-library`](https://github.com/Astraive/asciiwall-library).
- Shared private project: [AsciiWall — Master Development Roadmap](https://github.com/orgs/Astraive/projects/4), `PVT_kwDOEF_JvM4BrI0s`.
- Both repositories remain public with their original MIT licenses and history preserved. Project visibility remains private.
- **Zero GitHub issues and zero project items/drafts were published.** The issue publisher's live `-Apply` mode was not run. No publication ledger or lock was created.
- Detailed tickets, coverage, publication state, and generated reports remain in the workspace sibling `../.issues`, outside both Git repositories. Only static configuration definitions and summary documentation are version controlled.

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

## Observed remote infrastructure

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
- All **18 views** have their declared names, supported layouts, filters, and visible-field IDs configured and re-fetched. The original View 1 ID was reused for Master Roadmap. Milestone Roadmap uses the documented table fallback because no schedule dates were approved.

### Exact pending UI operation

**Primary grouping, secondary grouping, and sorting for each of the 18 views remain pending.** The public schema exposes those settings for reading but provides no mutation setters; view configuration input supports visible-field IDs only. The private project was inaccessible in the signed-out browser. The harness browser relay was unavailable, with no adoptable authenticated fallback. No credentials were injected and privacy was not relaxed.

1. Sign in through an authorized browser session and open [Project 4](https://github.com/orgs/Astraive/projects/4).
2. For each named entry in the [static view registry](../.github/project-management/project.json), apply its `group_by`, `secondary_group_by`, and ordered `sort_by` fields using the UI. Save each view and visually inspect it. Record any UI limitation instead of substituting a different configuration silently.
3. Run the read-only audit below. It deliberately exits nonzero while any requested setting differs or remains manual-only; it never edits issues or items.

```powershell
pwsh -NoProfile -File scripts/configure-github.ps1 -VerifyOnly
```

Filters were re-fetched without inserting trial issues. Native parent/sub-issue, blocker, and project-item population remain withheld with issue publication; they are implemented by the separate publisher, not claimed as existing remote relationships.

## Exercised verification

- Actual Windows Git-for-Windows `pre-commit` and `commit-msg` hooks passed. Both checkouts opted into local hooks explicitly; author/global configuration was not rewritten. An isolated existing-custom-hook scenario was rejected without changing that hook path.
- Local repository checks passed; all workflow/template YAML parsed, and four application/two library PowerShell files parsed without execution.
- The complete **356-object offline issue-publication plan** passed with a deliberately nonexistent `gh` executable, including after the Work type mapping cutover. All parent/blocker creation precedences were checked; project Platform values retained zero/one/many cardinalities. No ledger or lock was written.
- Isolated publisher scenarios exercised body-size boundaries, immutable source references, fenced examples, edited-body/closed-issue preservation, interrupted initial-to-final body recovery, manual progress preservation, native numeric relationship direction, and conflicting relationship rejection. These were isolated scenarios, not live GitHub issue publication.
- Both repository checkers reject broken same-file heading links while ignoring inline-code link examples. Missing ticket IDs and actual mixed-case coauthor trailers fail; ordinary policy prose is accepted.
- Library art/package/catalog inputs fail closed even if prerequisite-shaped files exist. Real pinned-engine builds, integrity/rights validation, previews, and catalog/signing gates remain blocked on implementation, not represented by skipped-green workflows.
- Bandit scanned the actual application-repository Python tooling without findings. The installed workflow dependency audit found zero known vulnerabilities after the pinned pip bootstrap was upgraded; the license inventory was emitted.

### GitHub CI evidence at this snapshot

| PR / head | Observed passing checks |
|---|---|
| [Application PR #2](https://github.com/Astraive/asciiwall/pull/2), `c789960` | [Repository quality](https://github.com/Astraive/asciiwall/actions/runs/43046714547/job/130612476996), [Python security analysis](https://github.com/Astraive/asciiwall/actions/runs/43046714634/job/130612476859) |
| [Library PR #1](https://github.com/Astraive/asciiwall-library/pull/1), `916ed70` | [Repository quality](https://github.com/Astraive/asciiwall-library/actions/runs/43036768898/job/130577522630), [Python security analysis](https://github.com/Astraive/asciiwall-library/actions/runs/43036768843/job/130577522843), [CodeQL](https://github.com/Astraive/asciiwall-library/runs/130577938554) |

The original documentation [PR #1](https://github.com/Astraive/asciiwall/pull/1) was merged before this setup. Navigation repair `c321475` is carried by application PR #2. Setup commits are authored as **Neeraj Sathish Kumar <neerajcodz@gmail.com>**, with no coauthor trailers; future contributors retain their own identities.

No Cargo workspace, native desktop wallpaper host, package assets, binary release, or signing workflow exists yet. Native platform, GPU/media, package/export, accessibility, fuzz/performance, clean-machine/offline, and release-signing acceptance have **not** been exercised. The source specifications and local milestone gates retain those requirements explicitly.
