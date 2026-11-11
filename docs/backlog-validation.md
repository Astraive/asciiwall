# Backlog validation and external layout

`scripts/validate-backlog.py` is a Python 3.12+ standard-library-only CLI that validates the local planning backlog against the frozen 100-document source pack and optionally writes deterministic derived reports. It performs no network calls, spawns no subprocesses, mutates no tickets or source documents, and creates no GitHub issues, labels, milestones, or project items.

## External directory layout

Local planning artifacts and derived reports live outside both Git repositories (`asciiwall` and `asciiwall-library`) so unpublished backlog data is never committed or pushed:

```text
<workspace>/
├── asciiwall/                  # Main repository (--source-root, default: ROOT)
│   ├── docs/                   # Frozen 100 original source documents + repo docs
│   └── scripts/
│       └── validate-backlog.py
├── asciiwall-library/          # Sibling artwork/catalog repository
└── .issues/                    # External backlog root (--backlog-root, default: ROOT.parent/.issues)
    ├── schema.json             # Draft 2020-12 schema for local issue.json records
    ├── asciiwall/<ID>/         # issue.json + CONTENT.md
    ├── asciiwall-library/<ID>/ # issue.json + CONTENT.md
    ├── coverage/*.json         # Slice read attestations & requirement mappings
    ├── labels.json             # Local label registry (if present)
    ├── milestones.json         # Local milestone registry (if present)
    ├── project.json            # Local project field/view configuration (if present)
    └── reports/                # Derived reports written only with --write-reports
```

Key isolation rules enforced by [scripts/validate-backlog.py](../scripts/validate-backlog.py):

- `--source-root` supplies the frozen 100 original Markdown documents under `docs/`.
- `--backlog-root` supplies `schema.json`, `asciiwall/`, `asciiwall-library/`, `coverage/*.json`, and any local backlog registries.
- Any `--backlog-root` inside `--source-root`, inside the main repository, inside `asciiwall-library`, or inside any `.git` repository is rejected (including through symlinked intermediate directories), as is a backlog root that contains the source repository.
- A missing, non-directory, or empty `--backlog-root` fails validation with exit code `1` (never passes green).
- Symlinked backlog roots, report directories, JSON files (`schema.json`, `issue.json`, `coverage/*.json`, report `.json` files), and escaping paths are refused.
- Empty or `null` JSON files fail closed rather than silently disappearing, malformed `coverage` fields (`documents`, etc.) report validation errors without crashing `--write-reports`, and error locations/messages use deterministic POSIX `/` path separators across Windows and POSIX platforms.
- Defensive `/.issues/` and `__pycache__/` patterns in both `asciiwall/.gitignore` and `asciiwall-library/.gitignore` prevent accidental tracking of local backlog directories or Python bytecode if placed inside a checkout.

## CLI usage

Default check-only mode is strictly read-only and writes nothing:

```console
python scripts/validate-backlog.py
python scripts/validate-backlog.py --backlog-root ../.issues --source-root .
python scripts/validate-backlog.py --backlog-root ../.issues --source-root . --json
```

Report generation mode (`--write-reports`) writes derived artifacts under `<backlog-root>/reports/` only:

```console
python scripts/validate-backlog.py --write-reports
python scripts/validate-backlog.py --backlog-root ../.issues --source-root . --write-reports --json
```

Exit code `0` means all structural, link, graph, and coverage checks passed (and reports were safely written when `--write-reports` was supplied). Exit code `1` means validation errors, missing/in-repo roots, or an unsafe/symlinked path.

## Derived report paths

When `--write-reports` is specified against a valid external `--backlog-root`, reports are written under `<backlog-root>/reports/` — **never** under `docs/generated/` inside the repository and **never** committed to version control:

- `<backlog-root>/reports/backlog-audit.json` — machine-readable counts and sorted validation errors.
- `<backlog-root>/reports/backlog-creation-ledger.json` — local-object inventory with `null` GitHub issue numbers, URLs, and project item IDs (`publication_status: "local-only"`).
- `<backlog-root>/reports/backlog-requirement-traceability.md` — requirement-to-issue traceability and planned acceptance checks.
- `<backlog-root>/reports/backlog-dependency-graph.md` — hard-blocker edges, cross-repository dependencies, most-blocking issues, and effort-weighted critical path.
- `<backlog-root>/reports/backlog-inventory.md` — ticket inventory and original-document read/mapping summary.

All five outputs are written via exclusive temporary files and atomic replacement after verifying that neither `<backlog-root>/reports/`, any target JSON/Markdown report, nor any `.tmp` file is a symlink or escapes `<backlog-root>`. Invalid backlogs still produce reports when `--write-reports` is used on an existing external `--backlog-root`, explicitly marked invalid; missing, symlinked, or in-repo `--backlog-root` locations refuse to create directories or write reports.

## Local Markdown links and preserved checks

From `<backlog-root>/<repo>/<ID>/CONTENT.md`:

- Links to main repository source documents resolve via `../../../asciiwall/docs/...` under `--source-root`.
- Same-repository ticket links resolve via `../<ID>/CONTENT.md` under `--backlog-root`.
- Cross-repository ticket links resolve via `../../<other-repo>/<ID>/CONTENT.md` under `--backlog-root`.

Local Markdown link targets must resolve inside `--backlog-root` or `--source-root`; links escaping both roots, missing targets, missing heading anchors, symlinked targets, and remote GitHub issue/PR URLs fail validation.

All semantic requirement and graph checks remain unchanged: Draft 2020-12 schema validation with strict JSON parsing, JSON/CONTENT agreement and mandatory headings, explicit ticket Markdown link parsing (`[ID](.../ID/CONTENT.md)`) in `Depends On` / `Related Issues` / `Blocks / Unblocks` (so technical prose tokens like `UTF-8` or `SHA-256` are not misread as issue IDs), globally unique IDs and descriptive titles, local-only status with null GitHub IDs, acyclic same-repository parent trees, acyclic non-redundant hard-blocker DAGs, exactly one milestone gate per `M0`–`M10` and `L0`–`L4`, and complete coverage across the 100 frozen original documents, 172 settings keys, 120 edge-case IDs, 36 recipe briefs, and 40 `E`/`D`/`P`/`H` work units.

## CI behavior

On a fresh repository clone, the external sibling `.issues` directory is absent and no local backlog source data or derived reports are committed to Git. Accordingly:

- Repository CI on fresh clones does **not** require `.issues` or run external-backlog validation, and never commits generated backlog reports; repository docs and policy checks run separately against tracked repository files.
- Local backlog validation is run explicitly against the external workspace sibling `.issues` directory (and invoked read-only without `--write-reports` by publication tooling prior to any dry-run or authorized run).
