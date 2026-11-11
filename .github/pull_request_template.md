<!--
Pull request template for Astraive/asciiwall.
Rules: CONTRIBUTING.md and docs/issue-conventions.md (https://github.com/Astraive/asciiwall/blob/main/docs/issue-conventions.md).
PR title MUST match: type(scope): subject [TICKET-ID]
  Allowed types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert
  Scope: lowercase kebab-case (e.g. docs, engine, platform, ci, repo)
  Ticket ID: uppercase prefix and suffix (e.g. [PRJ-004], [REL-009], [ENG-001], [DOC-001])
PRs are squash-merged into main, so the PR title becomes the commit subject.
No Co-authored-by trailers are permitted in commit messages or PR descriptions.
Dependabot PRs have no CI title bypass: maintainers triage and rename automatic dependency PR titles to append [REL-009] (or a dedicated ticket ID) before merge, re-triggering the check on `edited`.
Bootstrap documentation PR #1 (3801d37) and navigation repair (c321475) preceded these checks and remain in history as-is.
Specialized templates are also available via ?template=docs.md and ?template=spike.md.
-->

**Ticket:** <!-- Stable ticket ID (e.g. PRJ-004 or REL-009) and `Closes #N` / `Refs #N` once published on GitHub. Do not invent `#N` numbers prior to issue publication. -->

## What and why

<!-- Summarize what changes, why it is needed now, and which `docs/` specifications or planning contracts it implements. -->

## Platforms affected

- [ ] All / Cross-platform shared
- [ ] Windows
- [ ] macOS
- [ ] Linux (shared)
- [ ] X11
- [ ] Wayland
- [ ] KDE Plasma
- [ ] GNOME
- [ ] CLI / Headless
- [ ] Documentation, tooling, or CI only

## Acceptance criteria

<!-- Copy the ticket's acceptance criteria and check each one with observed evidence (test name, command output, or verified environment tuple). Leave unfinished items unchecked and name the follow-up ticket. -->

- [ ]

## Test evidence

<!-- Record the exact commands you ran and their observed results (e.g. `python scripts/check-repository.py`, `pwsh -NoProfile -File scripts/check-powershell.ps1`, `python scripts/check-conventions.py --title "..."`). For native host or Rust implementation changes once present, record the exact OS build, session/compositor, GPU/driver, and manual verification steps. Never claim unrun checks passed. -->

## Checklist

- [ ] Only actually executed checks and verified platform tuples are claimed in Test evidence.
- [ ] Offline core invariant, permission separation, and data-only `.asciiwall` v1 package boundaries remain intact.
- [ ] Affected documentation (`docs/`), schemas, or fixtures are updated in this PR.
- [ ] No secrets, tokens, personal media, proprietary fonts, unredacted local paths, or external `.issues/` backlog files/reports are included.
- [ ] No `Co-authored-by` trailers appear in commit messages or this PR body.
- [ ] Any added third-party dependencies, fonts, codecs, or test fixtures include verified licenses and notices ([licensing policy](https://github.com/Astraive/asciiwall/blob/main/docs/library/licensing-and-attribution.md)).
