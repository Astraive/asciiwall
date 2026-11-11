# Package examples and conformance vectors

## Valid scene example

This original documentation fixture uses only built-in scene data. It contains no third-party art or bundled font. It is a specification example, not an already published library wallpaper.

```json
{
  "scene_version": "1.0",
  "seed": 42,
  "nodes": [
    {
      "id": "rain",
      "kind": "matrix-rain",
      "params": {
        "density": 0.5,
        "speed": 1.2,
        "trail_cells": 18,
        "color": "#62dd88ff"
      }
    },
    {
      "id": "ripple",
      "kind": "click-ripple",
      "params": {
        "radius_cells": 15,
        "duration_ms": 700,
        "strength": 0.6
      }
    }
  ],
  "edges": [
    {
      "from": "rain",
      "to": "ripple",
      "slot": 0
    }
  ],
  "output": "ripple",
  "controls": [
    {
      "id": "frame-rate",
      "label": "Frame rate",
      "setting": "playback.fps"
    }
  ]
}
```

## Valid manifest example

The following asset hashes and sizes correspond to the exact example fixture bytes defined below. They are not dummy zero hashes. Pretty-printing or changing whitespace changes the scene hash, so the packer must recompute inventory from actual files.

```json
{
  "format": "asciiwall",
  "format_version": "1.0",
  "id": "org.asciiwall.examples.matrix-rain",
  "version": "1.0.0",
  "name": "Matrix Rain - Documentation Fixture",
  "summary": "A proposed built-in rain scene with optional passive click ripples; not a published wallpaper.",
  "authors": [
    {
      "name": "AsciiWall documentation example"
    }
  ],
  "license": "CC0-1.0",
  "license_files": [
    "licenses/example.txt"
  ],
  "engine": {
    "min_version": "0.1.0",
    "features_required": [
      "scene-v1"
    ]
  },
  "source": {
    "kind": "scene",
    "path": "scene.json"
  },
  "assets": [
    {
      "path": "scene.json",
      "media_type": "application/json",
      "sha256": "b4a12184e6f52544f13d48abe6cce3347b35f027375fa24a6856e0c0196937d3",
      "size": 653,
      "license": "CC0-1.0"
    },
    {
      "path": "licenses/example.txt",
      "media_type": "text/plain",
      "sha256": "6e208bde6c3e22e5a8c82e8336cf6a1ceec4aa081d673180db38e309efd0104b",
      "size": 106,
      "license": "CC0-1.0"
    }
  ],
  "settings": {
    "grid.columns": 240,
    "color.mode": "monochrome",
    "interaction.click.mode": "ripple"
  },
  "capabilities": {
    "required": [],
    "optional": [
      "pointer-buttons"
    ]
  },
  "tags": [
    "procedural",
    "rain",
    "monochrome"
  ]
}
```

The canonical `scene.json` fixture is the JSON above serialized with two-space indentation, ASCII escaping, LF line endings, and exactly one trailing LF. `licenses/example.txt` contains exactly this one line plus LF:

```text
Documentation fixture only. Original example data dedicated under CC0-1.0. No third-party media included.
```

The fixture text is for validator tests; real published packages must include the full applicable license text or required legal notices according to the chosen license.

## Source-kind examples

The source objects below are valid discriminators, not complete standalone manifests. Each referenced file must appear in a full manifest inventory with matching bytes and license.

```json
[
  {"kind":"image","path":"assets/mountain.png"},
  {"kind":"video","path":"assets/waves.webm"},
  {"kind":"ascii-clip","path":"assets/waves.ascii-clip"},
  {"kind":"rendered-video","path":"assets/waves-rendered.webm"}
]
```

## Negative conformance cases

| Case | Example / mutation | Required result |
|---|---|---|
| Traversal | `../outside.txt` | Reject before extraction |
| Absolute path | `/tmp/owned` | Reject |
| Windows path | `c:/users/file` or backslash path | Reject |
| Reserved name | `assets/con.png` | Reject |
| Stream name | `assets/art.png:payload` | Reject |
| Name collision | Two records with same normalized path | Reject |
| Link | ZIP entry with symlink mode | Reject |
| Undeclared entry | Extra executable or hidden file | Reject |
| Manifest ambiguity | Two manifests or duplicate JSON keys | Reject |
| Hash mismatch | Change one asset byte | Reject |
| Size lie | Header claims fewer bytes than actual | Stop at streamed cap and reject |
| Bomb | Expansion crosses total budget | Cancel and delete staging |
| Unknown source | `kind=script` | Schema rejection |
| Permission injection | `settings.privacy.network-enabled=true` | Schema rejection |
| Invalid numeric | Negative columns or non-finite number | Reject |
| Broken graph | Missing node, cycle, duplicate slot | Semantic rejection |
| Wrong port arity | Generator with an incoming edge | Semantic rejection |
| Undeclared observer | Click node without button capability | Semantic rejection |
| Unsupported feature | Unknown required engine feature | Import-only or reject activation |
| Missing font | Referenced font absent from inventory | Reject portable package |
| Wrong media type | Executable bytes labelled PNG | Decoder/content validation rejection |
| Huge raster | Small compressed file declares massive dimensions | Reject before full allocation |
| Nested I/O | Playlist referencing another local directory or URL | Reject/deny in worker |
| ID collision | Existing ID/version but different archive hash | Explicit conflict, never overwrite |
| Newer format | `format_version=99.0` | Unsupported-format error |

## What was verified for this documentation bundle

The embedded draft schemas and positive JSON fixtures are checked by a JSON Schema 2020-12 validator during document preparation. Asset example hashes are recomputed from exact fixture bytes. Negative schema mutations and a small semantic-graph validator are part of documentation QA. Native archive extraction, media decoding, platform integration, and fuzz campaigns are not implemented or tested by this bundle.
