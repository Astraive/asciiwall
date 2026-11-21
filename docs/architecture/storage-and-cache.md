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

A conversion cache key includes content hash, decode/orientation/color pipeline version, conversion algorithm version, font bytes hash, font raster parameters, glyph set/order, grid dimensions, fit/crop, palette, and settings digest. Video caches also include stream selection, time range, rational FPS, temporal algorithm, and seed. GPU-specific caches must additionally include relevant backend/device and shader version data.

A file path plus modification time is not a trustworthy identity for long-lived derived content. Use a fast stat check as a hint and content hashing when certainty matters. Local external media may move or change; show `missing source` or invalidate, never continue pretending it is the same artwork.

## Transactions

Configuration is parsed and accepted by the owning schema validator before persistence. Storage also
bounded-parses existing TOML to reject malformed, oversized, or unsupported current data before
creating a temporary or touching either destination or `.last-good`. Persist the original validated
bytes, not a storage-layer reserialization. Write a bounded sibling temporary file with mode
`0600` on Unix, flush and sync it, then atomically replace the destination under a
directory-scoped lock; remove the operation's reserved temporary on failure and before retrying an
interrupted write. Sync the containing directory after replacement and sync each newly created
directory entry before reporting a first write as durable on platforms that support directory sync.
Ordinary saves never overwrite invalid, unsupported, or oversized current settings and do not rotate
them into `.last-good`. Explicit recovery can restore a schema-valid `.last-good` to current settings;
it leaves the backup in place for another recovery attempt. Hook-injected failures cover temporary
creation, temporary sync, backup replacement, and destination replacement; tests check complete valid
current and last-good documents, owned-temporary cleanup, rejection without byte changes, explicit
recovery, and subsequent successful saves at each boundary. App-owned storage directories are mode
`0700`; persisted state and diagnostic log files, including rotated logs, are mode `0600` on Unix.
Import-journal begin and state transitions serialize callers sharing a journal directory, accepting
only staged→verified/abandoned and verified→committed/abandoned transitions. Recovery classifies
interrupted work without silently changing journal state. Importers own the transitions and commit
their validated immutable content plus registry update.

The failure hooks model returned in-process errors; they do not establish process-kill or OS power-loss
recovery guarantees. Parent-directory sync is implemented on Unix and is a no-op on other targets.

## Eviction

Cache has a configurable byte budget, an LRU policy, and pinned active resources. Derived cache can be deleted without losing projects or imported packages. Do not evict a file while a decoder or GPU upload still relies on it; coordinate refcounts/leases and platform file-lock behavior.

Low-disk checks include required extraction bytes, conversion scratch, output bytes, and safety reserve. When an export runs out of space, report a partial temporary artifact, preserve the original, and avoid registering the result as valid.

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
