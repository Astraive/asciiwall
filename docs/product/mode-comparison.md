# Wallpaper modes and editing capabilities

| Source kind | Runtime work | Editable glyphs/grid | Live reactions | Portable source | Main tradeoff |
| --- | --- | --- | --- | --- | --- |
| `scene` | simulation + rendering | Yes | Native graph events | Small declarative recipe | New scene types require engine support |
| `image` | decode + conversion on change | Yes | Character effects and overlays | Image plus recipe | Large images require bounds and color handling |
| `video` | decode + conversion + rendering | Yes | Effects after conversion | Local media plus recipe | CPU/GPU/energy load |
| `ascii-clip` | decompress + character playback | Glyph styling yes; original sampling no | Supported grid effects | Character frames plus glyph table | Storage size; fixed source grid |
| `rendered-video` | decode + raster presentation | No conversion-level editing | Raster effects and optional live overlays | Baked video | Font/grid/color detail is baked in |

## Preserve meaning in the UI

A control is visible but disabled with a reason when the current source cannot support it. For example, `conversion.edge-strength` is unavailable for `rendered-video`; `presentation.tint` can remain available. A character clip may remap a glyph table or palette, but changing columns must either resample an existing grid explicitly or regenerate from a retained source. It is not equivalent to fresh image matching.

A composite wallpaper can contain a base rendered-video layer and a live cursor ripple layer. In that case only the overlay responds structurally; the base video cannot reconstruct missing glyph identities. State this in export previews.

## Switching execution strategy

The command `Bake` offers two targets: a character clip that preserves per-cell identities and a raster video that maximizes interoperability. A bake dialog records seed, time range, FPS, size, font fingerprint, conversion settings, and event replay policy. It warns when interactivity cannot be preserved.

Bake does not delete the project recipe. The creator can keep source plus baked alternative locally, but a shared package selects one primary source kind. Additional alternatives are a future schema extension, not a hidden version-one convention.

## Representation-independent effects

Opacity, crop, screen fit, global tint, and crossfade can operate on a composed frame. Character substitution, character displacement, per-cell palette changes, and glyph trails require a character layer. Window collision effects require a dynamic simulation or grid effect, regardless of whether the source was procedural or converted.

## Static versus idle

A static image is not automatically expensive just because it passes through a GPU renderer. Convert once, render when invalidated, and avoid an always-running frame timer. If the user enables a shimmer or pointer trail, that effect introduces time or event dependencies and legitimately wakes the renderer.
