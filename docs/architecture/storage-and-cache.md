# Local storage, cache keys, and persistence

## Storage classes

Resolve each storage class independently from platform directories, rather than embedding user
names or runtime endpoints in portable data. Linux uses absolute XDG config, data, and cache
directories, falling back to the standard per-user locations when an XDG value is absent or
relative; runtime uses absolute `XDG_RUNTIME_DIR` or falls back to `data/runtime` when unset.
macOS uses `~/Library/Application Support/<app>` for config and data, `~/Library/Caches/<app>`
for cache, and `data/runtime` for runtime state. Windows keeps config in Roaming AppData and
other local state in Local AppData.

```text
config/     settings.toml and settings.toml.last-good
data/       install-registry.json (the sole authoritative install index)
library/    packages/<package-id>/<version>/<digest>/
projects/   user-created editable project directories
catalogs/   installed catalog snapshots and trust metadata
cache/      thumbnails, glyph-atlases, converted-images, clips
staging/    private incomplete imports, export workspaces, and import journals
logs/       bounded redacted diagnostic files
runtime/    session locks, local IPC endpoints, shared-memory metadata
```

The first release persists a bounded TOML settings document at `settings.toml`; storage checks TOML
syntax while the owning configuration API validates its schema. The install registry remains a
versioned JSON file with atomic replacement. A later SQLite index is acceptable as an embedded local
file only with an explicit migration and recovery plan; never keep two competing authoritative
registries.

## Cache identity

A conversion key is a structural identity over the source content digest (or explicit missing-source state), decode/orientation/color pipeline versions, conversion algorithm version, font-byte digest and raster settings, ordered glyphs, grid, fit/crop, palette, and settings digest. Sampling filter, geometry, patch dimensions, and the sampling identity digest are included as well. Digests are computed by the owning source or rendering pipeline; paths and stat metadata are not key components.

Video identity adds selected stream, time range, canonical rational FPS, temporal algorithm version, and seed. GPU-derived identity nests its upstream conversion/video identity and adds backend, device, and shader-byte digest. Every behavior-changing input therefore creates a distinct identity; equivalent rational FPS values share one canonical representation.

A `SourceObservation` retains path and stat data only as revalidation hints beside the content identity. A changed path or stat hint for the same digest requires a fresh content validation before reuse; a changed digest naturally selects a different key, while an explicitly missing source is distinct from available content. Entries stored with an observation require the caller to provide the current observation when leasing them. Stat equality is never itself a content proof.

## Transactions

Configuration is parsed and accepted by the owning schema validator before persistence. Storage also
bounded-parses existing TOML to reject malformed, oversized, or unsupported current data before
creating a temporary or touching either destination or `.last-good`. Persist the original validated
bytes, not a storage-layer reserialization. Write a bounded sibling temporary file with mode
`0600` on Unix, flush and sync it, then atomically replace the destination under a
directory-scoped lock; remove the operation's reserved temporary on failure and before retrying an
interrupted write. Sync the containing directory after replacement and sync each newly created
directory entry before reporting a first write as durable on platforms that support directory sync.

Settings saves and explicit recovery are compare-and-swap writes: callers provide the exact current
bytes observed from `load_config` or, when recovering an invalid file, the raw bytes previously
read from its path; callers pass `None` only when the file was absent. Storage compares this
expectation under the directory lock before replacement; a mismatch returns the typed
`StaleConfiguration` error without changing either file.
Ordinary saves never overwrite invalid, unsupported, or oversized current settings and do not rotate
them into `.last-good`. Explicit recovery applies the same recursive history-key policy to the
backup and can restore a schema-valid one to current settings; it leaves the backup in place and
rejects forbidden keys without changing either file. Recovery refuses a lower integer schema version
and fails closed when exactly one parseable current or backup document declares an integer version.
Hook-injected failures cover temporary creation, temporary sync, backup replacement, and
destination replacement; tests
check complete valid current and last-good documents, owned-temporary cleanup, rejection without byte
changes, explicit recovery, and subsequent successful saves at each boundary. App-owned storage
directories are mode `0700`; persisted state and diagnostic log files, including rotated logs, are
`0600` on Unix.

