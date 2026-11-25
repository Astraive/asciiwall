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

Plain-text output contains only supported printable glyphs and structural LFs; unsafe glyph-table entries are rejected rather than copied into output. ANSI output emits only engine-generated approved color sequences and printable glyph data. Package metadata is never printed as raw escape codes. Do not embed OSC hyperlinks, clipboard sequences, terminal title changes, or arbitrary control strings from user content.

`ascii-render` implements headless `.txt` and `.ans` serialization from an `ascii-core` frame and its bound `ascii-convert` glyph table. It returns a complete owned byte buffer, logical `GridSize`, and static target-loss notice; it does not choose or write a destination. Frame/table identity, glyph indices, and glyph safety are rechecked at the export boundary. Metadata, identifiers, and source names are not serialized.

Both targets preserve one glyph per grid column, use LF between rows, and include exactly one final LF. Glyph records carry individual Unicode scalars, not strings. The strict repertoire is printable ASCII (`U+0020`–`U+007E`), block elements (`U+2580`–`U+259F`), and Braille patterns (`U+2800`–`U+28FF`); C0/C1 controls, bidi formatting controls, combining marks, and multi-cell characters are rejected. Multi-scalar strings cannot be represented by the shared glyph-record type. Export rechecks each glyph through `ascii_convert::is_supported_single_cell_scalar`, the shared public single-cell predicate, in addition to the table's validated repertoire.

ANSI emits only engine-generated SGR color sequences (`ESC [` … `m`), begins with `ESC[0m` to clear inherited styles, and resets with `ESC[0m` before each row's LF. Supported resolved modes are ANSI16, xterm-256, and truecolor; other palette modes are not represented by the API. ANSI16 uses its conventional 16-color palette. ANSI256 selects from those 16 colors, the xterm 6×6×6 cube, and the 24-step grayscale ramp. Palette modes choose the nearest entry in encoded-sRGB RGB distance, breaking ties by lower palette index; truecolor emits direct encoded-sRGB channel values after alpha compositing. The background is composited over opaque black, then the foreground over that flattened background, using deterministic encoded-sRGB rounding; no output mode retains alpha. ANSI16 and ANSI256 quantize both resulting colors, and ANSI256's first 16 entries may reflect terminal-configured colors. Target notices disclose source-font and motion/live-effect loss and viewer/terminal cell-aspect dependence; ANSI notices also report alpha flattening, palette quantization where applicable, and terminal-palette dependence for indexed modes.

Each export checks the complete output length against `Budget::max_frame_bytes`, checks work against `Budget::max_work_units`, polls the shared cancellation token, and returns typed errors without exposing partial bytes. For `n` cells and `g` glyph entries, estimated work is `3n + g` for text and truecolor, `67n + g` for ANSI16, and `75n + g` for ANSI256. These bounds include a frame-validation cell scan, a size-pass scan, a serialization scan, glyph validation, and palette candidate-distance evaluations in both ANSI encoding passes (16 candidates per ANSI16 color and 18 per ANSI256 color). VID-014 owns the shared destination transaction; the CLI-001 integration uses this serializer for headless monochrome density `.txt`/`.ans` conversion and publishes through its same-directory helper. PNG still rendering remains unavailable pending VID-011.

### Focused export-boundary guard verification

Command:

```sh
CARGO_HOME='D:/.worktrees/asciiwall/cargo-home-vid010' CARGO_TARGET_DIR='D:/.worktrees/asciiwall/target-vid010-export-guard-tests' cargo +1.100.0 test --locked -p ascii-render --lib export::tests::invalid_export_glyphs_return_indexed_errors -- --exact
```

Result: 1 passed (1 suite, 2 filtered, 0.01s).

Environment: Windows 11 build 10.0.26300.9974, x86_64; `rustc 1.100.0 (0ef1bc386 2026-11-09)`, host `x86_64-pc-windows-msvc`; Cargo `1.100.0 (3acc79c0c 2026-11-05)`.

SHA-256:
- `crates/ascii-render/src/export.rs` (validator and focused test): `f70ad4cafdc3234c550ddc1583a63e822721cac1e6ec20ee148d16aaa8f03bfb`
- `crates/ascii-render/tests/support/synthetic_font.rs` (included unit-test fixture source): `4a1baa8158dee1addcae7a7edafeee1c5bdecc3e1ad51e07cee0b597717fd34f`

## Package export

Portable export embeds every required redistributable asset and attribution. It does not merely rename a project folder or a video. Relink/replace missing or restricted assets before packing. The package exporter and importer share the same schema and validation rules, and every export should pass a fresh import validation test.
