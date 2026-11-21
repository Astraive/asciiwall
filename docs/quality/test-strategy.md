# Testing strategy and evidence levels

## Evidence levels

Document-schema validation proves that examples match the proposed schema; it does not prove the app exists. Unit tests prove bounded functions for selected cases; they do not prove wallpaper layering. A normal GPU preview does not prove desktop integration. Manual native testing is required for shell behavior, permissions, power, accessibility, and installer cleanup.

Tag reports as document QA, unit/property, integration, fuzz, benchmark, native manual, or release acceptance. Record exact source/binary hash and environment. Never merge these into one vague 'tested' badge.

### Offline evidence tooling

Use [`scripts/evidence.py`](../../scripts/evidence.py) to create reproducible records and artifacts. It records evidence metadata and runs only its own synthetic controls; it does not execute the application or native acceptance procedures. JSON is canonical, content-addressed, schema-versioned, bounded, and written without overwriting existing files. The archive directory must already exist. Inputs are limited to 1 MiB, logs to 256 KiB before and after sanitization, grids to 4,096 cells, and pruning to at most 1,000 retained artifacts per type. Pruning fails before deletion if the archive enumeration exceeds 8,192 entries.

```text
python -c "from pathlib import Path; Path('tests/evidence/reports').mkdir(parents=True, exist_ok=True)"
python scripts/evidence.py record evidence-input.json --output evidence-record.json
python scripts/evidence.py validate evidence-record.json
python scripts/evidence.py fake-control timestamp-fault --archive tests/evidence/reports
python scripts/evidence.py grid-diff actual-grid.json --archive tests/evidence/reports
python scripts/evidence.py sanitize-log run.log --archive tests/evidence/reports
python scripts/evidence.py prune tests/evidence/reports --keep 100
python scripts/test_evidence.py
```
On Windows, also run `python scripts/test_evidence_windows.py` to exercise the anchored helper against real local-NTFS files and handles.

Each record distinguishes document QA, unit/property, integration, fuzz, benchmark, native manual, and release acceptance. Records require source, binary, and fixture SHA-256 hashes; EC IDs when applicable; observed OS/session/compositor/GPU and permission state; result; repository-relative evidence path; issue; reviewer; and date. Native/manual records require a procedure ID. An absent host must use explicit `not-available` environment details and `not-run`; it can never be reported as a pass.
#### Native manual pass manifests

For a native/manual record with `status: pass`, `evidence_path` may name a JSON manifest. It must be one object with exactly these top-level fields: `schema_version: 1`, `artifact_type: "native-manual-evidence"`, `execution: "manual"`, `procedure_id`, `reviewer`, `date`, `status: "pass"`, and `artifacts`. The manifest's `procedure_id`, `reviewer`, and `date` must match the record. Each `artifacts` item must be an object with exactly `path` (a repository-root-relative path to a regular file) and `sha256` (64 lowercase hexadecimal SHA-256 characters). The list must contain 1–16 unique paths; each referenced file must be nonempty and at most 16 MiB, with an aggregate maximum of 64 MiB. Files are streamed and hash-checked. A referenced file that looks like JSON must fit the 1 MiB inspection limit and be inspectable JSON.

Use a manifest for native binary evidence. A direct artifact is limited to 1 MiB and may be nonempty UTF-8 non-JSON text without unsafe control characters, or a validated sanitized-log JSON report. Parse failures that still match JSON syntax—including integers rejected by the runtime's digit-conversion limit—are rejected instead of treated as free text. Known synthetic control and CPU-grid-diff markers are rejected even if renamed or referenced by a manifest; this is not an exhaustive detector of fabricated evidence. Matching metadata, snapshots, and hashes establish consistency only: they do not authenticate a reviewer, prove a native procedure ran, or establish that the content is genuine.

