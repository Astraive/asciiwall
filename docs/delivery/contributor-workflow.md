# Contributor workflow and engineering conventions

## Getting oriented

Read the root guide, architecture contracts, relevant subsystem spec, and task plan before editing. Start with a small independently testable issue. Do not create every planned crate/module just to match the target tree; boundaries should be exercised by real behavior.

Rust code follows standard module/type naming; documentation filenames/folders are lowercase kebab-case. Public APIs include ownership, cancellation, error, and resource-limit behavior. Native unsafe code stays in reviewed adapter modules with explicit safety invariants.

## Development loop

Write a focused failing test, verify its intended failure, implement the minimal complete behavior, run the targeted and affected regressions, inspect the diff, and commit the work unit. For native behavior, include the manual reproduction and exact environment rather than a mock-only claim.

A new feature includes requested/effective settings behavior, capability denial, reduced-motion/power interaction, cleanup, and documentation. A bug fix includes a minimized regression fixture where possible. Never commit personal media or proprietary fonts to reproduce a failure.

## Architectural boundaries

Core crates must not import UI/native/network dependencies for convenience. FFmpeg-specific ownership does not leak into `ascii-core`. Packages do not execute code. Shell bridges do not decode untrusted media. The optional downloader cannot be invoked by a scene node.

Changing a shared frame/settings/package/IPC contract requires updating every consumer and the conformance fixtures. Keep format migrations separate from unrelated visual refactoring. Introduce new capability flags instead of silently changing the meaning of existing ones.

## Review checklist

Is input validated before allocation/native calls? Are buffers/handles owned long enough? Are queues bounded? Does cancellation leave partial state? Can stale generations overwrite current work? Does this collect unnecessary information? Does it work without optional permissions/network? Does it preserve user originals and desktop input?

Security-sensitive parser/FFI/IPC/release changes need additional review. Benchmarks include quality comparison and named hardware. An optimization that changes output must document its tradeoff.

## Issue reports and discussion

Use sanitized `doctor` output, clear expected/actual behavior, exact OS/compositor/GPU, reproduction steps, and rights-cleared minimal fixtures. A missing capability is a feature/support limitation, not automatically a bug. Keep private source files and full paths out of public logs.

## Community art contributions

The second repository follows the separate rights/review policy. Original procedural recipes are encouraged. A contribution does not need an online account inside the app; ordinary open-source repository workflows handle review. No paid backend or hosted content service is a project prerequisite.

## Documentation upkeep

Every published claim about support, performance, compatibility, or security links to evidence. Research snapshots expire; refresh dependency and platform facts when preparing a release. Keep open questions visible rather than burying uncertainty under an enthusiastic feature list.
