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
