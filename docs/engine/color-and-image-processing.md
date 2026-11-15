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

An `ascii-media` decoder is planned to apply display orientation exactly once and return top-down pixels; that decoder is not implemented in this PR. The existing `ascii-convert` normalizer accepts top-down raster inputs and never transforms orientation. The planned pipeline should interpret supported source color metadata after decoding and before fit, crop, or resampling. A normalized raster is owned linear-premultiplied RGBA, and raster size, stride, input extent, output allocation size, work, and cancellation are checked before and during normalization.

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

Future `ascii-media` decoder contract (not implemented by this PR): a decoder should accept caller-owned seekable streams and sniff PNG/JPEG content; it should not open paths or URLs or select codecs from suffixes. It should bound the encoded extent from the stream's current position, apply display orientation once, and return owned top-down RGB/RGBA with 8-bit samples labeled sRGB. If the image decoder reports an embedded ICC profile, the future decoder should reject the input rather than interpret or convert that profile. The sRGB label must not imply ICC/profile conversion or guarantee wide-gamut, HDR, or high-precision color management. Unsupported formats and malformed encoded data should remain distinct errors. Codec-local image limits should supplement, but not replace, the caller's pixel/work budget. This PR implements only the `ascii-convert` normalization behavior described above; it does not include an `ascii-media` decoder.
