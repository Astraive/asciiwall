# Security threat model

## Assets and trust boundaries

Protect the user's files, desktop responsiveness, session privacy, application settings, release signing keys, and integrity of installed packages. Treat imported archives, images, fonts, videos, scene graphs, catalog metadata, filenames, and contributor PRs as untrusted.

Boundaries: network-to-downloader; archive-to-validator; validator-to-private staging; controller-to-worker; worker-to-native codec/font parser; controller-to-host; host-to-shell; PR-to-CI; CI-to-release signing. Local IPC is also a boundary even though there is no backend.

## Adversary scenarios

| Threat | Main mitigation | Residual limitation |
|---|---|---|
| Archive overwrites arbitrary file | Strict paths, no-follow extraction, private staging, no overwrite | Requires platform-specific race testing |
| Decoder exploit | Patched codecs, restricted worker, limited handles/resources | Isolation is not proof of no vulnerabilities |
| ZIP/image decompression bomb | Actual byte/pixel/time caps and cancellation | OS memory accounting still needs measurement |
| Malicious scene consumes GPU | Bounded built-in graph; no arbitrary v1 shaders | Built-in effect bugs remain possible |
| Package enables monitoring | Permissions outside portable data | User may explicitly grant broader observation |
| Catalog replacement | Trusted-key signatures and exact artifact hashes | Offline clients cannot know latest revocation |
| Rollback/freeze | Sequence/expiration policy, visible stale state | Not a complete TUF design |
| Privileged CI compromise | Untrusted PR jobs separated from signing | Maintainer account/key compromise remains a risk |
| Shell crash | Minimal bridge, no untrusted decode inside shell | Extension/API bugs can still affect desktop |
| Malicious same-user process | Peer checks, private endpoints, no arbitrary command API | Same-user malware can often tamper with local state |

## Security goals and non-goals

No runtime networking in core playback/import/conversion/export. No screenshots, keystroke recording, or title collection. No downloaded executable package content. No admin/root requirement for normal operation. A broken wallpaper must be stoppable without losing desktop access.

This is not a defense against a fully compromised OS, hostile GPU driver, administrator controlling the account, or deliberate modification of application binaries. Do not market the app as a security sandbox for arbitrary programs.

## Release gates

Before community untrusted imports are enabled, demonstrate bounded extraction, decoder restrictions, process cleanup, and denied unrelated-file/network access on every supported platform. If a platform's worker confinement has not been proven, label the build limited/trusted-input-only rather than claiming complete isolation.

Security review covers both Rust and native FFI. Rust memory safety does not eliminate unsafe FFI, logic errors, archive traversal, excessive allocation, or codec vulnerabilities. Dependency scanning finds known reports, not every flaw. [S62](../research/source-register.md#s62)

## Incident handling

Publish a private vulnerability-report channel, affected-version policy, and hash-based advisory format. A security update must not silently upload user media for analysis. Reproduce with minimized consented fixtures and keep private source files out of public issue trackers.

A dangerous package hash can be warned about after trusted advisory refresh, but offline users cannot receive immediate revocation. Preserve that limitation in product claims.
