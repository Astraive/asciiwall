# Library build and release pipeline

## Immutable release flow

A protected commit selects wallpaper recipes and a pinned engine/schema version. A clean worker resolves only approved digest-pinned assets, packs each wallpaper, validates it independently, creates static previews, and records hashes/sizes. A trusted publishing job uploads immutable versioned packages and signs a catalog referring to those exact bytes.

Never modify bytes behind an existing ID/version/hash URL. Fixes create a new version and catalog sequence. Assignments pin installed versions; downloading new metadata never changes a running wallpaper automatically.

## Suggested artifacts

Per wallpaper: `slug-version.asciiwall`, package checksum, preview, and provenance/attribution metadata. Per collection release: catalog JSON, detached signature, changelog, build manifest, and optional offline bundle. Large artifacts live outside ordinary Git history. GitHub releases are one practical static distribution mechanism, subject to current service limits. [S41](../research/source-register.md#s41) [S42](../research/source-register.md#s42)

An offline collection bundle contains a catalog and packages with relative local paths in a separately specified bundle index, not malicious `file://` URLs in the remote catalog. It still passes the normal import validator. Define that bundle format later; v1 users can import several ordinary `.asciiwall` files without it.

## Network behavior

All online operations are explicit. Before download show size, source origin, claimed author, license, required features, and trust state. Use bounded retries and temp files. Resume only with validated range/ETag/hash rules; otherwise restart safely. Verify whole-file SHA-256 before extraction.

Proxy/certificate failures must not disable TLS validation. Captive-portal HTML is not a package. A redirect to an unapproved origin requires new approval or is rejected. Rate limiting and network loss preserve local functionality.

## Signing and provenance

Keep signing material out of PR workflows. Protect release approvals and branch/tag permissions. A signed catalog authenticates the chosen artifacts, not their artistic quality or absence of decoder bugs. Build attestations can record where/how artifacts were produced but are not a substitute for review. [S61](../research/source-register.md#s61)

Document key rotation and emergency catalog replacement before the first public release. Back up keys safely and test recovery without publishing fake production releases.

## Local collection indexing

The desktop maintains a local metadata index with package ID/version/hash, installation location, tags, compatibility, trust state, and last-use data. JSON or an embedded database is an implementation choice; neither implies a backend server. Start with atomic indexed files, migrate to embedded SQLite only if measured scale warrants it.

Keep index reconstruction possible from immutable installed packages. Corrupted search metadata must not destroy source packages or user overrides. Removed media is surfaced as missing, not silently redownloaded.
