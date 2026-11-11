# Manifest JSON Schema and semantic contract

## Normative draft schema

This embedded JSON Schema uses draft 2020-12. [S36](../research/source-register.md#s36) It is a closed-world v1 structure: unknown fields and unknown portable setting names are rejected. Defaults annotate the schema; a validator does not necessarily insert them. The engine's settings resolver fills absent values.

The schema is not an archive validator. Path normalization, duplicate JSON keys, asset existence, actual hashes, resource totals, version comparison, licensing review, and capability checks are additional mandatory phases.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:asciiwall:schema:manifest:1.0",
  "title": "AsciiWall draft package manifest 1.0",
  "type": "object",
  "properties": {
    "format": {
      "const": "asciiwall"
    },
    "format_version": {
      "const": "1.0"
    },
    "id": {
      "type": "string",
      "maxLength": 120,
      "pattern": "^[a-z0-9]+(?:[.-][a-z0-9]+)+$"
    },
    "version": {
      "type": "string",
      "maxLength": 80,
      "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$"
    },
    "name": {
      "type": "string",
      "minLength": 1,
      "maxLength": 120
    },
    "summary": {
      "type": "string",
      "minLength": 1,
      "maxLength": 300
    },
    "authors": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 120
          },
          "url": {
            "type": "string",
            "format": "uri",
            "maxLength": 2048
          }
        },
        "required": [
          "name"
        ],
        "additionalProperties": false
      },
      "maxItems": 16,
      "minItems": 1
    },
    "license": {
      "type": "string",
      "minLength": 1,
      "maxLength": 120
    },
    "license_files": {
      "type": "array",
      "items": {
        "type": "string",
        "minLength": 1,
        "maxLength": 240,
        "pattern": "^[a-z0-9][a-z0-9./-]*$"
      },
      "maxItems": 32,
      "minItems": 1
    },
    "engine": {
      "type": "object",
      "properties": {
        "min_version": {
          "type": "string",
          "maxLength": 80,
          "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$"
        },
        "features_required": {
          "type": "array",
          "items": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80
          },
          "maxItems": 32,
          "uniqueItems": true
        }
      },
      "required": [
        "min_version",
        "features_required"
      ],
      "additionalProperties": false
    },
    "source": {
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "kind": {
              "const": "scene"
            },
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": 240,
              "pattern": "^[a-z0-9][a-z0-9./-]*$"
            }
          },
          "required": [
            "kind",
            "path"
          ],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "kind": {
              "const": "image"
            },
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": 240,
              "pattern": "^[a-z0-9][a-z0-9./-]*$"
            }
          },
          "required": [
            "kind",
            "path"
          ],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "kind": {
              "const": "video"
            },
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": 240,
              "pattern": "^[a-z0-9][a-z0-9./-]*$"
            }
          },
          "required": [
            "kind",
            "path"
          ],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "kind": {
              "const": "ascii-clip"
            },
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": 240,
              "pattern": "^[a-z0-9][a-z0-9./-]*$"
            }
          },
          "required": [
            "kind",
            "path"
          ],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "kind": {
              "const": "rendered-video"
            },
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": 240,
              "pattern": "^[a-z0-9][a-z0-9./-]*$"
            }
          },
          "required": [
            "kind",
            "path"
          ],
          "additionalProperties": false
        }
      ]
    },
    "assets": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "path": {
            "type": "string",
            "minLength": 1,
            "maxLength": 240,
            "pattern": "^[a-z0-9][a-z0-9./-]*$"
          },
          "media_type": {
            "type": "string",
            "minLength": 1,
            "maxLength": 100
          },
          "sha256": {
            "type": "string",
            "pattern": "^[a-f0-9]{64}$"
          },
          "size": {
            "type": "integer",
            "minimum": 0,
            "maximum": 1073741824
          },
          "license": {
            "type": "string",
            "minLength": 1,
            "maxLength": 120
          },
          "attribution": {
            "type": "string",
            "maxLength": 2000
          }
        },
        "required": [
          "path",
          "media_type",
          "sha256",
          "size",
          "license"
        ],
        "additionalProperties": false
      },
      "maxItems": 4095,
      "minItems": 1
    },
    "settings": {
      "type": "object",
      "properties": {
        "grid.columns": {
          "type": "integer",
          "minimum": 32,
          "maximum": 1024,
          "default": 240
        },
        "grid.rows": {
          "type": "integer",
          "minimum": 0,
          "maximum": 1024,
          "default": 0
        },
        "grid.cell-aspect": {
          "type": "number",
          "minimum": 0,
          "maximum": 4,
          "default": 0
        },
        "grid.fit": {
          "type": "string",
          "enum": [
            "contain",
            "cover",
            "stretch"
          ],
          "default": "contain"
        },
        "grid.anchor-x": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.5
        },
        "grid.anchor-y": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.5
        },
        "grid.padding-cells": {
          "type": "integer",
          "minimum": 0,
          "maximum": 64,
          "default": 0
        },
        "font.face": {
          "type": "string",
          "maxLength": 80,
          "default": "builtin-mono"
        },
        "font.pixel-size": {
          "type": "integer",
          "minimum": 6,
          "maximum": 128,
          "default": 16
        },
        "font.line-spacing": {
          "type": "number",
          "minimum": 0.5,
          "maximum": 3,
          "default": 1
        },
        "font.letter-spacing": {
          "type": "number",
          "minimum": -2,
          "maximum": 8,
          "default": 0
        },
        "font.antialias": {
          "type": "boolean",
          "default": true
        },
        "charset.mode": {
          "type": "string",
          "enum": [
            "ascii",
            "blocks",
            "braille",
            "custom"
          ],
          "default": "ascii"
        },
        "charset.custom": {
          "type": "string",
          "maxLength": 4096,
          "default": " .:-=+*#%@"
        },
        "charset.invert": {
          "type": "boolean",
          "default": false
        },
        "conversion.matcher": {
          "type": "string",
          "enum": [
            "density",
            "structural"
          ],
          "default": "density"
        },
        "conversion.sample": {
          "type": "string",
          "enum": [
            "area",
            "bilinear"
          ],
          "default": "area"
        },
        "conversion.brightness": {
          "type": "number",
          "minimum": -1,
          "maximum": 1,
          "default": 0
        },
        "conversion.contrast": {
          "type": "number",
          "minimum": 0,
          "maximum": 4,
          "default": 1
        },
        "conversion.gamma": {
          "type": "number",
          "minimum": 0.1,
          "maximum": 5,
          "default": 1
        },
        "conversion.saturation": {
          "type": "number",
          "minimum": 0,
          "maximum": 3,
          "default": 1
        },
        "conversion.edge-strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 2,
          "default": 0.35
        },
        "conversion.tone-weight": {
          "type": "number",
          "minimum": 0,
          "maximum": 4,
          "default": 1
        },
        "conversion.shape-weight": {
          "type": "number",
          "minimum": 0,
          "maximum": 4,
          "default": 1
        },
        "conversion.candidate-count": {
          "type": "integer",
          "minimum": 1,
          "maximum": 128,
          "default": 12
        },
        "conversion.dither": {
          "type": "string",
          "enum": [
            "none",
            "ordered",
            "error-diffusion"
          ],
          "default": "none"
        },
        "conversion.dither-strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.25
        },
        "conversion.temporal-stability": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.3
        },
        "conversion.cut-threshold": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.35
        },
        "conversion.alpha": {
          "type": "string",
          "enum": [
            "composite",
            "preserve"
          ],
          "default": "composite"
        },
        "color.mode": {
          "type": "string",
          "enum": [
            "monochrome",
            "source",
            "ansi16",
            "ansi256",
            "duotone"
          ],
          "default": "monochrome"
        },
        "color.foreground": {
          "type": "string",
          "pattern": "^#[0-9a-fA-F]{8}$",
          "default": "#d7fbe8ff"
        },
        "color.background": {
          "type": "string",
          "pattern": "^#[0-9a-fA-F]{8}$",
          "default": "#07110bff"
        },
        "color.secondary": {
          "type": "string",
          "pattern": "^#[0-9a-fA-F]{8}$",
          "default": "#58aa80ff"
        },
        "color.opacity": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 1
        },
        "color.palette-dither": {
          "type": "boolean",
          "default": false
        },
        "playback.fps": {
          "type": "integer",
          "minimum": 1,
          "maximum": 120,
          "default": 30
        },
        "playback.speed": {
          "type": "number",
          "minimum": 0.1,
          "maximum": 4,
          "default": 1
        },
        "playback.loop": {
          "type": "boolean",
          "default": true
        },
        "playback.loop-start-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 86400000,
          "default": 0
        },
        "playback.loop-end-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 86400000,
          "default": 0
        },
        "playback.resume": {
          "type": "string",
          "enum": [
            "continue",
            "restart",
            "wall-clock"
          ],
          "default": "continue"
        },
        "playback.loop-transition-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 2000,
          "default": 0
        },
        "scene.seed": {
          "type": "integer",
          "minimum": 0,
          "maximum": 4294967295,
          "default": 1
        },
        "scene.time-scale": {
          "type": "number",
          "minimum": 0,
          "maximum": 4,
          "default": 1
        },
        "scene.max-particles": {
          "type": "integer",
          "minimum": 0,
          "maximum": 20000,
          "default": 2000
        },
        "scene.simulation-hz": {
          "type": "integer",
          "minimum": 15,
          "maximum": 120,
          "default": 60
        },
        "scene.background-motion": {
          "type": "boolean",
          "default": true
        },
        "effects.glitch.enabled": {
          "type": "boolean",
          "default": false
        },
        "effects.glitch.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0.1
        },
        "effects.glitch.interval-ms": {
          "type": "integer",
          "minimum": 100,
          "maximum": 60000,
          "default": 3000
        },
        "effects.scanlines.enabled": {
          "type": "boolean",
          "default": false
        },
        "effects.scanlines.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 0.5,
          "default": 0.1
        },
        "effects.bloom.enabled": {
          "type": "boolean",
          "default": false
        },
        "effects.bloom.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 2,
          "default": 0.2
        },
        "effects.trails.enabled": {
          "type": "boolean",
          "default": false
        },
        "effects.trails.decay": {
          "type": "number",
          "minimum": 0,
          "maximum": 0.99,
          "default": 0.8
        },
        "effects.vignette": {
          "type": "number",
          "minimum": 0,
          "maximum": 1,
          "default": 0
        },
        "effects.noise": {
          "type": "number",
          "minimum": 0,
          "maximum": 0.5,
          "default": 0
        },
        "interaction.pointer.mode": {
          "type": "string",
          "enum": [
            "none",
            "attract",
            "repel",
            "parallax",
            "reveal"
          ],
          "default": "none"
        },
        "interaction.pointer.radius-cells": {
          "type": "number",
          "minimum": 0.5,
          "maximum": 100,
          "default": 10
        },
        "interaction.pointer.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 5,
          "default": 0.5
        },
        "interaction.pointer.smoothing-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 1000,
          "default": 80
        },
        "interaction.pointer.max-offset-cells": {
          "type": "number",
          "minimum": 0,
          "maximum": 32,
          "default": 4
        },
        "interaction.click.mode": {
          "type": "string",
          "enum": [
            "none",
            "ripple",
            "burst",
            "reveal",
            "pulse"
          ],
          "default": "none"
        },
        "interaction.click.radius-cells": {
          "type": "number",
          "minimum": 1,
          "maximum": 100,
          "default": 15
        },
        "interaction.click.duration-ms": {
          "type": "integer",
          "minimum": 50,
          "maximum": 5000,
          "default": 700
        },
        "interaction.click.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 3,
          "default": 0.6
        },
        "interaction.click.max-active": {
          "type": "integer",
          "minimum": 1,
          "maximum": 64,
          "default": 8
        },
        "interaction.click.cooldown-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 2000,
          "default": 60
        },
        "interaction.window.mode": {
          "type": "string",
          "enum": [
            "none",
            "avoid",
            "wake",
            "edge-glow"
          ],
          "default": "none"
        },
        "interaction.window.margin-cells": {
          "type": "number",
          "minimum": 0,
          "maximum": 32,
          "default": 2
        },
        "interaction.window.strength": {
          "type": "number",
          "minimum": 0,
          "maximum": 5,
          "default": 0.5
        },
        "interaction.window.decay-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 3000,
          "default": 250
        },
        "transition.in": {
          "type": "string",
          "enum": [
            "none",
            "fade",
            "dissolve",
            "scanline",
            "cascade",
            "radial",
            "glitch"
          ],
          "default": "fade"
        },
        "transition.out": {
          "type": "string",
          "enum": [
            "none",
            "fade",
            "dissolve",
            "scanline",
            "cascade",
            "radial",
            "glitch"
          ],
          "default": "fade"
        },
        "transition.duration-ms": {
          "type": "integer",
          "minimum": 0,
          "maximum": 5000,
          "default": 350
        },
        "transition.easing": {
          "type": "string",
          "enum": [
            "linear",
            "ease-in",
            "ease-out",
            "ease-in-out"
          ],
          "default": "ease-in-out"
        },
        "transition.direction": {
          "type": "string",
          "enum": [
            "left-to-right",
            "right-to-left",
            "top-to-bottom",
            "bottom-to-top"
          ],
          "default": "left-to-right"
        },
        "transition.on-resume": {
          "type": "boolean",
          "default": false
        }
      },
      "additionalProperties": false
    },
    "capabilities": {
      "type": "object",
      "properties": {
        "required": {
          "type": "array",
          "items": {
            "type": "string",
            "enum": [
              "pointer-position",
              "pointer-buttons",
              "window-rectangles",
              "window-state",
              "local-clock"
            ]
          },
          "maxItems": 5,
          "uniqueItems": true
        },
        "optional": {
          "type": "array",
          "items": {
            "type": "string",
            "enum": [
              "pointer-position",
              "pointer-buttons",
              "window-rectangles",
              "window-state",
              "local-clock"
            ]
          },
          "maxItems": 5,
          "uniqueItems": true
        }
      },
      "required": [
        "required",
        "optional"
      ],
      "additionalProperties": false
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string",
        "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$",
        "maxLength": 40
      },
      "maxItems": 16,
      "uniqueItems": true
    },
    "preview": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "minLength": 1,
          "maxLength": 240,
          "pattern": "^[a-z0-9][a-z0-9./-]*$"
        }
      },
      "required": [
        "path"
      ],
      "additionalProperties": false
    },
    "fallback": {
      "type": "object",
      "properties": {
        "kind": {
          "const": "image"
        },
        "path": {
          "type": "string",
          "minLength": 1,
          "maxLength": 240,
          "pattern": "^[a-z0-9][a-z0-9./-]*$"
        }
      },
      "required": [
        "kind",
        "path"
      ],
      "additionalProperties": false
    },
    "font": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "minLength": 1,
          "maxLength": 240,
          "pattern": "^[a-z0-9][a-z0-9./-]*$"
        }
      },
      "required": [
        "path"
      ],
      "additionalProperties": false
    },
    "provenance": {
      "type": "object",
      "properties": {
        "base_id": {
          "type": "string",
          "maxLength": 120,
          "pattern": "^[a-z0-9]+(?:[.-][a-z0-9]+)+$"
        },
        "base_version": {
          "type": "string",
          "maxLength": 80,
          "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$"
        },
        "base_sha256": {
          "type": "string",
          "pattern": "^[a-f0-9]{64}$"
        },
        "changes": {
          "type": "string",
          "minLength": 1,
          "maxLength": 2000
        }
      },
      "required": [
        "base_id",
        "base_version",
        "changes"
      ],
      "additionalProperties": false
    }
  },
  "required": [
    "format",
    "format_version",
    "id",
    "version",
    "name",
    "summary",
    "authors",
    "license",
    "license_files",
    "engine",
    "source",
    "assets",
    "settings",
    "capabilities",
    "tags"
  ],
  "additionalProperties": false
}
```

## Mandatory semantic validation beyond JSON Schema

1. `id` is a package identity, not a path or verified publisher claim. Compare full semantic versions using a tested SemVer parser; the schema's pattern is a coarse syntax filter, not a full prerelease-order validator.
2. Asset paths are unique after the portable path policy. Their actual bytes, sizes, and hashes match. No undeclared files are extracted into the managed package tree.
3. Source and scene-referenced media, preview, fallback, font, and every license file exist in the inventory. Content sniffing and decoder validation agree with the declared role; extension or MIME text alone is insufficient.
4. Required and optional capabilities are disjoint. Missing required capabilities prevent activation; optional capabilities use a defined no-op or static fallback. Import for storage may still be allowed when activation is incompatible.
5. Feature identifiers are resolved against the engine's supported feature registry. Unknown required features reject activation; a source kind's implicit feature requirement cannot be bypassed by omitting it from `features_required`.
6. Package settings are suggestions only. Every value passes cross-field checks and cannot grant consent or exceed hard resource budgets. Unsupported font IDs fail or use an explicitly disclosed built-in fallback.
7. URLs in author metadata are display-only, validated as allowed HTTP(S) links before a user opens them. They are never fetched automatically. The JSON `format` keyword may be annotation-only in some validators, so enforce URL policy explicitly.
8. License identifiers and attribution are author declarations, not a legal determination. Custom `LicenseRef-` entries require their actual license text. Community publication additionally requires provenance review.

## Version policy

`format_version` is 1.0, package `version` is the author's SemVer, and `engine.min_version` describes implementation compatibility. These are not interchangeable. A newer engine is not necessarily capable of an optional disabled codec or an unavailable platform observer. See [compatibility](compatibility-and-migrations.md).

## Font selector semantics

A declared `font` asset becomes active only when `settings.font.face` is `package-font`. Selecting `package-font` without the asset is invalid. Other face values resolve through the engine's approved built-in font registry. This keeps font choice explicit and prevents arbitrary filesystem path resolution. A font asset included only for a later local variant still needs its inventory/license records.
