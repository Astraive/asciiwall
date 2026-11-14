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

A conversion key is a structural identity over the source content digest (or explicit missing-source state), decode/orientation/color pipeline versions, conversion algorithm version, font-byte digest and raster settings, ordered glyphs, grid, fit/crop, palette, and settings digest. Sampling filter, geometry, patch dimensions, and the sampling identity digest are included as well. Digests are computed by the owning source or rendering pipeline; paths and stat metadata are not key components.

Video identity adds selected stream, time range, canonical rational FPS, temporal algorithm version, and seed. GPU-derived identity nests its upstream conversion/video identity and adds backend, device, and shader-byte digest. Every behavior-changing input therefore creates a distinct identity; equivalent rational FPS values share one canonical representation.

A `SourceObservation` retains path and stat data only as revalidation hints beside the content identity. A changed path or stat hint for the same digest requires a fresh content validation before reuse; a changed digest naturally selects a different key, while an explicitly missing source is distinct from available content. Entries stored with an observation require the caller to provide the current observation when leasing them. Stat equality is never itself a content proof.

## Transactions

Write configuration to a new file, flush according to platform guarantees, then atomically replace. Keep a last-good backup. Package imports extract into a private staging directory, validate completely, and commit an immutable directory plus registry update. A crash-recovery journal distinguishes staged, verified, committed, and abandoned work.

## Eviction

A configurable byte budget evicts least-recently-used derived files only. Active decoder/upload users pin entries through non-cloneable leases; dropping a lease does not signal completion, and the owner must explicitly acknowledge completion before the file becomes eligible for eviction. On reopen, generated files without in-memory identities remain accounted and evictable. Keep the managed cache root private and stable: project directories, imported packages, and unmanaged files are outside the deletion scope.

Disk reservations account for expanded input, scratch, output capacity, and a safety reserve against a host-probed free-space estimate. The estimate can be refreshed, but real writes may still fail. Output is staged beside its destination and becomes registered only after successful flush, sync, and commit. A failed write reports its temporary partial path when one exists, does not commit or register the failed result, and leaves the destination unchanged provided its parent is not concurrently modified by an untrusted actor. Disk-full and quota failures are classified without claiming that a particular filesystem or quota limit was exercised.

## Privacy

Do not store cursor histories or window geometry histories. Diagnostic logs rotate and redact home directories and media filenames by default. Content exports strip machine-specific paths, monitor identifiers, permission grants, and application rules unless a clearly labeled nonportable local backup is requested.
