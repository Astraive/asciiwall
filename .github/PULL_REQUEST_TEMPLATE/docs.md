<!--
Documentation PR template.
PR title format: type(scope): subject [TICKET-ID], for example "docs(delivery): clarify M0 spike criteria [DOC-001]".
No Co-authored-by trailers are permitted anywhere in commit messages or squash-merge descriptions.
-->

**Ticket:** <!-- Stable ticket ID (e.g. DOC-001) and `Refs #N` / `Closes #N` once published; omit `#N` if unpublished. -->

## What and why

<!-- Which documentation files changed, why, and which design/infrastructure gaps they close. -->

## Documentation scope

- [ ] Architecture or design specifications (`docs/`)
- [ ] Planning entrypoints (`docs/plan.md`, `docs/roadmap.md`, etc.)
- [ ] Root policy or governance files (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`, etc.)
- [ ] CI / repository setup guides (`docs/github-setup.md`, `docs/backlog-*.md`)

## Acceptance criteria

- [ ] Links and heading fragments resolve cleanly inside the repository (`python scripts/check-repository.py`).
- [ ] Status claims remain truthful (draft formats, unimplemented Rust workspace, withheld remote publication, and unverified native hosts are never described as complete).
- [ ] No external `.issues/` backlog records, coverage registers, ledgers, or generated reports are committed or linked as GitHub URLs.

## Test evidence

<!-- Exact checks run (e.g. `python scripts/check-repository.py`) and observed output. -->

## Checklist

- [ ] No secrets, tokens, personal data, or external `.issues/` artifacts included.
- [ ] No `Co-authored-by` trailers in commits or PR description.
- [ ] Navigation indexes or cross-references affected by this change are updated in the same PR.
