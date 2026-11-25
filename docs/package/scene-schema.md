# Declarative scene schema and execution rules

## Purpose

A scene is a bounded graph of built-in operations, not a program with file/network/system access. v1 includes core generators, local image/video sources, color, passive reactions, trails, and a binary composite. Future generators extend the versioned registry and compatibility metadata rather than being executed as downloaded code.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:asciiwall:schema:scene:1.0",
  "title": "AsciiWall draft scene 1.0",
  "type": "object",
  "properties": {
    "scene_version": {
      "const": "1.0"
    },
    "seed": {
      "type": "integer",
      "minimum": 0,
      "maximum": 4294967295
    },
    "nodes": {
      "type": "array",
      "items": {
        "oneOf": [
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "solid"
              },
              "params": {
                "type": "object",
                "properties": {
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "matrix-rain"
              },
              "params": {
                "type": "object",
                "properties": {
                  "density": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1
                  },
                  "speed": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "trail_cells": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200
                  },
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "density",
                  "speed",
                  "trail_cells",
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "starfield"
              },
              "params": {
                "type": "object",
                "properties": {
                  "count": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20000
                  },
                  "speed": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "depth": {
                    "type": "number",
                    "minimum": 0.1,
                    "maximum": 100
                  },
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "count",
                  "speed",
                  "depth",
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "waves"
              },
              "params": {
                "type": "object",
                "properties": {
                  "amplitude": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "frequency": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 20
                  },
                  "speed": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "amplitude",
                  "frequency",
                  "speed",
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "noise-field"
              },
              "params": {
                "type": "object",
                "properties": {
                  "scale": {
                    "type": "number",
                    "minimum": 0.01,
                    "maximum": 100
                  },
                  "speed": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "scale",
                  "speed",
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "cube-wireframe"
              },
              "params": {
                "type": "object",
                "properties": {
                  "speed": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 10
                  },
                  "scale": {
                    "type": "number",
                    "minimum": 0.05,
                    "maximum": 1
                  },
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  }
                },
                "required": [
                  "speed",
                  "scale",
                  "color"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "image-source"
              },
              "params": {
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
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "video-source"
              },
              "params": {
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
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "glyph-color"
              },
              "params": {
                "type": "object",
                "properties": {
                  "color": {
                    "type": "string",
                    "pattern": "^#[0-9a-fA-F]{8}$"
                  },
                  "strength": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1
                  }
                },
                "required": [
                  "color",
                  "strength"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "pointer-force"
              },
              "params": {
                "type": "object",
                "properties": {
                  "mode": {
                    "type": "string",
                    "enum": [
                      "attract",
                      "repel"
                    ]
                  },
                  "radius_cells": {
                    "type": "number",
                    "minimum": 0.5,
                    "maximum": 100
                  },
                  "strength": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 5
                  }
                },
                "required": [
                  "mode",
                  "radius_cells",
                  "strength"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "click-ripple"
              },
              "params": {
                "type": "object",
                "properties": {
                  "radius_cells": {
                    "type": "number",
                    "minimum": 1,
                    "maximum": 100
                  },
                  "duration_ms": {
                    "type": "integer",
                    "minimum": 50,
                    "maximum": 5000
                  },
                  "strength": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 3
                  }
                },
                "required": [
                  "radius_cells",
                  "duration_ms",
                  "strength"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "window-avoid"
              },
              "params": {
                "type": "object",
                "properties": {
                  "margin_cells": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 32
                  },
                  "strength": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 5
                  }
                },
                "required": [
                  "margin_cells",
                  "strength"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "trails"
              },
              "params": {
                "type": "object",
                "properties": {
                  "decay": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 0.99
                  }
                },
                "required": [
                  "decay"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          },
          {
            "type": "object",
            "properties": {
              "id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9-]{0,63}$"
              },
              "kind": {
                "const": "composite"
              },
              "params": {
                "type": "object",
                "properties": {
                  "mode": {
                    "type": "string",
                    "enum": [
                      "over",
                      "add",
                      "multiply"
                    ]
                  },
                  "opacity": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1
                  }
                },
                "required": [
                  "mode",
                  "opacity"
                ],
                "additionalProperties": false
              }
            },
            "required": [
              "id",
              "kind",
              "params"
            ],
            "additionalProperties": false
          }
        ]
      },
      "maxItems": 128,
      "minItems": 1
    },
    "edges": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "from": {
            "type": "string",
            "minLength": 1,
            "maxLength": 64
          },
          "to": {
            "type": "string",
            "minLength": 1,
            "maxLength": 64
          },
          "slot": {
            "type": "integer",
            "minimum": 0,
            "maximum": 1
          }
        },
        "required": [
          "from",
          "to",
          "slot"
        ],
        "additionalProperties": false
      },
      "maxItems": 256,
      "minItems": 0
    },
    "output": {
      "type": "string",
      "minLength": 1,
      "maxLength": 64
    },
    "controls": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string",
            "pattern": "^[a-z][a-z0-9-]{0,63}$"
          },
          "label": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80
          },
          "setting": {
            "type": "string",
            "enum": [
              "grid.columns",
              "grid.rows",
              "grid.cell-aspect",
              "grid.fit",
              "grid.anchor-x",
              "grid.anchor-y",
              "grid.padding-cells",
              "font.face",
              "font.pixel-size",
              "font.line-spacing",
              "font.letter-spacing",
              "font.antialias",
              "charset.mode",
              "charset.custom",
              "charset.invert",
              "conversion.matcher",
              "conversion.sample",
              "conversion.brightness",
              "conversion.contrast",
              "conversion.gamma",
              "conversion.saturation",
              "conversion.edge-strength",
              "conversion.tone-weight",
              "conversion.shape-weight",
              "conversion.candidate-count",
              "conversion.dither",
              "conversion.dither-strength",
              "conversion.temporal-stability",
              "conversion.cut-threshold",
              "conversion.alpha",
              "color.mode",
              "color.foreground",
              "color.background",
              "color.secondary",
              "color.opacity",
              "color.palette-dither",
              "playback.fps",
              "playback.speed",
              "playback.loop",
              "playback.loop-start-ms",
              "playback.loop-end-ms",
              "playback.resume",
              "playback.loop-transition-ms",
              "scene.seed",
              "scene.time-scale",
              "scene.max-particles",
              "scene.simulation-hz",
              "scene.background-motion",
              "effects.glitch.enabled",
              "effects.glitch.strength",
              "effects.glitch.interval-ms",
              "effects.scanlines.enabled",
              "effects.scanlines.strength",
              "effects.bloom.enabled",
              "effects.bloom.strength",
              "effects.trails.enabled",
              "effects.trails.decay",
              "effects.vignette",
              "effects.noise",
              "interaction.pointer.mode",
              "interaction.pointer.radius-cells",
              "interaction.pointer.strength",
              "interaction.pointer.smoothing-ms",
              "interaction.pointer.max-offset-cells",
              "interaction.click.mode",
              "interaction.click.radius-cells",
              "interaction.click.duration-ms",
              "interaction.click.strength",
              "interaction.click.max-active",
              "interaction.click.cooldown-ms",
              "interaction.window.mode",
              "interaction.window.margin-cells",
              "interaction.window.strength",
              "interaction.window.decay-ms",
              "transition.in",
              "transition.out",
              "transition.duration-ms",
              "transition.easing",
              "transition.direction",
              "transition.on-resume"
            ]
          }
        },
        "required": [
          "id",
          "label",
          "setting"
        ],
        "additionalProperties": false
      },
      "maxItems": 64,
      "minItems": 0
    }
  },
  "required": [
    "scene_version",
    "seed",
    "nodes",
    "edges",
    "output",
    "controls"
  ],
  "additionalProperties": false
}
```

## Graph semantics

All node IDs are unique. Every edge references existing nodes; the output references one existing node. Graphs must be acyclic after sorting and all nodes must contribute to the selected output, preventing hidden unreachable work. Resource allocation is computed before execution.

Source/generator nodes (`solid`, `matrix-rain`, `starfield`, `waves`, `noise-field`, `cube-wireframe`, `image-source`, `video-source`) have no incoming edges. Unary effect nodes have exactly one incoming edge at slot 0. `composite` has exactly two incoming edges, one each at slots 0 and 1; slot 0 is the base and slot 1 the overlay. Duplicate destination slots and outgoing references from missing nodes are invalid.

All nodes produce the documented character-frame contract in v1. An image/video source node includes the shared conversion stage, not an untyped raster socket. Future mixed raster/cell graphs need explicit typed ports and another capability version; do not silently connect unlike buffers.

## FX-015 proposed typed graph and migration examples

**Research proposal only—not part of the draft-v1 schema and not an implementation or approval.** The closed v1 JSON schema above is unchanged. In v1 every node produces the documented character-frame contract; an image/video source contains the shared conversion stage and is not an untyped raster socket. A v1 document containing typed `ports`, bindings, or future capability declarations remains invalid under the current schema. The examples below are fixtures for future design review only and are explicitly non-conforming to v1, including the proposed-valid example.

### Proposed compatibility and arity matrix

Direct connections require exact matching types. `✓` is a candidate compatible pair; `—` is rejected. A conversion between unlike types must be a separately registered, versioned node with explicit typed input/output ports and bounded cost.

| Output type → input type | raster | character-grid | scalar-field | vector-field | impulse-stream | palette | presentation-layer |
| --- | --- | --- | --- | --- | --- | --- | --- |
| raster | ✓ | — | — | — | — | — | — |
| character-grid | — | ✓ | — | — | — | — | — |
| scalar-field | — | — | ✓ | — | — | — | — |
| vector-field | — | — | — | ✓ | — | — | — |
| impulse-stream | — | — | — | — | ✓ | — | — |
| palette | — | — | — | — | — | ✓ | — |
| presentation-layer | — | — | — | — | — | — | ✓ |

| Candidate node arity | Required inputs | Validation |
| --- | --- | --- |
| Source/generator | 0 unless its fixed descriptor explicitly declares inputs | Reject every unexpected incoming edge. |
| Unary effect | 1 input at its declared slot (candidate default slot 0) | Reject missing, duplicate, extra, or type-mismatched slots. |
| Binary operation | 2 inputs at its declared slots (candidate default slots 0 and 1) | Reject missing, duplicate, extra, or type-mismatched slots. |
| Multi-port conversion | Exactly the named inputs in its reviewed descriptor | Check each port independently; never infer conversion from endpoints. |

These arities and type names are design recommendations pending owner approval; the v1 arities above remain the only current contract.

### Proposed-version migration illustration

**Future-version proposal fixture; deliberately non-conforming to draft v1.** This sketch represents a v1 character-frame source extended through a registered conversion into a future typed graph. Names, descriptor version, `scene_version`, and capability key are illustrative and not accepted schema syntax.

```json
{
  "scene_version": "2.0",
  "required_capabilities": ["typed-ports-v1"],
  "seed": 7,
  "nodes": [
    {
      "id": "source",
      "kind": "solid",
      "descriptor_version": 1,
      "params": { "color": "#ffffffff" },
      "outputs": { "frame": "character-grid" }
    },
    {
      "id": "to-raster",
      "kind": "grid-to-raster",
      "descriptor_version": 1,
      "inputs": { "grid": "character-grid" },
      "outputs": { "image": "raster" },
      "params": {}
    },
    {
      "id": "present",
      "kind": "present",
      "descriptor_version": 1,
      "inputs": { "layer": "raster" },
      "outputs": { "layer": "presentation-layer" },
      "params": {}
    }
  ],
  "edges": [
    { "from": "source", "from_port": "frame", "to": "to-raster", "to_port": "grid", "slot": 0 },
    { "from": "to-raster", "from_port": "image", "to": "present", "to_port": "layer", "slot": 0 }
  ],
  "output": { "node": "present", "port": "layer" }
}
```

This is only an internally consistent typed-port sketch: it does not establish those node kinds, descriptors, a schema, or client support. A migration tool could preserve a v1 scene by registering and validating every extension or by baking an unsupported visual effect into ordinary source media, then validating the output against a supported schema. It must not relabel v1 JSON as v2 or silently discard meaning.

### Required v1 rejection examples

Each fragment below is an invalid draft-v1 migration candidate. None is a future-version conformance fixture or evidence of a running validator.

**Richer port / raster-to-cell mismatch:** `ports` is forbidden by v1's closed node object, and a future raster output cannot connect to a v1 character-frame-only node.

```json
{
  "scene_version": "1.0",
  "seed": 7,
  "nodes": [{
    "id": "source",
    "kind": "image-source",
    "params": { "path": "media/source.png" },
    "ports": { "output": "raster" }
  }],
  "edges": [],
  "output": "source",
  "controls": []
}
```

**Unknown node:** a proposed registry entry is not accepted by the fixed v1 kind registry.

```json
{
  "scene_version": "1.0",
  "seed": 7,
  "nodes": [{ "id": "glow", "kind": "third-party-glow", "params": {} }],
  "edges": [],
  "output": "glow",
  "controls": []
}
```

**Cycle:** ordinary graph cycles are invalid in v1 and remain forbidden in the proposed typed graph. FX-015 admits no feedback node or cycle exception; any exception requires a separate future reviewed decision defining the bounded-feedback node and its contract first.

```json
{
  "scene_version": "1.0",
  "seed": 7,
  "nodes": [
    { "id": "a", "kind": "trails", "params": { "decay": 0.5 } },
    { "id": "b", "kind": "trails", "params": { "decay": 0.5 } }
  ],
  "edges": [
    { "from": "a", "to": "b", "slot": 0 },
    { "from": "b", "to": "a", "slot": 0 }
  ],
  "output": "b",
  "controls": []
}
```

**Unbounded history:** no v1 field or proposed extension may request unbounded retained frames or bytes. A future declared cap exceeding checked per-node or aggregate budget also rejects before allocation.

```json
{
  "scene_version": "1.0",
  "seed": 7,
  "nodes": [{
    "id": "trail",
    "kind": "trails",
    "params": { "history_frames": "unbounded" }
  }],
  "edges": [],
  "output": "trail",
  "controls": []
}
```

**Arbitrary expression:** expression text is neither a v1 parameter nor an approved future binding operator and must never be parsed or executed.

```json
{
  "scene_version": "1.0",
  "seed": 7,
  "nodes": [{
    "id": "mapped",
    "kind": "waves",
    "params": {
      "amplitude": 1,
      "frequency": 1,
      "speed": 0,
      "color": "#ffffffff",
      "binding": "sin(pointer.x)"
    }
  }],
  "edges": [],
  "output": "mapped",
  "controls": []
}
```

### Signal-binding proposal and compatibility

**Future-version proposal fixture; deliberately non-conforming to draft v1.** A future binding must carry a fixed signal identifier, its source type/range, an approved host capability identity, permission separation, target type/range, bounded scale/offset/clamp/easing/smoothing/dead-zone operators, and missing-feed behavior. For example, normalized pointer `x` in `[0,1]` could use pointer-position capability to drive a parallax target clamped to `[-4,4]` cells; a missing, stale, denied, or revoked feed yields zero offset/pass-through with no prompt or broader capture. These are candidate bounds pending owner approval, not present v1 behavior.

```json
{
  "scene_version": "2.0",
  "required_capabilities": ["typed-ports-v1", "signal-binding-v1"],
  "seed": 7,
  "nodes": [
    {
      "id": "source",
      "kind": "solid",
      "descriptor_version": 1,
      "params": { "color": "#ffffffff" },
      "outputs": { "frame": "character-grid" }
    },
    {
      "id": "parallax",
      "kind": "cell-parallax",
      "descriptor_version": 1,
      "inputs": { "frame": "character-grid" },
      "outputs": { "frame": "character-grid" },
      "parameter_schema": {
        "offset-x": { "type": "scalar", "unit": "cells", "range": [-4, 4] }
      },
      "params": { "offset-x": 0 }
    }
  ],
  "edges": [
    { "from": "source", "from_port": "frame", "to": "parallax", "to_port": "frame", "slot": 0 }
  ],
  "output": { "node": "parallax", "port": "frame" },
  "bindings": [{
    "id": "pointer-parallax",
    "signal": "pointer.x",
    "source": { "type": "scalar", "range": [0, 1] },
    "capability": "pointer-position",
    "permission": "host-approved-only",
    "target": { "node": "parallax", "parameter": "offset-x", "type": "scalar", "unit": "cells", "range": [-4, 4] },
    "operators": [
      { "kind": "dead-zone", "fraction": 0.02, "range": [0, 0.5] },
      { "kind": "easing", "curve": "smoothstep" },
      { "kind": "scale", "value": 8, "range": [-8, 8] },
      { "kind": "offset", "value": -4, "range": [-8, 8] },
      { "kind": "smoothing", "duration-ms": 100, "range-ms": [0, 2000] },
      { "kind": "clamp", "min": -4, "max": 4 }
    ],
    "missing": "zero-offset-pass-through"
  }]
}
```

This sketch cannot grant permissions: the host capability registry supplies provenance and consent, and package data cannot query or broaden sensors. Existing v1 reaction nodes own their built-in mappings; v1 controls reference approved portable settings keys. Neither is an arbitrary binding language.

An unsupported client rejects a package that requires an unknown schema version or required capability without activating it or changing the current scene. It may ignore an optional extension only if an explicit reviewed fallback is declared and preserves safe behavior. No implicit downgrade, partial execution, plugin, executable package, or JSON escape hatch is permitted. Any later migration to v1 must rewrite using supported v1 nodes or bake an effect and validate the new document.

### FX-015 approval and verification status

**Proposed for review; not approved or implemented.** Product choices still open: final schema/version and capability identifiers; which port types and conversion nodes to support; precise arities and numeric/operator bounds; signal required/optional and stale/revoked behavior; whether a separate future decision should consider bounded feedback at all; per-node resource, reduced-motion, and export policies; and feature-version, node-extension, or baked-fallback selection for each future family. No native limitation is claimed measured: environment-specific native support remains unverified unless separately measured and recorded.

## Behavior without optional feeds

A pointer/click/window node becomes a transparent pass-through when its feed is unavailable, unless the manifest declared that capability required. A package containing such a node must list its corresponding capability as required or optional. `pointer-force` needs pointer-position; `click-ripple` needs pointer-buttons with usable position; `window-avoid` needs window-rectangles.

Safe global policy can override scene behavior even when a capability exists. Reduced motion, lock, resource caps, and user-disabled observation always win. Nodes do not enable their own sensors.

## Controls

The v1 control descriptor exposes labelled references to portable settings. A custom widget or arbitrary code callback is not supported. The renderer/editor gets the setting's type/default/range from the engine registry, so a package cannot relabel a permission grant as a harmless color slider.

Per-node parameter editing is available in the editor through the node schema. A later extended parameter-binding descriptor must define its own type/range checks; it is not implied by this minimal v1 descriptor.

## Bounded execution

Maximum 128 nodes and 256 edges; global cell, particle, decoder, memory, and time budgets also apply. Several individually small nodes can still exceed an aggregate budget. Reject or require a cheaper preset before activation rather than allocating first and hoping the OS kills the worker.

Each node is deterministic for seed, effective settings, normalized input stream, and time, subject to documented floating-point/GPU tolerance. No node may query the OS directly. Host signals enter through a separate capability-filtered event layer.
