# Color, alpha, image normalization, and quality

## Color semantics

Keep encoded sRGB, linear-light RGB, relative luminance, alpha, and output encoding distinct.
`ascii-convert` exposes typed `EncodedSrgb`, `LinearRgb`, `Luminance`, `Alpha`, and
`OutputEncoding` boundaries; its owned normalized pixels are linear-premultiplied RGBA. For
normalized encoded sRGB channel `c`, decode with `c/12.92` at and below the exact `0.04045`
breakpoint and `((c+0.055)/1.055)^2.4` above it. Inverse encoding uses `12.92*c` at and below
`0.0031308`, and `1.055*c^(1/2.4)-0.055` above it. The rounded standard breakpoints are tested for continuity within `1e-7` absolute channel value. Linear relative luminance is
`Y=0.2126R+0.7152G+0.0722B`. Physical transfer conversion happens once; user-selected artistic
gamma is a separate later effect and must not be folded into transfer decoding or applied twice.
The current explicit output encoding is SDR sRGB at 8 bits per channel.

Unknown source profile fields may use an sRGB assumption for SDR input, but normalization returns
a material `AssumedSrgbForUnknownProfile` warning. This is not HDR detection or full color
management: explicit PQ/HLG input returns a typed unsupported-HDR failure, and explicit Display-P3
or Rec. 2020 primaries return a typed unsupported-wide-gamut failure. There is no implicit tone map,
wide-gamut conversion, clipping, or complete ICC-profile claim. The convenience
`convert_density` API preserves its `Error::Unsupported(ColorDescription)` contract, but its
display text directs callers to provide SDR sRGB input; `convert_density_detailed` retains
the exact profile-specific normalization failure.

## Image preprocessing order

Decode within limits and apply display orientation once in the decoder; then interpret source color metadata, decide alpha compositing background, select crop/contain/cover, resample, and apply artistic adjustments such as exposure, contrast, gamma, saturation, and edge emphasis. Distinguish physical color conversion from user-selected artistic distortion.

The IMG-001 `ascii-media` decoder applies display orientation once and returns top-down pixels, as described below. `ascii-convert` accepts these top-down raster inputs and never transforms orientation. The decoder rejects unsupported color metadata rather than converting profiles; `ascii-convert` then performs the linear-light normalization described above. A normalized raster is owned linear-premultiplied RGBA, and raster size, stride, input extent, output allocation size, work, and cancellation are checked before and during normalization.

`ascii-convert` sizes column grids from the selected source geometry: custom-crop mode uses the
crop's aspect rather than the full raster, while fixed-pixel surfaces retain floor division and
right/bottom leftovers. The detailed density-conversion result carries any material warning from
normalization (including the sRGB assumption for unknown profile fields) alongside the generated
frame; callers must surface that warning rather than discard it. Optional retained quality patches
are bounded by the shared raster/work budgets before allocation, and their zero-initialization is
charged to work and checked for cancellation at bounded intervals.

Premultiplied RGBA is scanned in a cancellable preflight before normalized output is allocated;
each pixel is charged once for that validation and once for normalization. Straight and opaque
inputs require only the normalization pass.

The normalizer's per-operation raster-byte ceiling covers the checked sum of the borrowed input's
minimum readable extent, `(height - 1) * row_stride + row_bytes`, and its requested owned normalized
pixel storage, `width * height * size_of::<LinearPremultipliedRgba>()`. It includes padding between
rows but excludes trailing bytes after the last pixel. The borrowed input extent is counted exactly
once beside the distinct output, even though the normalizer does not allocate the input buffer.
`Budget` remains an allocation-free per-operation ceiling; it records no shared or process-wide
usage.

`NormalizedRasterResources` is a caller-created, clonable shared owner with a separate ceiling for
concurrently retained normalized output. Normalization reserves the checked output bytes before
allocation, and the returned `NormalizedRaster` holds that RAII reservation until drop; failed
allocation, cancellation, and validation errors release it. The shared owner tracks only the
normalized output it owns, not the borrowed input extent already owned upstream. Density conversion
APIs take the same owner explicitly so callers can share one ceiling across concurrent operations.

