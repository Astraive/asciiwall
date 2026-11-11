# Backlog publication

The publication source is the **external workspace** `../.issues`, relative to the main repository. It contains both repositories' ticket folders, `labels.json`, `milestones.json`, `project.json`, and coverage registers. It is not application code and must not be moved back into either repository, committed, or pushed. Source `issue.json` files remain local authoring records: the publisher never writes GitHub numbers, URLs, project IDs, or publication status into them.

`scripts/publish-issues.ps1` is a PowerShell 7 publisher for the existing `Astraive/asciiwall`, `Astraive/asciiwall-library`, and organization Project 4. Publication creates planning records, **not implementations or passing gates**. Bodies and project Status retain the source Backlog/unimplemented state. Checkbox completion, acceptance criteria, test claims, assignees, and issue open/closed state are not advanced by publication.

The publisher was initially authored without live issue writes. Live publication now requires explicit authorization for its requested scope; the external publication ledger, not this guide or the local authoring inventory, records actual remote results.

## Modes and prerequisites

Use PowerShell 7, Python 3 compatible with `scripts/validate-backlog.py`, and GitHub CLI `gh`. The Python validator is run before GitHub access, lock creation, or ledger writes, with exactly:

```text
python scripts/validate-backlog.py --backlog-root <external-backlog> --source-root <main-repository>
```

No report-writing flag is passed. The publisher additionally validates registries, every ticket's required labels/milestone/project values, select limits, native-field ownership, `project.derived_labels` (array field types, registered `labels.json` names, non-empty `source`/`meaning`, and unique existing `asciiwall`/`asciiwall-library` ticket IDs with no foreign repositories), local Markdown link destinations, and the combined containment/dependency DAG. The registry must include Kind and its exact `kind:*` labels; Area has at most 50 canonical values, Phase at most 16, and nonselect fields have no select options.

| Mode | GitHub access | Local publication writes | Remote writes |
| --- | --- | --- | --- |
| No switch / `-DryRun` | None; fully offline, including no `gh` invocation | None, including no ledger or lock | None |
| `-VerifyOnly` | Paginated REST/GraphQL reads and schema introspection | None; an existing ledger is read only | None; reports differences read-only and does not repair |
| `-Apply` | Reads and explicit, journaled writes | External lock and atomic external ledger only | Resumes managed publication; refuses manual body or progress drift |

Mode switches are mutually exclusive. Defaults are owner `Astraive`, project number `4`, documentation revision `main` for a fresh remote ledger, and sibling `../.issues`. `-BacklogPath` can select another external directory, but never a location inside either repository. Symlink/reparse-point backlog paths, ancestors, and descendants are rejected. `-SourceRef` selects the main repository's documentation branch, tag, or commit; remote modes resolve it before mutating and use the resolved commit SHA in every published documentation URL. Apply records both the requested ref and resolved SHA in the external ledger. Later Apply/VerifyOnly runs without `-SourceRef` use that ledger-pinned ref and reject a ref that now resolves to a different SHA; a conflicting explicit ref is rejected rather than silently changing published bodies. Review and deliberately update the external ledger only as part of a reviewed source-revision change. DryRun stays offline and reports the supplied ref, or `main` when omitted. The local validator checks the checkout's source documents, so the operator must ensure that checkout corresponds to the chosen published revision.

Future offline planning from the main repository:

```powershell
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -DryRun
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -BacklogPath ../.issues -DryRun -SourceRef main
```

Standard output is a JSON plan with creation order, source/body digests, declared `derived_labels`, desired labels, milestone keys, hard blockers, project values, and phases. The plan intentionally has no invented issue numbers or URLs: real publication identities can only be discovered remotely. It reports the requested documentation ref but does not resolve it or contact GitHub. The preliminary body digest describes the creation body, not the final identity-linked body.

### Publish only an authorized milestone

`-Milestone` takes qualified registry keys, such as `asciiwall/M0`. The complete source model is still validated, but only matching tickets and their required roadmap/epic ancestors are reconciled. An ancestor keeps its original milestone: it is a tracking container, not authorization to publish its later implementation tasks. Missing hard dependencies fail before any remote access; explicitly include their milestones rather than silently publishing future work. Multiple keys can be passed as a PowerShell array.

