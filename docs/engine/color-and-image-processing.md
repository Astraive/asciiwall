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
wide-gamut conversion, clipping, or complete ICC-profile claim.

## Image preprocessing order

Decode within limits and apply display orientation once in the decoder; then interpret source color metadata, decide alpha compositing background, select crop/contain/cover, resample, and apply artistic adjustments such as exposure, contrast, gamma, saturation, and edge emphasis. Distinguish physical color conversion from user-selected artistic distortion.

The `ascii-media` decoder applies display orientation exactly once and returns top-down pixels. The `ascii-convert` color normalizer requires top-down raster input and never transforms orientation. Interpret supported source color metadata after decoding and before fit, crop, or resampling. A normalized raster is owned linear-premultiplied RGBA, and raster size, stride, input extent, output allocation size, work, and cancellation are checked before and during normalization.

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

The normalizer's raster-byte ceiling covers the checked sum of the borrowed input's minimum
readable extent, `(height - 1) * row_stride + row_bytes`, and its requested owned normalized pixel
storage, `width * height * size_of::<LinearPremultipliedRgba>()`. It includes padding between rows
but excludes trailing bytes after the last pixel. The borrowed input extent is counted once beside
the distinct output, although the normalizer does not allocate that input buffer. This is an
operation-local ceiling check: `Budget` stores numeric ceilings only, not a shared reservation or
process-wide usage. The returned normalized raster remains caller-owned and live after this check;
any limit across it and other concurrently retained storage belongs to a higher-level owner.
Unused input backing capacity and allocator overhead or over-allocation are outside this
extent/request check. The input extent is already included in this operation's calculation and must
not be subtracted from the budget passed to the normalizer.

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

Palette semantics are explicit and versioned in `ascii-convert`. `Original` preserves the supplied
linear-light foreground/background colors without quantization; for ordinary source-color cells,
pass the sampled cell color as foreground and the selected explicit background. `Monochrome` always
returns its configured pair. A fixed palette preserves caller order, permits duplicate colors, and
resolves equal nearest-color distances to the earliest entry. ANSI-16 and ANSI-256 resolve to the
canonical xterm color table (ANSI-16's standard base/bright values, ANSI-256's 6×6×6 cube and
24-step gray ramp). These are explicit sRGB colors for image output and palette labels for terminal
output, not a guarantee that a terminal uses those RGB values; `Palette::terminal_appearance_notice`
exposes that caveat.

A user gradient contains 2–128 stops with exact endpoints at positions 0 and 1. Stop positions must
be finite and strictly increasing; out-of-order and duplicate-position stops are rejected before
matching. Duplicate stop colors are allowed; equal linear-distance scores select the earliest
segment, and encoded-distance scores within the documented tie tolerance retain the earliest.
Colors interpolate linearly in linear-light RGB. Palette distance is independently configurable
as squared Euclidean linear-RGB or encoded-sRGB distance; linear-RGB gradient matches project
exactly onto each segment. Encoded-sRGB matching uses deterministic global branch-and-bound with
encoded-RGB interval lower bounds, not an assumption of unimodality. A fixed `1e-14` score margin is
subtracted from each bound to reduce floating-point overestimation. Exact requested-color matches
on a segment are returned exactly. The search stops when guarded bounds leave at most `5e-7`
between the best squared distance and the segment's global minimum. A later segment replaces the
current one only when its candidate score is lower by more than `5e-7`; the returned color remains
within `1e-6` of the global minimum, with exact ties retaining the earliest segment. Since `powf`
has no portable error bound, the margin is an engineering guard, not a formal proof over exact real
arithmetic. Each segment is limited to 56 subdivisions and 171 three-channel distance evaluations;
failure to establish the tolerance returns a work-budget error. Fixed palette ties select the
earliest palette entry.
`Palette::cache_identity` covers the distance choice, definition contents, and palette algorithm
version. Conversion/cache consumers also include `COLOR_FIT_ALGORITHM_VERSION` and the selected
`ColorFitStrategy`, so a fitting-policy change cannot reuse colors from a different algorithm.

`Palette::new(definition, distance_space, budget, cancellation)` validates the definition before
use. Fixed palettes retain at most 4096 colors and gradients at most 128 stops. User arrays are
copied into immutable owned storage charged against the supplied raster-byte budget; definition
validation, matching, and lookups have explicit work caps. No output color is silently substituted.
`Palette::resolve_pair(requested_foreground, requested_background, budget, cancellation)` is the
direct per-cell path: original preserves both requested colors, monochrome returns its configured
pair, and other modes resolve each requested color to a palette color. Each color resolution is
capped at 65,536 channel-work units and a pair at 131,072; the combined estimate must also fit the
caller's work budget. `PaletteColor` carries explicit linear-light RGB and, for fixed/ANSI modes,
the palette index. The shared conversion frame stores explicit sRGBA8 values; consumers should use
those values for PNG rather than re-resolving the palette. This workspace has no PNG/ANSI exporter;
the detailed conversion metadata below supplies the ANSI device-appearance notice.