Unused input backing capacity and allocator overhead or over-allocation are outside these logical
extent/request checks. The input extent is already included in the per-operation calculation and
must not be subtracted from the `Budget` passed to the normalizer.

## Alpha

`Alpha` is an independent linear value. Straight RGBA input keeps color unassociated until after
transfer decoding, then multiplies linear RGB by alpha. Premultiplied sRGB input is interpreted as
associated in the encoded sRGB domain: for nonzero alpha, unassociate there, decode to linear, and
premultiply in linear light. Premultiplied linear input is already associated in linear light.
Alpha-zero pixels always normalize to transparent black without division, and their hidden channel
bytes are ignored even for premultiplied input. For nonzero alpha, premultiplied channels above
alpha are rejected; nonfinite, out-of-range, and other invalid samples are rejected rather than
clipped.

Ordinary opaque wallpaper must be composited over its chosen opaque `LinearRgb` background in
linear light before matching. `NormalizedRaster::composited_over` provides those colors without an
intermediate allocation. Transparent layers instead use
`LinearPremultipliedRgba::composite_over_layer`, which applies source-over directly to premultiplied
linear RGB and returns the resulting alpha:
`out_rgb = src_rgb + dst_rgb * (1 - src_alpha)` and
`out_alpha = src_alpha + dst_alpha * (1 - src_alpha)`. The result can be composited over another
transparent layer without flattening or re-decoding sRGB; zero-alpha output is canonical transparent
black. Ordinary opaque wallpaper continues to use its existing opaque-background path.

## Palettes

`ConversionSettings::with_color_fit` adds color fitting to the existing density conversion; leaving it unset preserves density-only output. The validated palette modes are original sampled color, an explicit monochrome pair, an ordered fixed palette, canonical xterm ANSI-16/ANSI-256 colors, and a user gradient. `PaletteDistanceSpace::LinearRgb` compares squared Euclidean distance in linear-light RGB. `PaletteDistanceSpace::EncodedSrgb` compares normalized sRGB8 output bytes after quantization. Fixed and ANSI ties select the earliest palette entry.

Gradient stops are finite, lie in `[0, 1]`, are strictly increasing, and must include positions `0` and `1`. Positions are unique, while repeated colors at distinct positions are valid. Gradients interpolate in linear-light RGB. Linear-RGB matching projects onto every segment; encoded-sRGB matching uses bounded global search and returns a result within `1e-6` of the global minimum, retaining the earliest segment for ties within that search tolerance. If bounded work cannot establish the tolerance, resolution fails rather than returning an unchecked approximation. Empty, malformed, nonfinite, unordered, or over-limit definitions return typed errors.

Color fitting compares each glyph using its own mask and resolved colors. For row-major cell samples `C(p)` and byte coverage `M(p) = coverage(p) / 255`, it minimizes mean squared error across pixels and linear RGB channels for `R(p) = B + (F - B)M(p)`. Fixed-background fitting holds the requested background fixed (or resolves it to the selected palette) and solves the foreground. Two-color least squares solves each channel with foreground and background clamped to `[0, 1]`; empty, full, and near-degenerate masks deterministically use the sampled channel mean for both colors. A glyph is selected only after scoring its fitted colors, with stable glyph-index/Unicode tie-breaking.

Palette storage, source sampling, candidate fitting, and frame construction are preflighted against checked operation budgets; cancellation or a failed limit publishes no partial frame. Fixed palettes are capped at 4,096 colors, gradients at 128 stops, and one color-fit patch at 65,536 samples, with lower effective limits where the work budget requires them. A detailed conversion retains the palette kind and cache identity, selected strategy, and—when using ANSI palettes—a notice that names in the xterm table do not determine actual terminal appearance.

The frame stores explicit resolved sRGBA8 foreground and background colors, so downstream image encoders can consume those colors directly. This conversion contract does not implement a PNG exporter, GUI, terminal renderer, or promise of terminal appearance; the ANSI notice is metadata, not a display guarantee.

## Dithering

