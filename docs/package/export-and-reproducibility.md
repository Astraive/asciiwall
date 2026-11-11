# Export, portability, and reproducible packages

## Export variants

Portable wallpaper export includes the manifest, resolved visual settings, required source/scene assets, redistribution-approved font if selected, static preview/fallback if chosen, and attribution/license records. It excludes user permissions, output IDs, absolute paths, app rules, logs, caches, credentials, and arbitrary neighboring files.

A local backup is a different operation. It may include assignments and references after explicit review, but must not masquerade as a portable wallpaper suitable for community sharing. Name its purpose clearly and keep it outside the stable `.asciiwall` publication contract unless a future distinct backup format is defined.

## Source inclusion

Exporting a video-based wallpaper can include the original source or a permitted baked alternative. Show file size and license implications. An external path cannot be used to create a supposedly portable package that fails on every other machine. Missing assets block portable export with a repair action.

A rendered-video export cannot reconstruct editable glyphs later. An ASCII clip retains a glyph grid but does not retain the original source detail or semantic scene controls. Include source optionally only when the user owns redistribution rights. Conversion does not erase the original media's rights. [S54](../research/source-register.md#s54) [S55](../research/source-register.md#s55)

## Deterministic archive profile

Normalize and validate paths; sort entries bytewise; write one root manifest with deterministic UTF-8 encoding; use fixed ZIP timestamps and normalized permission metadata; select a fixed compression method/level and recorded packer version. Exclude volatile creation timestamps from canonical content.

Hash assets after their final bytes are produced. Serialize the manifest after the inventory is final. Compute the complete archive SHA-256 only after closing it and store that hash in the local receipt/catalog, not inside itself. Two identical source inputs/settings and the same toolchain should produce identical bytes under the deterministic profile.

Pixel-exact exports across different GPUs, font rasterizers, and encoder builds are not guaranteed. A reproducible package can contain a previously built artifact even when regenerating that artifact on another GPU produces small differences. Distinguish archive reproducibility, cell-frame reproducibility, and raster/video reproducibility.

## Atomic output

Write to a temporary sibling file and replace only on success. Existing output requires explicit `--overwrite` or UI confirmation. On cancellation or disk full, preserve the prior output and remove incomplete temp data. Do not export to a symlink target without a clear safe-path policy.

## Review screen

Show source files included, declared licenses, estimated size, font ownership, source versus baked mode, optional capability fallbacks, and a redacted metadata preview. Warn when author metadata contains contact details. A package is not automatically uploaded when exported.

## Verification

After packing, run the same independent validator used for imports. Confirm entry inventory, hashes, schema, graph, asset references, and limits. Test clean-machine import using only the package, with network denied and no access to the creator's original source directory.
