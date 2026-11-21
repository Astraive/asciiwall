# Configuration validation and migrations

## Validation pipeline

Parse with byte/depth limits; reject duplicate keys and non-finite numbers; check version; validate known keys/types/ranges; enforce cross-field invariants; resolve assets/fonts; apply capability/resource constraints; prepare resources; commit atomically. A malformed value must not reach a native API or GPU allocation.

Reject unknown configuration keys unless they occur inside an explicitly namespaced extension field supported by that format. Stable v1 packages have no arbitrary extension execution. UI typo correction can be offered, but the stored file is not silently reinterpreted.

## Failures and recovery

Keep the last valid saved configuration and an explicit migration backup. Write to a same-directory temporary file, flush as appropriate, then atomically replace. Preserve restrictive local permissions. Disk-full errors keep the old file and show an actionable message.

A corrupted main config starts in safe mode with defaults and a repair prompt. It must not delete library entries. On startup, validate the last wallpaper before enabling it; repeated failures suppress automatic restore.

## Versioning

Use a configuration schema version separate from app version, package format, catalog schema, clip binary version, and IPC protocol. Each migration transforms one version to the next with tests and a reversible backup. An older app opening a newer config should refuse destructive writes and offer read-only/fallback mode.

Migrate portable settings separately from machine-specific assignments. Never convert absent permission fields into granted consent. Removed controls produce an explanation in migration diagnostics; a security-relevant control is not silently mapped to a weaker alternative.

## Cross-field examples

A 1024-by-1024 grid satisfies individual dimensions but may violate the user's cell budget. A loop end of 1000 ms with a start of 2000 ms is invalid. A custom character set consisting only of control characters is invalid even when it fits the string length. A manual scale of 1.25 does not apply when automatic scale is selected. A coverage threshold pair of 0.9/0.98 is inverted and rejected.

## Concurrent writers

The controller owns active configuration. CLI and UI changes use a revision precondition. A stale write returns a conflict with the current revision rather than silently replacing other changes. Batch edits validate as a transaction and either apply entirely or not at all.

## Tests

Property-test range boundaries, overflow, nesting, duplicate keys, invalid UTF-8, unsupported versions, lost disk space, interrupted writes, stale revisions, and rollback. Golden migration fixtures include old defaults and explicit user overrides so upgrades do not erase intentional customization.

## CFG-001 implementation boundary

`crates/ascii-config` owns the common typed descriptor registry, layer-authority checks, requested/effective resolution, validation, immutable snapshots, revisioned batch controller, persistence, and schema migration machinery. Domain setting descriptors and their source-documented defaults remain assigned to CFG-004..016; `Registry::empty()` is the foundation default, and this change does not claim the proposed 172-key table is populated.

The canonical file is `settings.toml`, selected under the platform's per-user config directory by the consuming application. Its current schema is `2`, independent from application and other format versions. The closed envelope is `schema_version`, `revision`, and `[settings]` with quoted dotted scalar keys. Unknown root fields and setting keys are rejected; no extension namespace is currently modeled. The decoder rejects invalid UTF-8, duplicate TOML keys, non-finite numbers, unsupported older versions, invalid scalar structures, excessive nesting, and files over 256 KiB. Preflight caps nesting at 16; `MAX_SETTING_ENTRIES` caps parsed setting entries at 4,096, in addition to the two envelope assignments.

Writes go only through `SettingsController`: batches validate completely, compare the expected revision and current bytes, acquire a per-file OS lock, write a same-directory temporary file, sync it, atomically replace, and sync the parent directory where supported. A failed durable write does not publish the candidate snapshot; if replacement happened but directory sync failed, the old main bytes are restored or an indeterminate-commit error is returned. `.bak` holds the previous valid bytes, while corrupt bytes are archived separately during explicit repair. Unix config/temp files use owner-only mode `0600`. Windows temp files inherit the containing directory ACL; this crate does not call or verify native ACL APIs.

The only migration fixture is a clearly synthetic pre-release schema-1 document. Its consecutive v1→v2 migration changes only the schema marker, preserves every explicit scalar override, leaves absent consent absent, and retains the exact predecessor bytes as `.bak`. No deployed historical schema or migration data is claimed. Future schema documents open read-only. Corrupt documents start with validated registry defaults in `RepairRequired`; repairing archives the corrupt file and does not touch library content.

The `RestoreFailureState` API validates a last-wallpaper candidate before returning it and suppresses retries according to a caller-supplied policy. The host must persist that state and report activation/resource-preparation failures. No desktop host or CLI currently exists on main, so no automatic restore path is wired; CLI `--config` is also outside this change.

Tests and synthetic golden fixtures cover authority, range endpoints/overflow, requested-versus-effective constraints, semantic checks, TOML bounds and corruption classes, schema/override migration, revision conflicts across independent controllers, batch rollback, explicit repair/backup restore, and injected storage failures. This implementation agent did not run checks; verification remains pending the parent gates.