Density dithering is optional and defaults to `DitherMode::Off`, which preserves the existing
nearest-mean-coverage choice. Ordered and diffusion modes operate only on normalized linear-light
cell luminance after sampling and background compositing: the scalar target is
`clamp((Y - Y_background) / (Y_foreground - Y_background), 0, 1)`. It is not applied to encoded
bytes, individual RGB channels, retained quality-patch samples, or candidate-specific color-fit
scores; combining a non-off mode with color fitting is rejected.

`Ordered { pattern: Bayer8, seed }` adds a signed Bayer-8 threshold in coverage units with amplitude
`1 / max(glyph_count - 1, 1)`. The seed is mixed with a fixed SplitMix64 finalizer to select one
stable 8x8 phase; the phase depends on the seed and absolute output-cell coordinates, never frame
number, wall clock, randomness, or thread scheduling. Quantization chooses the nearest measured
glyph mean coverage, ties by glyph index then Unicode scalar. Thresholds are clamped to `[0,1]`.

`ErrorDiffusion { kernel: FloydSteinberg }` processes cells serially in row-major, left-to-right
order. It distributes the residual between adjusted target and chosen measured glyph coverage as
7/16 to the next cell, 3/16 to next-row left, 5/16 to next-row same-column, and 1/16 to next-row
right. Out-of-grid neighbors are discarded; adjusted values clamp to `[0,1]`; ties use the same
glyph rule. This dependency chain is deliberately not parallelized. Two `f64` row buffers are the
only diffusion scratch; their checked bytes join the conversion raster budget. The additional
dither operations, including row-buffer initialization and clearing, are charged to the operation
work budget and capped at 16,000,000 units. Cancellation is polled during scratch initialization,
row clearing, and conversion. Nonfinite residuals, checked-size overflow, allocation failure,
cancellation, or budget exhaustion fail without returning a partial frame.

The detailed result exposes a SHA-256 dither cache identity that includes mode, selected kernel or
pattern, and the ordered seed. Applications must combine it with source, glyph-table, sampling,
color, and conversion settings in their overall cache key.

### Retained synthetic comparison evidence

The regression test `synthetic_motion_and_scene_change_metrics_match_retained_evidence` compares
each mode against `Off` and asserts these deterministic values. The fixture is a 32×36 linear-gray
moving horizontal gradient (one source-pixel shift) followed by a 4×3 per-cell checker scene cut
using linear values 96/160; the output grid has 8×12-pixel cells. Tone MAE is the mean absolute
difference between each input cell's linear-gray mean and the selected glyph's measured mean
coverage. Flicker is changed output glyphs between the two gradient frames divided by 12. Glyph
deltas count cell positions different from `Off`; scene-cut carryover is the number of additional
same-scene conversions before the cut frame equals a fresh deterministic conversion (zero here).

| Mode | Gradient tone MAE | Gradient shift changes / flicker | Gradient glyph delta vs Off | Scene-cut tone MAE | Gradient→cut glyph changes | Cut glyph delta vs Off | Carryover frames |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Off | 0.32316176396082397 | 3 / 12 (0.25) | 0 | 0.26862744998698074 | 3 | 0 | 0 |
| Ordered Bayer-8, seed 23 | 0.32406045685406576 | 0 / 12 (0.0) | 1 | 0.26862744998698074 | 2 | 0 | 0 |
| Floyd–Steinberg | 0.3249591497473075 | 1 / 12 (0.08333333333333333) | 2 | 0.26862744998698074 | 1 | 0 | 0 |

Fixture SHA-256: `73c009fc5946e391fcca501e89a4cfe96aba7bb385eef19422302eb9609aca33`.
Configuration SHA-256: `9192795ae357a250499087e804163084cc222b03e0f17ab8c99369c8159a57c0`.
The exact hashes and metrics are also asserted and printed by the regression test. Verification
environment: Windows 10.0.26300.9974 x64, rustc 1.100.0, cargo 1.100.0. Exact evidence command:
`CARGO_HOME=D:/target/cargo-home CARGO_TARGET_DIR=D:/worktrees/.targets/cnv-006-dither cargo test --locked --package ascii-convert --test density_matcher -- --nocapture`.

