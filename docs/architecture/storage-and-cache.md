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
config/     settings.json and settings.json.last-good
data/       install-registry.json (the sole authoritative install index)
library/    packages/<package-id>/<version>/<digest>/
projects/   user-created editable project directories
catalogs/   installed catalog snapshots and trust metadata
cache/      thumbnails, glyph-atlases, converted-images, clips
staging/    private incomplete imports, export workspaces, and import journals
logs/       bounded redacted diagnostic files
runtime/    session locks, local IPC endpoints, shared-memory metadata
```

The first release persists one versioned JSON install registry with atomic replacement. A later
SQLite index is acceptable as an embedded local file only with an explicit migration and recovery
plan; never keep two competing authoritative registries.

## Cache identity

A conversion cache key includes content hash, decode/orientation/color pipeline version, conversion algorithm version, font bytes hash, font raster parameters, glyph set/order, grid dimensions, fit/crop, palette, and settings digest. Video caches also include stream selection, time range, rational FPS, temporal algorithm, and seed. GPU-specific caches must additionally include relevant backend/device and shader version data.

A file path plus modification time is not a trustworthy identity for long-lived derived content. Use a fast stat check as a hint and content hashing when certainty matters. Local external media may move or change; show `missing source` or invalidate, never continue pretending it is the same artwork.

## Transactions

Configuration is parsed and accepted by the owning schema validator before persistence. Write a
bounded sibling temporary file, flush and sync it, then atomically replace the destination under a
directory-scoped lock; remove the operation's reserved temporary on failure and before retrying an
interrupted write. Sync the containing directory after replacement and sync each newly created
directory entry before reporting a first write as durable on platforms that support directory sync.
Keep `settings.json.last-good` only when the previous file also passes privacy checks and the current
schema validator; malformed, schema-invalid, and oversized current files do not replace a known-good
backup and do not prevent a valid replacement. Import-journal begin and state transitions serialize
callers sharing a journal directory, accepting only staged→verified/abandoned and
verified→committed/abandoned transitions. Recovery classifies interrupted work without silently
changing journal state. Importers own the transitions and commit their validated immutable content
plus registry update.

## Eviction

Cache has a configurable byte budget, an LRU policy, and pinned active resources. Derived cache can be deleted without losing projects or imported packages. Do not evict a file while a decoder or GPU upload still relies on it; coordinate refcounts/leases and platform file-lock behavior.

Low-disk checks include required extraction bytes, conversion scratch, output bytes, and safety reserve. When an export runs out of space, report a partial temporary artifact, preserve the original, and avoid registering the result as valid.

## Privacy

Never persist cursor histories or window geometry histories; both portable and explicitly
labeled nonportable local-backup exports omit them. Diagnostic logs use bounded rotation, cap each
record independently of each file, serialize writers targeting the same directory, and redact home
directories and media-bearing messages by default. A clipped message is redacted as a whole because
the truncated prefix cannot prove whether a path or filename is private. Portable exports strip
machine-specific paths, file URIs, network endpoints (including host-and-port strings and IP
addresses), monitor/display identifiers, permission grants, application rules, and user/home data;
only an explicitly labeled nonportable local backup may retain those values.
