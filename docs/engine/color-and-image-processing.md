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

The IMG-001 `ascii-media` decoder applies display orientation once and returns top-down pixels, as described below. `ascii-convert` accepts these top-down raster inputs and never transforms orientation. The decoder rejects unsupported color metadata rather than converting profiles; `ascii-convert` then performs the linear-light normalization described above. A normalized raster is owned linear-premultiplied RGBA, and raster size, stride, input extent, output allocation size, work, and cancellation are checked before and during normalization.

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

Support original per-cell color, monochrome foreground/background, fixed palettes, ANSI-16, ANSI-256, and user gradients. Palette matching space is a configurable algorithm choice; do not confuse a named terminal palette with the actual colors a user's terminal may display. PNG export uses explicit colors, while ANSI export is subject to terminal interpretation.

Background/foreground separation can be a single fixed background for speed or a two-color cell fit for quality. The latter must evaluate the character mask and color solution together; selecting a glyph only by average brightness then changing both colors can undermine its meaning.

## Filtering and edges

Area/box sampling is a practical baseline for shrink-heavy conversion. Compare alternatives with thin lines, checkerboards, text, and photographs. Edge enhancement should be bounded and operate before quantization; strong enhancement can introduce halos. Dithering is optional and can increase flicker in video unless its pattern is temporally stable.

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
loops poll at 8 KiB offsets. This is bounded input/parser cancellation, not CPU preemption:
work over codec-buffered input or between JPEG polls may continue until the next check.
Deterministic tests cover regular/Adam7 PNG, progressive JPEG refills, and cancellation inside
both JPEG FF-fill loops.
Malformed input, cancellation, and failed budget checks release reservations without publishing
partial frames. Codec-provided allocation limits remain defense in depth; application
accounting is not an allocator-enforced process memory limit.

Rights-cleared generated fixtures exercise PNG color/depth/interlace variants, JPEG baseline
and progressive scans, malformed/truncated inputs, metadata policy, and budget/cancellation
paths. Their generated configuration and SHA-256 values are printed/asserted by the focused
tests. These fixture checks do not imply native-codec, performance, or platform-support claims.
