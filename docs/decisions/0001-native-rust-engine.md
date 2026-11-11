# ADR 0001 - Own Rust engine and shared frame model

## Status

Proposed, accepted for this design; not implemented.

## Context and options

The user wants an original open-source engine, offline processing, native desktop behavior, and reusable image/video conversion. Options were a webview wallpaper wrapper, a thin wrapper around an existing symbol renderer, or an owned Rust conversion/scene/rendering stack with third-party low-level codecs and graphics abstractions.

## Decision

Own the ASCII mapping, glyph analysis, temporal processing, scenes/effects, frame model, scheduling, and package contracts in Rust. Use `image`/FFmpeg to decode media, `fontdue` or a validated replacement to rasterize fonts, and wgpu for the default presenter. Existing tools/research are prior art and comparison baselines, not an engine implementation dependency. [S24](../research/source-register.md#s24) [S26](../research/source-register.md#s26) [S27](../research/source-register.md#s27) [S38](../research/source-register.md#s38) [S39](../research/source-register.md#s39)

The desktop app and CLI consume the same libraries. Procedural scenes may create character frames directly; raster sources pass through the converter. Rendered video remains a distinct noneditable representation.

## Consequences

More engineering work than wrapping a player, but clear ownership of quality/performance and a reusable core. We do not reinvent video codecs, OS window systems, cryptography, or GPU drivers. A Rust implementation does not automatically establish performance or safety; native dependencies and FFI remain review targets.

## Revisit when

A concrete benchmark or maintenance issue shows a selected low-level library is unsuitable. Replace the adapter dependency without abandoning the shared core contracts. Do not switch to a web runtime simply because a prototype UI is easier unless the product constraints are explicitly reconsidered.
