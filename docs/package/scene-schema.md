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

## Behavior without optional feeds

A pointer/click/window node becomes a transparent pass-through when its feed is unavailable, unless the manifest declared that capability required. A package containing such a node must list its corresponding capability as required or optional. `pointer-force` needs pointer-position; `click-ripple` needs pointer-buttons with usable position; `window-avoid` needs window-rectangles.

Safe global policy can override scene behavior even when a capability exists. Reduced motion, lock, resource caps, and user-disabled observation always win. Nodes do not enable their own sensors.

## Controls

The v1 control descriptor exposes labelled references to portable settings. A custom widget or arbitrary code callback is not supported. The renderer/editor gets the setting's type/default/range from the engine registry, so a package cannot relabel a permission grant as a harmless color slider.

Per-node parameter editing is available in the editor through the node schema. A later extended parameter-binding descriptor must define its own type/range checks; it is not implied by this minimal v1 descriptor.

## Bounded execution

Maximum 128 nodes and 256 edges; global cell, particle, decoder, memory, and time budgets also apply. Several individually small nodes can still exceed an aggregate budget. Reject or require a cheaper preset before activation rather than allocating first and hoping the OS kills the worker.

Each node is deterministic for seed, effective settings, normalized input stream, and time, subject to documented floating-point/GPU tolerance. No node may query the OS directly. Host signals enter through a separate capability-filtered event layer.
