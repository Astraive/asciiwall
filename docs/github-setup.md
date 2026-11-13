# GitHub Repository and Project Setup

This repository keeps a **static configuration definition** in `.github/project-management/`. The versioned files contain safe label definitions, milestone goals and gate summaries, project field definitions, and project view definitions. They do not contain issue bodies, `issue.json`, coverage/export data, per-issue work items, ticket membership, or derived ticket-ID lists. The private expanded planning model remains external in `../.issues`; the existing publisher continues to read that model for its separate issue-publication workflow. This configuration tool does not invoke the publisher and does not create or update issues, project items, or drafts.

The registries are definitions, not a shadow planning model or a snapshot of project items. Milestone summaries intentionally omit the full ticket-membership lists. `path:critical` is a static derived-label rule/meaning, not a committed list of its member tickets. No calendar dates, completed work, native issue relationships, or platform-support claims are inferred by these files.

Observed remote settings, CI evidence, local inventory breakdowns, full-backlog publication state, and API-vs-UI view verification status are recorded in the [setup audit](setup-audit.md). The audit separates API-verifiable view configuration (`groupByFields`, `verticalGroupByFields`, `sortByFields`, layouts, filters, and visible fields) from UI-only `Slice by` sidebar controls (`slice_by`) that the public GraphQL API cannot read or mutate.

## Safe configuration model

`scripts/configure-github.ps1` is a PowerShell 7 tool limited to repository settings, topics, security feature settings, labels, milestones, branch protection, and the Project V2 title, repository links, fields, and views. It never calls the issue publisher. Its GitHub mutations are limited to these infrastructure APIs; it does not change repository visibility, names, commit history, issue metadata, issue labels, project items/drafts, or native issue relationships. It preserves unrelated label/topic objects and existing project select options wherever changes are made.

The repository remains public and MIT-licensed. The script refuses to proceed if the repository is not public. Security feature settings are requested as enabled; GitHub may reject features unavailable for the repository/plan or insufficiently authorized. Each mutation is followed by a refetch where the API supports readback, and any unsuccessful readback is recorded as a failure rather than reported as success. Mutations are paced, reads are bounded by the configured command timeout, and mutation calls are not retried blindly. A failed or interrupted apply may leave some infrastructure changes applied; inspect the JSON audit report and run verification before deciding what to do next.

## Offline plan and audit report

From the repository root, an ordinary invocation is offline: it reads only the checked-in static registries and proposes operations without contacting GitHub or writing a report inside the repository.

```powershell
pwsh -NoProfile -File scripts/configure-github.ps1
```

The default audit report is `../.issues/reports/github-config-audit.json`, outside the repository. Use `-ReportPath` to choose another location. The report records operations, intended values, pending manual configuration, and failures; it does not contain issue content or ticket membership.

## Authorized apply

Apply is an explicit, separate action and requires an already authenticated GitHub CLI (`gh auth status`):

```powershell
pwsh -NoProfile -File scripts/configure-github.ps1 -Apply
```

Apply only from an authorized maintainer session after reviewing the offline plan. It requests the repository description/topics and merge defaults (squash merge, delete merged branches; no required sign-off), enables discussions and available security protections, restricts Actions workflow tokens to read-only permissions, upserts the 78 canonical labels in both repositories and the 16 milestone summaries in their designated repositories, renames and links the existing shared project, and upserts project fields/views. It does not alter project visibility. Do not add credentials to the browser or change the private project to public to make the API work. Once issues and custom milestone descriptions or manual UI view layouts are in place, avoid re-running full `-Apply` unless an infrastructure reset is explicitly intended, because full `-Apply` overwrites native milestone descriptions with static summaries; use read-only `-VerifyOnly` for ongoing infrastructure audits.

Branch protection is intentionally excluded unless the maintainer supplies the exact names of checks that the parent has observed green on the actual CI workflows:

```powershell
& ./scripts/configure-github.ps1 -Apply -ConfigureBranchProtection `
  -RequiredCheck @('Repository quality', 'Python security analysis')