This single synthetic fixture shows a slight gradient-tone error increase for both dither modes
alongside fewer changed glyphs; the scene-cut tone and mode-vs-Off glyph deltas happen to be equal
here. Zero carryover reflects frame-independent conversion, not perceptual lag or a temporal
quality guarantee. The small, synthetic comparison is evidence for the tested inputs only; it does
not claim still-image gains improve video or that dithering improves arbitrary scenes.

## Filtering and edges

Area/box sampling is a practical baseline for shrink-heavy conversion. Compare alternatives with thin
lines, checkerboards, text, and photographs. Edge enhancement should be bounded and operate before
quantization; strong enhancement can introduce halos. Dithering is optional and can increase
flicker in video unless its pattern is temporally stable.

## Input coverage

Start with PNG/JPEG and add WebP, GIF, APNG, and other formats only when the actual codec feature set and animation semantics are tested. Extensions are not codec guarantees. Check dimensions and allocation limits before large buffers; the image crate's limits are one layer of defense, not a substitute for process budgets. [S27](../research/source-register.md#s27) [S28](../research/source-register.md#s28)

### Image decoder implementation status (IMG-001)

`ascii-media::decode_image` sniffs PNG/JPEG bytes from the caller's current stream position; it
does not open paths or URLs. The stream is copied into a size-bounded, budgeted snapshot before
codec parsing so all validation and decode passes observe identical bytes. PNG handling uses
`png` 0.18.0 and validates every chunk CRC through an exact terminal IEND. JPEG handling uses
the strict, safe-mode pure-Rust `zune-jpeg` 0.5.5 decoder; it does not cross a native decoder
callback boundary. The `image` 0.25.10 dependency is used for orientation metadata and test
helpers, not as the PNG/JPEG decode backend.

Published rasters are top-down and tightly packed as 8-bit RGB or straight-alpha RGBA. Gray
samples expand to RGB; indexed PNG transparency expands to RGBA; 16-bit PNG samples are rounded
to 8 bits. PNG Adam7 and JPEG baseline/progressive inputs are supported. JPEG EXIF orientation
is applied before enforcing output dimensions. Untagged content is assumed sRGB; embedded ICC
profiles, APNG, unsupported JPEG color models/depths, and non-sRGB PNG `gAMA`/`cHRM` color
descriptions are rejected as explicit capability errors rather than silently mislabeled.

`DecodeLimits` carries the core per-operation `Budget`, decoder-local encoded/scratch limits,
dimensions, and a shared `DecodeResources` ledger for concurrent decode slots and retained
memory. The owned input snapshot, codec scratch and temporary rasters are preflighted and
charged before output allocation. PNG scratch includes row/inflater and reader buffers plus the
largest non-IDAT chunk buffer; large IDAT payloads are streamed and are not counted again as
codec scratch. Progressive JPEG scan-block work is budgeted separately from pixel work. Retained
output remains charged until its `DecodedFrame` is dropped; its `RasterFrameView` borrows from
that owner. Snapshot acquisition requests at most 128 bytes per caller `Read` and checks the
token between completed calls; cancellation cannot interrupt an underlying `Read` while it is
blocked. Codec compressed-input reads use a 128-byte buffer, and JPEG marker/entropy FF-fill
loops poll at 8 KiB offsets. Pixel normalization polls at each row boundary and after each
128 pixels within a row. These checks bound work intervals but are not CPU or I/O preemption:
codec work may continue until its next poll, and a caller's in-progress `Read` cannot be
interrupted until that call returns. Deterministic tests cover regular/Adam7 PNG, progressive JPEG
refills, cancellation inside both JPEG FF-fill loops, and cancellation partway through a one-row
grayscale PNG normalization pass.
Malformed input, cancellation, and failed budget checks release reservations without publishing
partial frames. Codec-provided allocation limits remain defense in depth; application
accounting is not an allocator-enforced process memory limit.

Rights-cleared generated fixtures exercise PNG color/depth/interlace variants, JPEG baseline
and progressive scans, malformed/truncated inputs, metadata policy, and budget/cancellation
paths. Their generated configuration and SHA-256 values are printed/asserted by the focused
tests. These fixture checks do not imply native-codec, performance, or platform-support claims.
