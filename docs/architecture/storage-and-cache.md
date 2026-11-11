# Local storage, cache keys, and persistence

## Storage classes

Keep configuration, installed immutable content, derived cache, temporary staging, logs, and runtime IPC separate. On Linux, use XDG-style config/data/cache/runtime locations; on Windows use the appropriate per-user application directories; on macOS use standard Application Support and Caches conventions. Paths are computed by a platform-aware library, not hard-coded usernames.

```text
config/     settings.toml, display-assignments.json, permissions.json
library/    packages/<package-id>/<version>/<digest>/
projects/   user-created editable project directories
catalogs/   installed catalog snapshots and trust metadata
cache/      thumbnails, glyph-atlases, converted-images, clips
staging/    incomplete imports and export workspaces
logs/       bounded redacted diagnostic files
runtime/    session locks, local IPC endpoints, shared-memory metadata
```

For the first release, a versioned JSON install registry with atomic replacement is sufficient. A later SQLite index is acceptable as an embedded local file, not a server, but must have an explicit migration and recovery plan. Avoid two competing authoritative registries.

## Cache identity

A conversion cache key includes content hash, decode/orientation/color pipeline version, conversion algorithm version, font bytes hash, font raster parameters, glyph set/order, grid dimensions, fit/crop, palette, and settings digest. Video caches also include stream selection, time range, rational FPS, temporal algorithm, and seed. GPU-specific caches must additionally include relevant backend/device and shader version data.

A file path plus modification time is not a trustworthy identity for long-lived derived content. Use a fast stat check as a hint and content hashing when certainty matters. Local external media may move or change; show `missing source` or invalidate, never continue pretending it is the same artwork.

## Transactions

Write configuration to a new file, flush according to platform guarantees, then atomically replace. Keep a last-good backup. Package imports extract into a private staging directory, validate completely, and commit an immutable directory plus registry update. A crash-recovery journal distinguishes staged, verified, committed, and abandoned work.

## Eviction

Cache has a configurable byte budget, an LRU policy, and pinned active resources. Derived cache can be deleted without losing projects or imported packages. Do not evict a file while a decoder or GPU upload still relies on it; coordinate refcounts/leases and platform file-lock behavior.

Low-disk checks include required extraction bytes, conversion scratch, output bytes, and safety reserve. When an export runs out of space, report a partial temporary artifact, preserve the original, and avoid registering the result as valid.

## Privacy

Do not store cursor histories or window geometry histories. Diagnostic logs rotate and redact home directories and media filenames by default. Content exports strip machine-specific paths, monitor identifiers, permission grants, and application rules unless a clearly labeled nonportable local backup is requested.
