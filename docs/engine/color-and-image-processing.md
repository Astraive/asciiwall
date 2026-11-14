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

The normalizer's raster-byte ceiling covers the checked sum of the borrowed input's minimum
readable extent, `(height - 1) * row_stride + row_bytes`, and its requested owned normalized pixel
storage, `width * height * size_of::<LinearPremultipliedRgba>()`. It includes padding between rows
but excludes trailing bytes after the last pixel. The view is not a second allocation: its extent
is counted once alongside the distinct output. This is an operation-local ceiling check, not a
shared reservation or a process-wide memory cap. Pipeline callers must account for any additional
concurrently live storage, but must not subtract this input extent from the budget passed to the
normalizer. Unused backing capacity and allocator overhead or over-allocation are outside this
extent/request-based check.

## Alpha

`Alpha` is an independent linear value. Straight RGBA input keeps color unassociated until after
transfer decoding, then multiplies linear RGB by alpha. Premultiplied sRGB input is interpreted as
associated in the encoded sRGB domain: for nonzero alpha, unassociate there, decode to linear, and
premultiply in linear light. Premultiplied linear input is already associated in linear light.
Alpha-zero pixels always normalize to transparent black without division, so invisible RGB cannot
affect averages. Nonfinite, out-of-range, and invalid premultiplied samples are rejected rather
than clipped.

Ordinary opaque wallpaper must be composited over its chosen opaque `LinearRgb` background in
linear light before matching. `NormalizedRaster::composited_over` provides those colors without an
intermediate allocation. Transparent layered paths retain linear-premultiplied channels and alpha
until their layers are composited.

## Palettes

Support original per-cell color, monochrome foreground/background, fixed palettes, ANSI-16, ANSI-256, and user gradients. Palette matching space is a configurable algorithm choice; do not confuse a named terminal palette with the actual colors a user's terminal may display. PNG export uses explicit colors, while ANSI export is subject to terminal interpretation.

Background/foreground separation can be a single fixed background for speed or a two-color cell fit for quality. The latter must evaluate the character mask and color solution together; selecting a glyph only by average brightness then changing both colors can undermine its meaning.

## Filtering and edges

Area/box sampling is a practical baseline for shrink-heavy conversion. Compare alternatives with thin lines, checkerboards, text, and photographs. Edge enhancement should be bounded and operate before quantization; strong enhancement can introduce halos. Dithering is optional and can increase flicker in video unless its pattern is temporally stable.

## Input coverage

Start with PNG/JPEG and add WebP, GIF, APNG, and other formats only when the actual codec feature set and animation semantics are tested. Extensions are not codec guarantees. Check dimensions and allocation limits before large buffers; the image crate's limits are one layer of defense, not a substitute for process budgets. [S27](../research/source-register.md#s27) [S28](../research/source-register.md#s28)

The `ascii-media` decoder accepts caller-owned seekable streams and sniffs PNG/JPEG content; it does not open paths or URLs or select a codec from a suffix. It bounds the encoded extent from the stream's current position, applies display orientation once, and returns owned top-down RGB/RGBA with 8-bit samples labeled sRGB. If the image decoder reports an embedded ICC profile, `ascii-media` rejects the input rather than interpreting or converting that profile. The sRGB label does not establish ICC/profile conversion or guarantee wide-gamut, HDR, or high-precision color management. Unsupported formats and malformed encoded data remain distinct errors. Codec-local image limits supplement, but do not replace, the caller's pixel/work budget.