```powershell
pwsh -NoProfile -File scripts/publish-issues.ps1 -DryRun -Milestone asciiwall/M0
pwsh -NoProfile -File scripts/publish-issues.ps1 -Apply -Milestone asciiwall/M0 -ExpectedLogin NeerajCodz
pwsh -NoProfile -File scripts/publish-issues.ps1 -VerifyOnly -Milestone asciiwall/M0 -ExpectedLogin NeerajCodz
```

The current M0 selection is **37 milestone members plus CFG-EPIC and WND-EPIC**, two required parent containers retaining M5 and M3 respectively. No later implementation task or library issue is selected. Links to unpublished, out-of-scope tickets become explicitly labeled local-backlog references rather than invented URLs; in-scope links use real issue identities.

`-ExpectedLogin` checks the authenticated account before lock/ledger creation and any mutation, and verifies each selected issue's author. It does not assign issues to that account. Scoped verification records its exact ticket keys and leaves unrelated ticket/operation verification records untouched. Only requested milestone descriptions are managed; an ancestor's other milestone must already exist and is read without modification.

Omitting `-Milestone` retains the full-backlog behavior. **An unscoped Apply requires authorization for all tickets**, not just the first milestone. Dry-run remains fully offline even when `-ExpectedLogin` is supplied.

`-GhCommand` and `-PythonCommand` accept **executable paths**, not shell expressions, PowerShell functions, argument strings, or `.ps1` scripts. These seams support a parent-owned throwaway fake-gh boundary smoke. Processes use `ProcessStartInfo.ArgumentList`, no shell, UTF-8 streams, and JSON on stdin (`gh api --input -`). A fake gh executable must implement `gh api` arguments and emit the same `--include` HTTP-header-plus-JSON envelope as gh; a fake Python executable receives the validator script path and the exact root arguments. Neither seam changes the mode's safety rules. Do not use a fake validator as evidence that the actual backlog validates.

## Authorization and native API requirements

Before a future Apply, the operator must authenticate gh with access to **both** repositories and the organization project. The principal needs issue creation/update, labels, milestones, native issue types, native cross-repository parent/sub-issue and hard-blocker operations, project repository links, fields/options, real issue items, and item-field writes. Fine-grained tokens need the corresponding repository Issues and organization Projects permissions; organization policies and cross-repository issue relationships may impose additional access requirements. Credentials belong in the operator's gh credential store/environment, never in these files or the ledger.

Remote modes introspect the live schema. Native Platform multi-select support must include `ProjectV2MultiSelectField`, `multiSelectOptions`, and `ProjectV2FieldValue.multiSelectOptionIds`. Updating select options must accept existing option IDs. Missing or incompatible native APIs produce a nonzero failure, **not** text fields, draft items, label-only representations, or silently skipped phases. Existing conflicting native fields, parents, blockers, duplicate matches, or option-limit conflicts also require operator resolution; the publisher never destroys unrelated data to make verification pass.

Future read-only remote audit:

```powershell
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -BacklogPath ../.issues -VerifyOnly
```

The explicit mutation example below targets only the authorized first milestone. Its outcome must be checked in the external ledger and by a subsequent read-only audit:

```powershell
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -BacklogPath ../.issues -Apply -Milestone asciiwall/M0 -ExpectedLogin NeerajCodz
```

Do not run concurrent Apply processes or edit the source backlog while publishing. Read-only verification does not acquire an Apply lock, so do not interpret an audit racing an Apply as a stable snapshot.

## Apply phases

