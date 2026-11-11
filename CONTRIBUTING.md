# Contributing to AsciiWall

Thank you for contributing to AsciiWall (`Astraive/asciiwall`). Before writing code or documentation, read [docs/start-here.md](docs/start-here.md), [docs/delivery/contributor-workflow.md](docs/delivery/contributor-workflow.md), [docs/development.md](docs/development.md), and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Repository Stage and Scope

AsciiWall is currently in its **design, backlog-tooling, and repository-infrastructure baseline** (pre-implementation).

- Do **not** create empty Rust crate skeletons, placeholder `Cargo.toml` manifests, or no-op workflow stubs solely to mirror the target directory layout in [docs/architecture/repository-structure.md](docs/architecture/repository-structure.md) ([docs/delivery/contributor-workflow.md#getting-oriented](docs/delivery/contributor-workflow.md#getting-oriented)).
- Every change must truthfully state what was verified versus what remains unimplemented.
- Community wallpaper recipes, artwork metadata, and catalog submissions belong in the companion repository [`Astraive/asciiwall-library`](https://github.com/Astraive/asciiwall-library), not inside `Astraive/asciiwall`.

## Prerequisites and Local Verification

Current repository checks use Python 3.12+ (standard library only), PowerShell 7 (`pwsh`), and Git:

```sh
python scripts/check-repository.py
python scripts/check-conventions.py --title "docs(repo): clarify policy links [DOC-001]"
pwsh -NoProfile -File scripts/check-powershell.ps1
```

Wait for actual Rust/cargo tooling to exist in later milestones before running `cargo` workflows; do not invent passing Rust or native test claims before those prerequisites exist ([docs/quality-gates.md](docs/quality-gates.md)).

## Opt-In Local Git Hooks

To catch broken links, PowerShell syntax issues, or non-compliant commit messages locally before pushing, install the opt-in repository hooks ([.githooks/README.md](.githooks/README.md)):

```powershell
pwsh -NoProfile -File .githooks/install.ps1
```

This configures **only** repository-local `core.hooksPath = .githooks`. It does **not** alter your global Git configuration, `user.name`, `user.email`, or commit history, and never rewrites files silently.

## Commit Messages, PR Titles, and Identity Rules

Pull requests are squash-merged into `main`, so your PR title becomes the commit subject on `main`. Both local commit subjects (`commit-msg`) and PR titles (`scripts/check-conventions.py`) enforce:

```text
type(scope): subject [TICKET-ID]
```

- **Allowed `type` values:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`
- **Required `scope`:** lowercase kebab-case matching `^[a-z0-9][a-z0-9-]*$` (for example `docs`, `repo`, `ci`, `convert`, `platform`)
- **Required `[TICKET-ID]`:** `[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)+` (for example `[PRJ-004]`, `[REL-009]`, `[ENG-001]`, `[DOC-001]`)
- **No `Co-authored-by` trailers:** any `Co-authored-by:` line (case-insensitive) anywhere in a commit message or PR description is rejected.
- **No Dependabot bypass:** Dependabot cannot append trailing ticket IDs via supported `dependabot.yml` commit-message settings, and CI intentionally enforces strict PR titles for all PRs without bot exemptions. A maintainer triages automatic dependency PRs and renames the PR title to append the stable dependency-freshness owner (`[REL-009]`, or a dedicated upgrade/security ticket) before merge, re-triggering the `pull_request` `edited` check.
- **Contributor identity:** commit using your own Git `user.name` and `user.email`. Do not enable automatic DCO/sign-off injection or rewrite other contributors' history.
- **Historical baseline:** initial documentation PR `#1` (`3801d37`) and the already-committed original documentation navigation repair (`c321475`) preceded repository convention checks and remain preserved in history as-is.

See [docs/issue-conventions.md](docs/issue-conventions.md) for ticket taxonomy, labels, and milestones.

## External Backlog Boundary

Detailed local backlog records (`356` tickets across `asciiwall` and `asciiwall-library`) and derived reports live **outside** Git in `` `../.issues` `` ([docs/backlog-validation.md](docs/backlog-validation.md), [docs/backlog-publication.md](docs/backlog-publication.md)):

- **Never** copy `.issues/`, `publication-ledger.json`, `publication-ledger.lock`, or `reports/` files into either Git repository.
- **Never** commit links pointing to `../.issues/...` or invented GitHub issue numbers prior to authorized publication.

## Engineering and Review Standards

Once application and host development starts, follow [docs/delivery/contributor-workflow.md](docs/delivery/contributor-workflow.md):

1. **Test-first loop:** write a focused failing test, confirm the intended failure, implement the minimal complete behavior, and run targeted plus affected regression checks.
2. **Architectural boundaries:** keep `ascii-core` free of UI, native host, FFmpeg, and network dependencies; `.asciiwall` v1 packages are strictly declarative data (never executable scripts or arbitrary shaders); desktop shell bridges never decode untrusted media.
3. **Platform evidence:** native desktop behavior requires manual reproduction and an exact environment tuple (OS build, session/compositor, GPU/driver, capabilities), never mock-only claims.
4. **Privacy and rights:** never commit personal media, proprietary fonts, tokens/secrets, or unredacted host paths. Any third-party dependency, font, codec build flag, or fixture requires verified license and notice records ([docs/library/licensing-and-attribution.md](docs/library/licensing-and-attribution.md)).
5. **Security reports:** never open a public PR or issue for an undisclosed vulnerability; follow [SECURITY.md](SECURITY.md).

## Related Policies

- [GOVERNANCE.md](GOVERNANCE.md) — maintainer ownership, review policy, and bootstrap branch protection
- [SECURITY.md](SECURITY.md) — private vulnerability disclosure and security invariants
- [SUPPORT.md](SUPPORT.md) — support channels and platform tuple boundaries
- [VERSIONING.md](VERSIONING.md) — independent SemVer and draft format versioning
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — community participation standards