Import-journal begin and state transitions serialize callers sharing a journal directory, accepting
only staged→verified/abandoned and verified→committed/abandoned transitions. Recovery classifies
interrupted work without silently changing journal state. Importers own the transitions and commit
their validated immutable content plus registry update.

The failure hooks model returned in-process errors; they do not establish process-kill or OS power-loss
recovery guarantees. Parent-directory sync is implemented on Unix and is a no-op on other targets.

## Eviction

A cache root uses a marked cache-owned subdirectory; a generated-looking filename at the root or in an unmarked directory is never sufficient ownership evidence. Operations that read or change root-wide byte accounting (open, store, budget changes, trimming, and snapshots) scan generated files beneath the root while holding its operations lock, so independently opened instances account for the same root-wide byte usage. Stores keep that lock through file creation, write, flush, and in-memory state publication; stale per-instance accounting cannot reserve bytes already stored by another instance. Instances sharing a root must use the same configured byte budget. Each instance retains semantic identity and LRU state only for its own known entries; files discovered from another instance or after reopen remain accounted as orphans and are eviction candidates ordered by generated file ID, not available for semantic lookup by identity.

Active decoder/upload users pin entries through non-cloneable leases; dropping a lease does not signal completion, and the owner must explicitly acknowledge completion. A live lease holds a shared OS file lock, and eviction must obtain an exclusive lock before deleting a file, so another cache instance cannot remove an actively leased artifact. Keep the managed cache root private and stable: project directories, imported packages, and unmanaged files remain outside the deletion scope.

Disk reservations account for expanded input, scratch, output capacity, and a safety reserve against
a host-probed free-space estimate. Capture a `DiskBudgetProbe` with
`begin_available_bytes_probe` before probing the host, then apply that reading with
`refresh_available_bytes`. The ledger charges every output byte written since the token was
captured and ignores readings from older tokens, so a delayed probe cannot forget concurrent writes.
Existing reservations are preserved; a later probe replaces prior consumed-byte charges with writes
since that token was captured. Real writes may still fail.

Output is staged beside its destination and becomes registered only after successful flush, sync,
and commit. Relative output paths are normalized once to an absolute destination at transaction
start; staging, verification, errors, commit, and registration all use that captured path even if
the writer callback changes the process working directory. A callback cannot suppress an underlying
write or flush error and publish truncated output. A failed write reports its temporary partial path
when one exists, does not commit or register the failed result, and leaves the destination unchanged
provided its parent is not concurrently modified by an untrusted actor. Replacing a registered
destination invalidates any identity for the same canonical path or, where supported, the same
filesystem file identity. Unix case-variant names are compared by resolving existing paths through
the actual filesystem, so case-insensitive volumes clear stale registrations while case-sensitive
volumes retain distinct names; Windows uses the directory's case-sensitivity setting. Disk-full and
quota failures are classified without claiming that a particular filesystem or quota limit was
exercised.

## Privacy

Never persist cursor histories or window geometry histories; both portable and explicitly labeled
nonportable local-backup exports omit them. Diagnostic logs use at most 16 files of 16 MiB each,
with each record capped at 64 KiB. Their input is bounded before redaction, clipped messages are
redacted as a whole because a truncated prefix cannot prove whether a path or filename is private,
and writers targeting the same directory are serialized. Home paths, media-bearing messages, and
path-like values are redacted by default. Portable exports strip machine-specific paths, file URIs,
bare host fields and network endpoints (including single-label host-and-port strings and IP addresses),
monitor/display identifiers, permission grants, application rules, and user/home data; only an explicitly labeled
nonportable local backup may retain those values. Export traversal is capped at 16,384 node/key
visits and depth 64; keys are limited to 256 bytes, strings to 16 KiB, and encoded JSON to 1 MiB.
An over-budget export fails with `DocumentTooLarge` instead of returning a partial export.