The recorder filters free-text environment values and result summaries and sanitizes `sanitize-log` output; it does not sanitize arbitrary direct evidence or referenced binary files. Dotted private-field labels such as `input.value` are redacted, and a free-text line containing a path separator, `file:` URL, or Windows drive prefix such as `C:MedicalReport.pdf` is redacted in full. These conservative filters may remove benign text and cannot identify every sensitive value. `evidence_path` and manifest paths are retained as written. Before archiving or publishing, use nonpersonal filenames and inspect and sanitize all text, binaries, and their embedded metadata; do not include personal media, window, or input traces in CI artifacts. Use only rights-cleared evidence. Hashes and validation are not privacy scans.

#### Safe filesystem access and retention

Native/manual pass inspection and pruning require no-follow directory-relative access. On POSIX the tool opens components through directory descriptors, refuses symlinks, and pins the archive directory; systems lacking the required descriptor operations fail closed. POSIX provides no portable atomic compare-inode-and-unlink operation, so pruning refuses to delete expired entries rather than risk deleting a same-name replacement after a snapshot check. Before returning success, pruning also revalidates retained candidate identities and reports changes as an incomplete prune, preserving replacements instead of returning stale `kept_by_type`; this snapshot check does not provide bounded retention against hostile concurrent writers on POSIX. The fail-closed behavior preserves entries and reports an error; retention enumeration and inspection remain available. On Windows the anchored-handle deletion protocol remains available. Do not prune an archive writable by untrusted or concurrent processes.
Each prune is limited to 256 MiB of aggregate verification reads across candidate records, native-pass manifests, referenced artifacts, and Windows snapshot rechecks. On Windows, each candidate reread reservation is precharged at its scan-time `st_size` (archive candidates are limited to 1 MiB), and every deletion or retained-entry reread is checked against that snapshot before reading and capped at exactly that reservation. All scan and reread bytes count toward the aggregate. Exceeding the limit returns an error before deletion and without successful retention counts. This keeps the 8,192-entry scan's theoretical 64 MiB-per-record referenced-artifact workload (about 512 GiB) within a 256 MiB verification budget.

On Windows these operations require a local, fixed, drive-rooted NTFS volume. Each component is opened relative to a pinned ancestor handle with reparse traversal disabled; junctions, symlinks and other reparse points are rejected. Network paths, substituted drives, removable or unsupported volumes, and filesystem mount points encountered as reparse points fail closed rather than falling back to pathname-only access. The repository root is taken as a lexical absolute path so that its components are checked rather than resolved through reparse points. Before pruning starts deletion, it checks every candidate against its scan-time snapshot. For pruning, entries are reopened without write or delete sharing; each later reread is limited to that candidate's precharged scan-time `st_size` and compared to the scan-time snapshot and content before deletion through the same handle. Archive scans surface file-handle close failures as incomplete pruning even if a candidate also raises a scan, type, or read error; the primary error remains in the cause chain instead of the candidate being silently skipped. Ordinary unreadable candidates remain skippable if their handle closes successfully.

Neither backend takes a transaction over a changing archive. In particular, Windows' multi-call enumeration can omit concurrently moved entries and produce stale retention ordering; validating each selected deletion handle does not establish that the ranking was complete. The bounded rereads and snapshot checks do not make pruning transactional: a candidate changed after an earlier deletion completed is rejected, and pruning reports an incomplete operation listing completed removals rather than returning successful retention counts. On Windows, duplicate enumeration entries, excessive entry counts, and enumeration failures are rejected, but absence of those errors is not proof of an unchanged directory. Windows sharing modes do not prevent attribute-only access, so even an open file's modification time can change. A file lock or repeated scan alone cannot exclude noncooperating writers. The tool cannot verify exclusive ownership: keep the archive writable only by a dedicated account and quiesce other processes with write access, including same-account or privileged processes, during pruning. Read snapshots and sharing restrictions are not a guarantee against every pre-existing writable memory mapping; use controlled, quiescent evidence files. They do not replace independent native execution and human review.

