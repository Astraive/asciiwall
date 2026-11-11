# The .asciiwall package format

## Status

Draft format 1.0, proposed for implementation. This is a design specification, not an existing registered standard or a claim that the application is available. Freeze only after a reference packer, validator, importer, and cross-platform conformance suite agree.

`.asciiwall` is a ZIP transport containing declarative metadata and local assets. Renaming a ZIP is not sufficient: every package must pass structural, semantic, security, compatibility, and resource validation. Stable v1 packages cannot contain executable plugins, arbitrary shaders, JavaScript, shell commands, native libraries, or WebAssembly.

## Layout

```text
example.asciiwall
  manifest.json
  scene.json                  # only for source.kind=scene
  assets/
    source-image.png           # optional selected media
    source-video.webm
    rendered-video.webm
    animation.ascii-clip
  previews/
    thumbnail.png
  fonts/
    chosen-font.ttf            # optional, redistribution rights required
  licenses/
    artwork.txt
    font-license.txt
```

Unused files are not automatically allowed. All regular files except `manifest.json` must appear exactly once in `assets` with a path, actual size, SHA-256, media type, and license. Directory records are optional and carry no content. The archive has exactly one root manifest, no enclosing folder, and no second archive disguised as a source.

## Source kinds

`scene` references a validated scene graph; `image` a decoded raster; `video` a locally decoded source converted live; `ascii-clip` a bounded precomputed character stream; `rendered-video` baked pixels already showing ASCII artwork. Baked video does not retain editable character structure. See [mode comparison](../product/mode-comparison.md).

The optional preview is a static raster, not active HTML/video. A fallback is a safe static image. A font is an explicitly declared font asset. Source, preview, fallback, font, and license paths must resolve to declared files of appropriate types.

## Paths and transport

v1 portable paths are lowercase ASCII, slash-separated, at most 240 characters and eight components. Permit letters, digits, hyphens, and dots within normal names. Reject empty components, dot/dot-dot components, leading slash, backslash, colons, trailing dots/spaces, drive/UNC paths, reserved Windows device basenames, and case-equivalent duplicates. The JSON path pattern is only an initial filter; semantic checks are mandatory.

ZIP Store and Deflate are the baseline. Reject encryption, multi-volume archives, links, special files, unsupported compression methods, and ambiguous filename metadata. Do not infer safety from a helper's path check alone. [S35](../research/source-register.md#s35)

## Default hard format budgets

| Limit | v1 value |
|---|---:|
| Compressed archive | 1,073,741,824 bytes (1 GiB) |
| Total actual expanded file bytes | 2,147,483,648 bytes (2 GiB) |
| Entries including directories/manifest | 4096 |
| Single ordinary asset | 1,073,741,824 bytes |
| Manifest | 1,048,576 bytes |
| Scene JSON | 4,194,304 bytes |
| Font asset | 16,777,216 bytes |
| Static preview asset | 8,388,608 bytes |
| JSON nesting | 32 levels |
| Decoded image | 64,000,000 pixels and 256 MiB buffer ceiling |

Limits apply to counted bytes, not just headers. Smaller device/user budgets may reject an otherwise conforming package with a clear explanation. Archives larger than these are not v1-conforming; users can reference large local media outside portable packages instead.

## Trust and identity

Package ID/version are claims, not proof of author identity. Compute a hash of the complete archive outside the archive, in the local import record or signed catalog. Do not put a self-referential archive hash inside the manifest. Asset hashes detect unexpected content changes but do not establish provenance.

Import is separate from Apply. Permissions, local output assignments, autostart, network settings, and app rules never live in the portable manifest. Updates install immutable new versions and preserve user overrides separately.