Color reconstruction uses `R(p) = B + (F - B)M(p)` with byte coverage interpreted as `M/255`.
The Rust entry point is
`fit_color_patch(target: &[LinearRgb], coverage: &[u8], palette: &Palette, strategy: ColorFitStrategy, budget: &Budget, cancellation: &CancellationToken) -> Result<ColorFit, ColorFitError>`.
It receives row-major per-pixel linear-light RGB samples for one cell and the equally sized
row-major glyph coverage mask. Target colors must be opaque linear-light RGB with alpha already
composited under the established image-normalization contract. It returns the candidate-specific
resolved foreground/background and mean squared error across linear-light RGB channels. Candidate
comparison must fit and score every glyph with its own returned pair; `ColorFitStrategy::FixedBackground(background)` holds the background and solves/clamps the foreground.
`ColorFitStrategy::TwoColorLeastSquares` solves each channel jointly with foreground/background constrained to
`[0,1]`. Finite fixed/ANSI palettes search allowed colors; gradient fitting searches allowed
interpolated segments, so the score uses exactly the colors returned. Original mode solves
unrestricted colors; monochrome uses its explicit pair. Equal scores keep the first palette/segment
pair for deterministic ties.

The fit objective is linear-light RGB mean squared error, distinct from the configurable palette
lookup distance. A zero, full, or numerically singular/near-degenerate coverage mask has no reliable
two-color separation; per channel the stable fallback is equal foreground/background at the target
channel mean. Otherwise the bounded least-squares solution checks interior and box-boundary minima.
All output channels are finite and clamped to `[0,1]`. One fit accepts at most 65,536 patch samples
and 64,000,000 channel-work units; one palette resolution is capped at 65,536 channel-work units,
and resolving a foreground/background pair is capped at 131,072. Resolution work charges three
channel units per distance or interval-bound comparison: original uses 0, monochrome uses 6,
fixed/ANSI modes use 3 per palette color, linear gradients use 3 per segment, and encoded gradients
use at most 513 per segment (65,151 for the largest 128-stop gradient).
Use `estimate_color_fit_work` for every shortlisted glyph and checked-add estimates against the whole
job budget before reranking multiple candidates.

Color fitting is integrated into the shared density consumer through
`ConversionSettings::with_color_fit(&Palette, ColorFitStrategy)` and
`convert_density_detailed`. The consumer uses the established normalized raster and sampler to
produce one bounded, reusable row-major linear-RGB patch for each cell, then fits and scores every
glyph with that glyph's own coverage mask and corresponding foreground/background solution. It
stores the selected explicit sRGBA8 pair in the resulting `AsciiCell`; it does not first choose a
glyph by brightness and recolor it afterward. The temporary RGB patch is not retained. Optional
`QualityPatches` continue to hold luminance and finite-difference descriptors, sampled independently
when requested.

`DensityConversion` exposes palette kind, cache identity, fit strategy, and the ANSI
terminal-appearance notice alongside the actual `AsciiFrame`. Use the detailed result when an
exporter needs to propagate that metadata; the convenience `convert_density` returns the frame
only. ANSI palette colors are explicit engine sRGB values, not a promise of identical physical
appearance across terminal devices.

### CNV-005 regression evidence

The synthetic conversion regression uses `tests/support/synthetic_font.rs` with
`synthetic_font(false)`, repertoire `BA `, 8×12 glyph cells, baseline 9, RGB8 sRGB opaque
top-down input, and explicit original/monochrome/fixed/ANSI-16/ANSI-256/gradient modes. The test
asserts the source-font fixture SHA-256
`8a0b3b4e8ee150d6d16b084e51200525112ed6424855afa652a55ed3274176c8` and configuration descriptor
SHA-256 `c5784309d79a7a1116319a718da3465cf6ffa032de7ab22f4b51c40655e4ebed` in
`tests/color_fit.rs`; changing either fixture or configuration requires updating the golden
evidence.

Focused executable evidence:

```text
cargo test -p ascii-convert --test color_fit reconstruction -- --nocapture
cargo test -p ascii-convert --test color_fit palette_modes -- --nocapture
cargo test -p ascii-convert --test color_fit validation -- --nocapture
cargo test -p ascii-convert --test color_fit gradient_pair_fit -- --nocapture
cargo test -p ascii-convert --test color_fit synthetic_color_fit_fixture_configuration_hash -- --nocapture
```

Workspace acceptance commands:

```text
cargo test -p ascii-convert --test color_fit
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The synthetic conversion tests exercise the real `convert_density_detailed` consumer: candidate
glyph selection by paired reconstruction error, two independently colored cells, all six palette
modes and ANSI notices, preflight rejection, in-flight cancellation without a published frame, and
successful reuse after cancellation. No separate CLI or image-exporter crate exists in this
workspace.


## Filtering and edges

Area/box sampling is a practical baseline for shrink-heavy conversion. Compare alternatives with thin lines, checkerboards, text, and photographs. Edge enhancement should be bounded and operate before quantization; strong enhancement can introduce halos. Dithering is optional and can increase flicker in video unless its pattern is temporally stable.

## Input coverage

Start with PNG/JPEG and add WebP, GIF, APNG, and other formats only when the actual codec feature set and animation semantics are tested. Extensions are not codec guarantees. Check dimensions and allocation limits before large buffers; the image crate's limits are one layer of defense, not a substitute for process budgets. [S27](../research/source-register.md#s27) [S28](../research/source-register.md#s28)

The `ascii-media` decoder accepts caller-owned seekable streams and sniffs PNG/JPEG content; it does not open paths or URLs or select a codec from a suffix. It bounds the encoded extent from the stream's current position, applies display orientation once, and returns owned top-down RGB/RGBA with 8-bit samples labeled sRGB. If the image decoder reports an embedded ICC profile, `ascii-media` rejects the input rather than interpreting or converting that profile. The sRGB label does not establish ICC/profile conversion or guarantee wide-gamut, HDR, or high-precision color management. Unsupported formats and malformed encoded data remain distinct errors. Codec-local image limits supplement, but do not replace, the caller's pixel/work budget.
