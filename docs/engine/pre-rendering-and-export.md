# Pre-rendering, baking, and export behavior

## Export targets

| Target | Preserves | Loses or restricts |
| --- | --- | --- |
| `.txt` | printable character grid | font, colors, motion, exact aspect on arbitrary viewers |
| `.ans` | approved ANSI color codes + characters | consistent terminal palette/font, live effects |
| PNG | exact rasterized still under chosen renderer | editability and motion |
| SVG | optional outlined/raster-safe still in a later phase | font portability unless outlines are used; added attack surface |
| `.ascii-clip` | glyph ids, cell colors, timestamps | original source detail and arbitrary grid resampling |
| video | rendered appearance and timing | conversion-level editing and live interaction |
| `.asciiwall` | portable recipe/assets/licenses | machine-specific permissions and assignments |

SVG is a future export option, not a default import path. Avoid accepting arbitrary external-resource SVG documents without a separate safe rasterization design.

## Bake specification

A bake job records input package/project digest, output resolution, cell grid, font identity, algorithm version, seed, start/end time, rational FPS, renderer backend, color target, effect settings, and interaction policy. Export uses a deterministic synthetic timeline rather than wall-clock scheduling.

For live interactions choose: no interaction, a synthetic demonstration, or an explicitly supplied event track. Do not capture the user's actual desktop/window activity automatically. Rendered video gets a notice that font and density are baked.

## Codec policy

List actual available encoders and supported containers from the shipped build. Do not advertise H.264, AV1, alpha video, or lossless encoding solely because a container name is available. Codec/library choices and redistribution conditions need separate review. [S33](../research/source-register.md#s33)

Keep audio off unless the user selects an explicit supported audio mode. Explain whether export drops, retains, or re-encodes audio. A silent visual-only release is acceptable when clearly specified.

## Execution

Run exports as cancellable low-priority jobs with bounded memory and temporary output. For each timestamp, evaluate simulation, conversion if required, effects, render, read back only what is needed, and encode. Do not try to run export faster by skipping stateful simulation steps.

Write a temporary file in the destination filesystem, validate the completed result, then rename atomically when supported. Cancellation must not overwrite an existing output. A forced overwrite flag must be explicit in CLI and confirmed in UI.

## Terminal safety

Plain-text export removes control bytes and terminal escape sequences. ANSI export emits only engine-generated approved color sequences and printable glyph data; package metadata is never printed as raw escape codes. Do not embed OSC hyperlinks, clipboard sequences, terminal title changes, or arbitrary control strings from user content.

## Package export

Portable export embeds every required redistributable asset and attribution. It does not merely rename a project folder or a video. Relink/replace missing or restricted assets before packing. The package exporter and importer share the same schema and validation rules, and every export should pass a fresh import validation test.
