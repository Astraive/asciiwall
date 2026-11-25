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

Support original per-cell color, monochrome foreground/background, fixed palettes, ANSI-16, ANSI-256, and user gradients. Palette matching space is a configurable algorithm choice; do not confuse a named terminal palette with the actual colors a user's terminal may display. PNG export uses explicit colors, while ANSI export is subject to terminal interpretation.

Background/foreground separation can be a single fixed background for speed or a two-color cell fit for quality. The latter must evaluate the character mask and color solution together; selecting a glyph only by average brightness then changing both colors can undermine its meaning.

## Filtering and edges

Area/box sampling is a practical baseline for shrink-heavy conversion. Compare alternatives with thin lines, checkerboards, text, and photographs. Edge enhancement should be bounded and operate before quantization; strong enhancement can introduce halos. Dithering is optional and can increase flicker in video unless its pattern is temporally stable.

## Input coverage

This build implements PNG/JPEG and feature-gated still WebP; add GIF, APNG, and other formats
only when the exact codec feature set and animation semantics are tested. Extensions are not
codec guarantees. Check dimensions and allocation limits before large buffers; codec limits are
one layer of defense, not a substitute for process budgets. [S27](../research/source-register.md#s27) [S28](../research/source-register.md#s28)

### Image decoder implementation status (IMG-001)

`ascii-media::decode_image` sniffs PNG/JPEG/WebP bytes from the caller's current stream position;
it does not open paths or URLs. The stream is copied into a size-bounded, budgeted snapshot
before codec parsing so all validation and decode passes observe identical bytes. PNG handling
uses `png` 0.18.0 and validates every chunk CRC through an exact terminal IEND. JPEG handling
uses the strict, safe-mode pure-Rust `zune-jpeg` 0.5.5 decoder; it does not cross a native
decoder callback boundary. Still WebP uses the direct `image-webp` 0.2.4 decoder behind the
`ascii-media/webp` feature; the `image` 0.25.10 dependency remains for PNG/JPEG and orientation
metadata.

Published rasters are top-down and tightly packed as 8-bit RGB or straight-alpha RGBA. Gray
samples expand to RGB; indexed PNG transparency expands to RGBA; 16-bit PNG samples are rounded
to 8 bits. PNG Adam7, JPEG baseline/progressive, and still WebP lossy/lossless with alpha are
supported when their build capabilities are enabled. JPEG and WebP EXIF display orientation is
applied before enforcing output dimensions. Each `DecodedFrame` carries a copyable,
allocation-free `ColorDescription`: accepted sRGB primaries/transfer interpretation, actual alpha
semantics, compact source-profile provenance, and source range metadata.

`DecodeLimits` carries the core per-operation `Budget`, decoder-local encoded/scratch limits,
dimensions, and a shared `DecodeResources` ledger for concurrent decode slots and retained
memory. The owned input snapshot, codec scratch and temporary rasters are preflighted and
charged before output allocation. PNG scratch includes row/inflater and reader buffers plus the
largest non-IDAT chunk buffer; large IDAT payloads are streamed and are not counted again as
codec scratch. JPEG scan-block work and WebP pixel/chunk work are budgeted with encoded-input
work. WebP scratch reserves 512 KiB fixed, 32 KiB per source pixel, and two encoded-input
extents; EXIF data is borrowed from the owned snapshot rather than copied. Retained output
remains charged until its `DecodedFrame` is dropped; its `RasterFrameView` borrows from that
owner. Snapshot acquisition requests at most 128 bytes per caller `Read` and checks the token
between completed calls; cancellation cannot interrupt an underlying `Read` while it is blocked.
Codec compressed-input reads use a 128-byte buffer, and JPEG marker/entropy FF-fill loops poll
at 8 KiB offsets. WebP container scans poll between chunk headers. Pixel normalization polls at
each row boundary and after each 128 pixels within a row. These checks bound work intervals but
are not CPU or I/O preemption: codec work may continue until its next poll, and a caller's
in-progress `Read` cannot be interrupted until that call returns. Malformed input, cancellation,
and failed budget checks release reservations without publishing partial frames.
Codec-provided allocation limits remain defense in depth; application accounting is not an
allocator-enforced process memory limit.

Rights-cleared generated fixtures exercise PNG color/depth/interlace variants, JPEG baseline and
progressive scans, and WebP lossy/lossless RGB and RGBA alpha plus animated-input rejection.
Their generated configuration and SHA-256 values are printed/asserted by focused tests. The
WebP fixtures are generated with Pillow 12.2.0/libwebp 1.6.0. Settings
`pillow=12.2.0;libwebp=1.6.0;size=3x2;rgb=255,0,0|0,255,0|0,0,255|255,255,255|64,128,192|12,34,56;rgba=255,0,0,0|0,255,0,64|0,0,255,128|255,255,255,192|64,128,192,255|12,34,56,100;lossy=quality75,method6;lossless=quality100,method6;lossy_alpha=exact;animation=2frames,100ms,loop0`
have SHA-256 `5a96886b8f939d20359a0cf06da1b79385170b048c1274427839ff029d4c9999`; lossy RGB
`6572694ee1d61d75c576ac2a06814aad5d22bc764dbad28dbf80cf9d4c52a14e`, lossless RGB
`c5328c391d6e687347f7700507725d74544f36dadadf4042dcb2bb2d2133bda2`, lossy RGBA
`531d0a9bd04a054844397ec748604127c91e45545a8f32aff13331a9591e75e3`, lossless RGBA
`7e0d46bb4497d2c65804e12947b5fb2ac0d621108979fccd14bc8ce91d4e52c9`, animated WebP
`441f56c14aa8f83a84bf7f57c6624834b22de219cc6f18b363c8590e24de5280`.
The EXIF orientation fixture uses settings `pillow=12.2.0;libwebp=1.6.0;webp=lossless-rgb-3x2;exif=TIFF-MM;IFD0.Orientation=SHORT,count1,value6`,
SHA-256 `6e99fb4f37eedf4c8ce8c3cc125c84baa0a5f278909fb6b9ced55da47becef6b`; fixture
SHA-256 `9de5e8fc73091d2ade011874871beb6c6a37110faf7e839269e3a0a8d83eb6e5`.
These fixture checks do not imply native-codec, performance, or platform-support claims.
### Image capability contract (IMG-002)

`ascii-media::image_capabilities()` reports this build's static decoder variants: still PNG,
baseline/progressive JPEG, and, with the default `webp` feature, still WebP lossy/lossless and
alpha variants. APNG and animated WebP are disabled; APNG support is represented solely by
`png.animated`. The pinned direct `image-webp` 0.2.4 decoder inspects animation before pixel
decode and returns `Unsupported(AnimatedImage)` rather than publishing the first frame. A build
without the `webp` feature reports all WebP still variants disabled.

JPEG display orientation is applied once while producing the normalized top-down raster. The
returned `RasterFrameView` carries `RasterOrientation::TopDown`; consumers must use that pixel
orientation and must not apply source EXIF orientation again. When Orientation is absent, identity
is used; malformed Orientation tags and malformed ExifIFD pointers return
`Malformed(InvalidEncodedData)` rather than being silently ignored or treated as absent. The eight
generated asymmetric orientation fixtures and expected output hashes are maintained in
`image_decode_jpeg_controls.rs`.

The repository currently has no CLI doctor or import-error surface to consume the capability
report. Those user-facing diagnostics remain blocked until those product surfaces exist; callers
can query the media-library API in the meantime.

For accepted PNG, JPEG, and WebP, `ColorDescription::source_profile()` distinguishes observed
sRGB metadata from untagged data handled under the documented sRGB assumption. PNG sample range
is full; JPEG and WebP source range is unspecified because these decoders have no authoritative
range tag. WebP alpha is preserved as straight alpha. Untagged values are not claimed to have
undergone profile conversion. Embedded ICC, uncalibrated/unknown EXIF color space, non-sRGB PNG
`gAMA`/`cHRM`, and unsupported transfer/HDR metadata fail with typed color-description errors;
no ICC or HDR conversion accuracy is claimed.

Animated-image routing remains blocked: no animation handler exists in the current repository.
The static decoder classifies animated WebP as `Unsupported(AnimatedImage)` before `read_image`,
so no first frame can enter the still-image path. A future IMG-003 integration must handle that
typed signal and route frames through timeline/composition, disposal/blend, cancellation, and
bounded-history semantics.