1. **Validate and prepare.** Validate the external source and registries, reject initial or final rendered bodies over GitHub's 65,536-character issue-body limit (including a conservative final-link expansion estimate for every ticket), obtain an exclusive external file lock, load the ledger, inspect the live GraphQL schema, and confirm the documentation revision. Mutation intent is durable before each remote write.
2. **Upsert real registries.** Upsert every source label in both repositories, preserving unrelated labels. Resolve milestones by exact repository/title across open and closed milestones, reject ambiguous matches, and update only managed title/description. Existing due dates and open/closed milestone state are never replaced. No release dates are invented.
3. **Reconcile the existing project.** Use organization Project 4 rather than creating another project. Reconcile its desired title and link both repositories. Reuse native Repository, Milestone, and Assignees; never shadow them. Accept declared native visible fields by their native ProjectV2 data type, while requiring those three named native fields with their exact native kinds. Read existing iteration fields as well as other field types so unrelated project fields survive. Create missing custom fields with the registry's actual data types. Merge missing select options without deleting unrelated options or changing existing IDs, colors, or descriptions. If preserving existing options would exceed 50, stop for a non-destructive manual migration.
4. **Discover and create issues.** Exhaustively paginate both repositories' open and closed issues, excluding pull requests. Match the stable HTML marker, an exact title plus explicit ticket/repository metadata, or an already-recorded exact ledger identity. Multiple candidates or conflicting markers are errors. Create only the selected master/roadmap, epic, task, and gate tickets in topological containment/dependency order. Initial bodies retain requirements and source links; references without a published identity remain plain text, never fabricated URLs.
5. **Render final metadata and bodies.** Once every selected identity is known, render in-scope ticket links to real GitHub issue URLs and leave unpublished out-of-scope references explicitly local. Source documentation targets `Astraive/asciiwall/blob/<resolved-commit-SHA>/docs/...`; coverage registers remain external-backlog references. Preserve detailed requirements, unchecked criteria, and unimplemented status. Refuse to overwrite closed-issue planning metadata or remote bodies outside known managed hashes (prior desired/verified body, journaled initial create body, current rendered body, or raw template). Preserve unrelated labels and assignees. Native type maps to Task/Bug/Feature; exact detailed classification remains in `type:*` and **Work type**.
6. **Reconcile native relationships.** Read each immediate native parent. Add a missing parent with `POST /repos/{owner}/{parentRepo}/issues/{parentNumber}/sub_issues` and `sub_issue_id` equal to the child's real numeric REST ID. Never set `replace_parent`, and never replace or remove another parent. Add hard blockers on the **dependent** issue's `dependencies/blocked_by` endpoint with `issue_id` equal to the blocker's real numeric REST ID. Soft `related` links remain body links, not hard blockers. Unexpected existing blockers are preserved and reported as conflicts, not deleted.
7. **Reconcile actual project items.** Add open issues by their real GraphQL issue node IDs, never as drafts; refuse adding closed issues or resetting a closed issue's `Status` to Backlog. Check project `Status` before writing any other item fields: for a previously verified item `Status`, if the current remote `Status` differs from both the recorded verified `Status` and the new desired `Status`, stop rather than rolling back manual progress (such as `In Progress` or `Done`). Initial unconfigured/unset or default `Todo` project items still reconcile to `Backlog` during legitimate first publication or unverified resume, while unverified items already in non-default states are preserved rather than rolled back. Reconcile every registry custom field: Kind, Status, Priority, Size, detailed Type, canonical Area, all Platform selections, Repo role where present, canonical Phase, Blocked, Target release, and Effort. Blocked is Yes exactly when a declared hard blocker remains open; it is independent of workflow Status. Target release comes from the repository-specific milestone registry. Native Repository/Milestone/Assignees flow from the issue and are independently checked.
8. **Independently refetch everything.** Refetch all registry state, project configuration/repository links, issue identities, individual issue metadata, native parents/blockers, actual project items, and all item fields. Verify titles, exact normalized final bodies and SHA-256, managed and preserved submitted labels, native type, exact milestone, parent/blocker IDs, item content, native fields, custom values/options, and source Status. Mutation responses are not treated as proof of success. A permission-dependent silent drop fails this audit. Exit zero and a `verified` JSON result require the entire set to match.
Rendered initial and final issue bodies are each checked against the 65,536-character API limit. Before any remote mutation, the publisher also sizes a conservative final-render estimate for every ticket using temporary synthetic issue-number lengths; those values are internal sizing inputs only and are never emitted or published. Oversized requirements are rejected, never truncated.
Only supported local Markdown destinations are rewritten: repository documentation, external coverage-register references, and ticket links (including optional titles and same-ticket anchors). Links inside fenced code examples are not rewritten; any other local link fails preflight rather than being published as a broken href.

The project registry may also describe preferred views/groupings. The publisher's owned project changes are the existing project's title, repository links, fields/options, real items, and values; it does not delete or rewrite user views or grouping preferences.

## Derived labels and withheld publication

`project.json` may declare `derived_labels` as an array of `{ name, ticket_ids, source, meaning }` objects (for example `path:critical`, which the canonical `P0 Critical Path` view filter uses alongside `priority:P0`). Desired issue labels include a declared derived label solely when the ticket's `id` appears in that entry's `ticket_ids`, without adding computed-label fields to source `issue.json` files or deriving dependency closures on the fly. Both reconciliation and independent verification treat only the declared `derived_labels` names as managed alongside the six taxonomy families, leaving unrelated user `path:*` labels intact.