```

The names above are the workflow job contexts defined in these repositories; run this command from PowerShell only after observing both green on the actual PR heads. Protection applies to both `main` branches and preserves additional existing required checks and push restrictions. It requires strict up-to-date checks, administrator enforcement, zero approvals for single-maintainer bootstrap, dismissal of stale approvals, no force pushes/deletions, linear history, and resolved conversations. Signing and code-owner approval remain unenforced until their prerequisites exist.

## Verification and known limitations

`-VerifyOnly` performs read-only API calls, writes an external audit report, and exits nonzero if any requested state differs, any required native field is absent, or a manual-only setting remains pending. Do not combine it with `-Apply`.

```powershell
pwsh -NoProfile -File scripts/configure-github.ps1 -VerifyOnly
```

The Project V2 GraphQL API supports `name`, `layout`, `filter`, and `configuration: { visibleFieldIds }` on `createProjectV2View` and `updateProjectV2View`, but exposes no public setters for grouping, sorting, or `Slice by`. New views are created with name/layout/visible fields, then receive their filter in a separate update mutation before independent refetch verification; existing views are updated directly. For readback, `ProjectV2View` exposes `groupByFields`, `verticalGroupByFields`, and ordered `sortByFields` (with direction), **not** `Slice by` (`slice_by` has no public GraphQL read or write field):

- **Board orientation (`BOARD_LAYOUT`, `Master Roadmap`):** Native board columns come from `verticalGroupByFields` (`group_by`: `Phase`) and swimlanes come from `groupByFields` (`secondary_group_by`: `Status`), with `slice_by: null` and ordered `sort_by` (`["Priority", "Milestone"]`).
- **Table orientation (`TABLE_LAYOUT`, 17 table views):** GitHub Projects tables allow one grouping field (`groupByFields` mapped to `group_by`), with `secondary_group_by: null`. Each table view's approved secondary dimension lives in the UI **Slice by** sidebar (`slice_by`), alongside two ordered `sort_by` fields (`sortByFields`).

In `-VerifyOnly` mode, `configure-github.ps1` normalizes those layout-specific API vectors separately rather than merging `groupByFields` and `verticalGroupByFields`:

- `view-api:<view name>` compares normalized `{ primary_group_by, secondary_group_by, sort_by }` against the live layout-specific fields (`BOARD_LAYOUT`: `primary_group_by = verticalGroupByFields`, `secondary_group_by = groupByFields`; `TABLE_LAYOUT`: `primary_group_by = groupByFields`, `secondary_group_by = null`) and exact ordered `sortByFields`, marking `verified` only on exact match and `pending` otherwise.
- `view-slice:<view name>` records `slice_by` (`{ slice_by: <field> }` on table views, or `null` when absent) with state `manual-only-unverifiable-by-api` (`observed: null`). `-VerifyOnly` still exits nonzero when any operation is not `verified` (including `manual-only-unverifiable-by-api`), explicitly reporting that manual-only `Slice by` verification cannot be proven via the public API rather than misclassifying it as pending configuration.

The Milestone Roadmap stays a table because approved schedule dates are absent. Existing select-field options are merged with the desired options while preserving their IDs, colors, and descriptions. If the shared project has zero items, `Todo` is renamed to `Backlog` while retaining that option ID; when items exist, legacy `Todo` is retained and migration is recorded without rewriting item values. Platform is configured as a real Project V2 multi-select field. Project field types, options, security controls, and repository settings that GitHub or the authenticated account rejects are reported as failed/pending, not successful.

Native Repository, Milestone, Assignees, Title, Labels, Parent issue, Sub-issues progress, and Updated remain native; no custom shadows are created. This infrastructure tool does not publish issue relationships. Explicitly authorized milestone batches use the separate [issue publisher](backlog-publication.md), whose external ledger records native parent/blocker and project-item verification.

GitHub rejects a custom field named `Type` because it is reserved for native issue types. Detailed classification therefore uses `Work type` with all 14 planned values, mapped from the unchanged `issue.json.type`; native issue types remain Task, Bug, or Feature. The external publication registry, static definition, views, and publisher use this same field name.

## Project view grouping, sorting, and Slice by

The 18 Project 4 views are configured and saved through the authorized Orca embedded browser. Each view was reloaded; native grouping axes and ordered sorts match GraphQL readback, and all 17 table `slice_by` values were observed in their reloaded UI. Per-view receipts and screenshots are stored externally as described below.

- **`Master Roadmap` (`BOARD_LAYOUT`):** Columns use `group_by` (`Phase`, `verticalGroupByFields`); Swimlanes use `secondary_group_by` (`Status`, `groupByFields`); `Slice by` is unset; ordered sorts are `Priority`, then `Milestone`.
- **17 `TABLE_LAYOUT` views:** Group by uses the single `group_by`; `secondary_group_by` is `null`; the approved secondary dimension is the UI-only `slice_by`; and the two `sort_by` fields are applied in exact order.

`configure-github.ps1 -VerifyOnly` checks API-readable layout grouping and sort order through `view-api:<view>`. It must keep table `view-slice:<view>` as `manual-only-unverifiable-by-api`; GraphQL cannot prove `Slice by`. The saved/reloaded UI receipts and screenshots are in `../.issues/reports/project-view-ui-verification.json` and `../.issues/reports/project-view-ui-verification/`.