`fake-control` scenarios (`timestamp-fault`, `capability-change`, `surface-failure`, and `cancellation`) exercise only synthetic decoder/host controls. Their reports bound resources, queued frames, and generations and explicitly disclaim product behavior. The small CPU grid golden is synthetic and compared exactly; changed cells are listed in a diff that stays `unreviewed` until reviewer/date metadata is supplied. Raster screenshots are separate and must document platform/font/GPU tolerances.

Native manual procedures remain separate from fake/VM evidence. Evidence for physical mixed-DPI/GPU/power behavior, native shell/permissions, screen-reader accessibility, and installer cleanup is still outstanding: run each procedure on its named host, retain its hashes, environment and permission state, result, evidence path, reviewer, and date, and mark unavailable hosts `not-run`. Do not treat this recorder or a synthetic control report as a receipt for those procedures. Sanitize retained logs; CI artifacts must not contain personal media, window, or input traces.

The `tests/evidence/evidence-record.schema.json` file defines the closed record schema. The offline recorder suite covers required fields, privacy filtering, deterministic controls, exact-grid diffs, sanitized logs, and archive retention on Linux, macOS, and Windows. Windows CI and `python scripts/test_evidence_windows.py` additionally exercise the NTFS handle boundary; no test outcome is implied by the presence of these tests.

## Engine correctness

Unit-test color transforms, alpha composition, cell aspect ratio, glyph calibration, deterministic seeds, matcher score normalization, row strides, frame dimensions, timestamp arithmetic, and cache fingerprints. Property tests enforce checked multiplication, no out-of-range glyph IDs, finite values, valid buffers, and deterministic output under identical inputs.

Golden tests compare character grids exactly for deterministic CPU algorithms. Raster screenshots use documented tolerances because font/GPU/platform differences can affect pixels. Keep goldens small and licensed. Changing a golden requires an explanation, not automatic acceptance of every generated difference.

## Media and scheduling

Use fixtures for VFR, B-frames, truncated files, absent duration, unusual orientation, animated-image disposal, alpha, unsupported HDR, decoder drain, seek, loop, and cancellation. A fake decoder allows precise timestamps and failure injection without relying on a particular codec build.

Model tests exercise pause-reason combinations, generation cancellation, bounded queues, dropped late frames, suspend/resume, repeated Apply, and decoder crashes. Assert queue/memory bounds, not just correct final appearance.

## Package and configuration

Validate schemas, semantic graphs, paths, hashes, budgets, unknown fields, migration backups, revision conflicts, and deterministic packing. Fuzz archive headers, manifest parser, scene graph, clip binary reader, and IPC envelope. Run fuzz jobs in isolated environments; malicious fixtures are not opened manually in the desktop shell.

Native parser dependencies require sanitizer/fuzz coverage where supported. Known-advisory scanning is an additional input, not a replacement for runtime defenses. [S62](../research/source-register.md#s62)

## Platform contracts

A fake host provides capability changes and surface failures for CI. Real-host tests verify behind-icons placement, no focus stealing, click-through, desktop menus/dragging, output changes, shell restart, lock, fullscreen, and safe shutdown. Record exact Windows build/macOS version/Linux compositor and graphics backend.

Plasma/GNOME bridge testing includes shell-main-thread responsiveness and complete disable cleanup. Virtual machines help broad coverage but do not replace physical mixed-DPI/GPU and power testing.

## UI and accessibility

Test keyboard-only first-run/import/apply/pause/export, screen-reader labels, high-contrast and scaled UI, localization overflow, unsaved changes, batch cancellation, and effective-settings explanations. A visual screenshot alone does not prove accessibility.

## Release gates

All P0 cases must pass for every claimed supported host; P1 failures are explicitly triaged with documented limitations. No release if import can write outside staging, a wallpaper steals input, crash loops block login, or the advertised offline path requires network. See [edge cases](edge-case-matrix.md) and [acceptance criteria](acceptance-criteria.md).
