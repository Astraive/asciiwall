# Supply chain, signatures, and updates

## Dependency provenance

Commit Cargo.lock for application builds. Pin native FFmpeg source/build configuration and verify downloaded artifacts. Record enabled features, target architecture, compiler/toolchain, source revision, and license inventory. Cargo workspaces centralize shared dependency policy but do not automatically resolve incompatible graphics versions. [S34](../research/source-register.md#s34)

Scan known advisories and license policy in CI; review unsafe/native/parser dependency updates. A passing cargo-audit report means no matching known advisories in that database snapshot, not proof of security. [S62](../research/source-register.md#s62)

## CI privileges

Untrusted contribution tests have no production secrets or write tokens. Pin external actions/dependencies to reviewed immutable revisions where feasible. Protect release branches/tags and require approval for signing/publishing. Never run attacker-controlled PR code in a privileged `pull_request_target` context. [S43](../research/source-register.md#s43)

Cache poisoning matters: separate caches by trust context and verify artifacts crossing into a privileged job. Do not trust a file merely because a prior job uploaded it under a familiar name.

## Binary versus library updates

Application binaries and wallpaper catalog packages have different trust and activation paths. A wallpaper cannot replace the executable. A catalog entry cannot point the updater at arbitrary code. Initial application updates can be manual signed downloads; automatic self-update is a separate security feature, not a hidden baseline dependency.

On macOS, sign/notarize the distributed app and helpers. [S45](../research/source-register.md#s45) Other platform signing/package-integrity mechanisms need tested release integration. Maintain checksums and provenance; unsigned development builds are clearly labelled.

## Attestations

Build attestations can link artifacts to their producing workflow/source context. [S61](../research/source-register.md#s61) Verify them where supported, but do not equate provenance with safe dependencies or legitimate artwork rights. A malicious trusted workflow can still produce a signed bad artifact.

## Catalog trust

Verify exact-byte signatures against separately trusted keys, package hashes, monotonically increasing sequences, and expiration policy. [S60](../research/source-register.md#s60) A simple signed catalog does not implement every TUF defense. [S44](../research/source-register.md#s44) Document key compromise, rotation, offline stale metadata, and rollback recovery explicitly.

## Incident response

Keep the last known valid binary/package available for rollback where format compatibility permits. Publish affected hashes and versions without collecting user media. Test emergency release procedures in a separate nonproduction trust namespace.

Do not silently disable certificate checking or signature verification to fix a hosting outage. Failed verification should leave current local wallpapers usable and show a precise explanation.