For `path:critical`, `source` records the **planned critical chain, not an actual delivery date** (an effort-weighted longest hard-dependency chain computed from estimated person-days). Project views and label definitions are infrastructure separate from issue publication. A scoped publication applies derived labels only to selected issues; remaining tickets stay local until separately authorized.

## External ledger, locking, and recovery

The ledger is `../.issues/publication-ledger.json`; its lock is `../.issues/publication-ledger.lock`. Both stay outside version control. Keep the ledger with the external workspace and back it up as operational state. Never copy its IDs into source `issue.json`, and never discard it merely to force recreation.

Apply opens the lock with exclusive file sharing. The lock file may remain after exit; **the held file handle**, not file existence, determines ownership. Atomic ledger updates write a same-directory unique temporary file, flush it to disk, then overwrite the destination with a same-directory rename. Process failure leaves a complete old or new ledger, not partially rewritten JSON. A hard crash can leave an unreferenced `.tmp` file; never mistake it for a committed ledger.

The ledger stores exact numeric issue IDs, GraphQL node IDs, issue numbers/URLs, stable markers and source hashes; desired issue metadata/body hash; label/milestone identities and states; native parent/blocker identities; project identity, option configurations, issue-item IDs and desired field payloads; independently observed verification results; and exact operation intent/outcome/failure details. Intent is persisted **before** remote mutation. If ledger persistence or process completion fails after the request, the operation remains uncertain rather than being assumed absent.

Mutation requests are **never automatically retried**. This includes creation, updates, relationship additions, and GraphQL mutations. Safe reads alone have bounded retries (`-ReadAttempts`, default 4) for transport/rate-limit/transient service failures. Retry-After and rate-reset headers are honored up to a 60-second wait bound; a longer wait stops cleanly for later resumption. Each child process has a bounded `-CommandTimeoutSeconds` (default 180). A timed-out mutation may already have reached GitHub.

Remote mutations are paced by at least 1.05 seconds per process; pacing does not authorize retries or bypass GitHub rate limits.

After interruption, run the normal future Apply command again. It refetches remote state first for each phase: a marker/metadata-matched created issue is adopted, an initial-body `create` payload or prior `desired.body_sha256` lets interrupted finalization resume cleanly, and an already-observed desired mutation result reconciles the stored intent. If remote state shows manual body edits, a closed issue needing Backlog metadata changes, or manual project `Status` drift, Apply stops cleanly and refuses to overwrite or roll back that work; there is no force flag to bypass these preservation guards. If an uncertain operation is still not proven applied, publication stops and prints its exact operation key, HTTP method/endpoint, and JSON payload. It never assumes that a temporarily absent issue or item makes another creation safe.

Only after independently establishing that the exact pending request **did not apply**, an operator may explicitly authorize one attempt for that operation key:

```powershell
# Future recovery example; not executed. Use the exact key printed by the publisher.
pwsh -NoProfile -File ./scripts/publish-issues.ps1 -BacklogPath ../.issues -Apply -RetryOperation 'create:asciiwall/PRJ-001'
```

`-RetryOperation` is valid only with Apply. It is not a blanket retry policy, and it does not bypass identity ambiguity, native conflicts, body/closed-issue/status preservation guards, registry validation, or final verification. Never authorize a creation retry solely because an immediately preceding eventually-consistent inventory did not yet show the issue. Preserve the ledger and resolve the uncertainty or permissions first. Rerun `-VerifyOnly` after remote changes made outside this workflow; it inspects and reports all differences (including manual body edits and project `Status` progress drift) read-only, performs no repair, and never writes verification results into the ledger.

Failures are nonzero. Exact unverified journaled requests and their payloads are printed to standard error and remain durable in the ledger. Verification disagreements are reported as discrepancies, not converted into implementation success or textual fallback relationships.

## Ledger persistence regression

`pwsh -NoProfile -File scripts/test-publisher-ledger.ps1` exercises actual ledger creation and repeated replacement on isolated temporary files, without GitHub access. It verifies retained issue identities, exact Unicode mutation payloads, and verification/receipt transitions. The repository CI runs it alongside syntax and documentation checks. The overwrite rename avoids PowerShell's null-to-empty-string binding for the optional backup path in `File.Replace`, which otherwise prevents updates to an existing ledger on the verified Windows runtime.
