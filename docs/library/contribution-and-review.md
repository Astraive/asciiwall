# Wallpaper contribution and review policy

## Submission requirements

A contribution includes the declarative scene or permitted source media, strict package metadata, reproducible build inputs, a static preview, license/attribution records for every asset, intended capabilities, minimum engine features, and a low-power or noninteractive fallback where practical.

Do not accept a wallpaper that only works because the author installed an undeclared font, has a private file path, or ran a hidden script. All portable dependencies must be explicit and redistribution-authorized.

## Automated gates

Validate metadata and paths; scan inventory and licenses; build with the pinned engine packer in an isolated job; validate the output archive; render bounded previews; compare expected dimensions; enforce source size and compute budgets; verify deterministic output where promised; and check that network-disabled playback succeeds.

No contributor-provided arbitrary build script runs during this path. A failed thumbnail is an error, not a reason to execute the media in the CI controller process. Apply CPU/memory/time limits to every renderer/decoder job.

## Human review

Review visual quality, legibility, default motion intensity, reduced-motion behavior, attribution plausibility, original asset origin, optional permission explanations, and graceful behavior without observers. Reviewers should test at least one narrow/portrait layout and one low-power setting for featured packages.

A copyright declaration is necessary but not sufficient proof. Do not accept branded movie/game clips simply because their pixels have been converted to text. Mark any uncertain rights case for clarification rather than laundering it through a permissive package license.

## Privilege separation in CI

Untrusted PR validation has no signing key, release token, or privileged write credentials. Never execute untrusted PR checkout under `pull_request_target` with secrets. [S43](../research/source-register.md#s43) Publishing occurs from reviewed, protected commits through a separate trusted workflow.

Static data can still exploit parsers, so an unprivileged CI renderer remains sandboxed and resource-limited. Only validated artifacts cross into the release job; the trusted job verifies their hashes and provenance again.

## Moderation and maintenance

Record a maintainer, package identity, author attribution, and compatibility status. Deprecation means the catalog stops recommending or offering an entry, with a reason. Security advisories distinguish dangerous payloads from merely unsupported or visually poor content.

A user may keep a local package even if it leaves the curated gallery. The application can warn about a known compromised hash when updated metadata is available, but it cannot guarantee instant revocation while offline and should not delete local files without consent.

## Acceptance checklist

A reviewer can reproduce the package, confirm all assets are declared, inspect license evidence, run without network/optional feeds, exercise Apply/Stop, and identify no default input interception or excessive resource use. These are release criteria; this documentation bundle does not contain a reviewed community collection.
