# Color, alpha, image normalization, and quality

## Color semantics

Treat encoded sRGB, linear-light RGB, luminance, alpha, and display output as distinct quantities. Decode/normalize metadata before matching and blend in a consistent linear-light representation. W3C's color specification documents sRGB and linear-light conversion; it is a useful primary reference for transfer functions. [S37](../research/source-register.md#s37)

For normalized sRGB channel `c`, use `c/12.92` below the standard breakpoint and `((c+0.055)/1.055)^2.4` above it. For normalized linear RGB, a standard relative-luminance approximation is `Y=0.2126R+0.7152G+0.0722B`. Document the exact numeric thresholds/constants in code and fixtures. Never apply a user gamma curve accidentally twice.

## Image preprocessing order

Decode within limits, apply EXIF/display orientation, interpret source color metadata, decide alpha compositing background, select crop/contain/cover, resample, then apply artistic adjustments such as exposure, contrast, gamma, saturation, and edge emphasis. Distinguish physical color conversion from user-selected artistic distortion.

For unknown profiles, document an sRGB assumption and provide a warning where material. Do not market complete ICC/HDR accuracy until a chosen color-management implementation is integrated and tested. Initial wallpaper output is SDR; HDR video is either explicitly tone-mapped by a supported path or rejected with a useful explanation. Silent clipping is not a quality feature.

## Alpha

Transparent pixels must not contribute invisible RGB garbage to average colors. Use a documented straight/premultiplied convention at each boundary. For ordinary opaque wallpaper, composite the source over the selected background before matching. Transparent multi-layer scenes need linear premultiplied compositing and tests for colored fringes.

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
charged before output allocation. Progressive JPEG scan-block work is budgeted separately
from pixel work. Retained output remains charged until its `DecodedFrame` is dropped; its
`RasterFrameView` borrows from that owner. Cancellation is checked whenever either codec
requests compressed input through a 128-byte buffer; deterministic tests exercise regular and
Adam7 PNG plus progressive JPEG interruption before the end of the compressed stream. This is
bounded input cancellation, not CPU preemption: work on the current buffer continues until the
codec requests more input.
Malformed input, cancellation, and failed budget checks release reservations without publishing
partial frames. Codec-provided allocation limits remain defense in depth; application
accounting is not an allocator-enforced process memory limit.

Rights-cleared generated fixtures exercise PNG color/depth/interlace variants, JPEG baseline
and progressive scans, malformed/truncated inputs, metadata policy, and budget/cancellation
paths. Their generated configuration and SHA-256 values are printed/asserted by the focused
tests. These fixture checks do not imply native-codec, performance, or platform-support claims.
