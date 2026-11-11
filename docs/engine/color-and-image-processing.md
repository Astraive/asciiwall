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
