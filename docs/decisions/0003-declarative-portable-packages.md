# ADR 0003 - Declarative .asciiwall packages

## Status

Proposed draft format 1.0.

## Options

An arbitrary project folder with scripts, a self-contained rendered video, or a validated ZIP containing metadata, built-in scene graphs, and local assets. Arbitrary scripts create a much larger security problem; video alone loses editable character structure and interactive controls.

## Decision

Use a bounded, versioned `.asciiwall` ZIP with one strict manifest, complete asset inventory, per-asset hashes/licenses, and explicit source representation. Stable v1 accepts data-only built-in graphs, not executable plugins or custom shaders. Required/optional capabilities are declarative and cannot grant permissions.

Import validates in private staging and commits immutable bytes. User overrides are separate. Export resolves portable assets and excludes local private settings. Signed catalogs authenticate distributed artifact selection; unsigned local imports remain possible after safety checks.

## Consequences

The packer/importer require careful path, parser, resource, compatibility, and licensing work. JSON Schema alone does not provide archive safety. [S35](../research/source-register.md#s35) [S36](../research/source-register.md#s36) Advanced arbitrary code extensions are deferred to a separate security design.

The format can represent procedural, image, video, character clip, and baked video without pretending they share identical editing power. Future changes need explicit version/feature negotiation.

## Revisit when

Real creator needs cannot be expressed by reviewed built-in nodes. Extend the declarative registry first; only then assess a metered isolated extension ABI. Do not weaken import boundaries to make one special wallpaper work.
