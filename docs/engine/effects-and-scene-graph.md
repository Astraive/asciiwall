# Effects and declarative scene graph

## Stable v1 graph

The draft v1 portable graph has character-frame outputs, a fixed built-in node registry, and explicit input slots. Raster source nodes include conversion internally; generators have no inputs, unary effects use slot 0, and the binary composite uses slots 0 and 1. The [scene schema](../package/scene-schema.md) is the authoritative portable contract. The loader validates IDs, arity, bounds, acyclicity, and output reachability.

## Later typed-graph architecture

The internal engine may evolve toward typed ports for raster, character grid, scalar/vector fields, event impulses, palettes, and presentation layers. Those richer ports are not accepted by the v1 package schema. Publishing them requires a versioned schema/feature extension and explicit type checks. All v1 and proposed typed graphs remain DAGs; this proposal admits no feedback node or cycle exception. A separate future reviewed decision must define any bounded-feedback node and its contract before an exception is considered.

A package contains data selecting built-in node kinds and validated parameters. It does not contain Rust, Lua, JavaScript, shell commands, arbitrary mathematical expressions, or user shaders. This enables rich customization without making every wallpaper an executable extension.

## Effect stages

**Before conversion:** crop, resampling, exposure, tone adjustment, edge extraction, and palette preparation.

**Character space:** glyph substitution, density modulation, cell displacement, color waves, bounded glitch, ripple distortion, character trails, and particle overlays.

**Pixel space:** opacity, tint, vignette, scanline simulation, restrained bloom, crossfade, and approved compositing.

Do not claim equivalent results when the same visual label operates in different spaces. A pixel-space ripple on baked video is not a character-grid ripple.

## Ordering and state

Graph order is explicit, stable, and included in cache keys. A temporal effect owns history tagged by source generation, grid size, font table, and timeline epoch. Any incompatible change resets or migrates that history deliberately. Trail buffers must decay while active and be cleared on privacy/session boundaries.

## Parameter binding

In v1, built-in reaction nodes own their parameterized signal mapping, and exposed UI controls refer only to the approved portable settings registry. There is no arbitrary graph-binding expression field in the v1 schema.

A later explicit binding schema may map approved signals through bounded scale, offset, clamp, easing, smoothing, and dead-zone operators. A pointer x coordinate could drive parallax or window-move speed could drive wave amplitude, subject to a safe maximum. That is an extension proposal, not an already specified portable v1 language.

Bindings declare a missing-signal fallback, such as zero force or a slow autonomous oscillation. A requested optional capability must not turn into an automatic permission prompt or a silent fallback to broad capture.

## Resource accounting

The graph compiler estimates intermediate textures, buffer counts, work per cell, particles, and history size. It rejects impossible dimensions and warns when the selected quality profile cannot fit the declared budget. Runtime monitors actual cost and can reduce only settings explicitly marked adaptive. It must not silently alter artistic constants such as palette or random seed.

## Extensibility

New node types require engine support, a versioned parameter schema, deterministic tests, resource limits, and reduced-motion behavior. A future plugin system is a separate security project, not an unofficial escape hatch hidden in a JSON field.

## FX-015 design decision: versioned typed ports and bounded bindings

**Status: research proposal; not implemented or product-approved.** This decision does not change the stable v1 graph or authorize a new schema, node, feed, or runtime capability. Draft v1 continues to produce the documented character-frame contract: source conversion is internal, the fixed node registry and slot arities apply, the graph is a DAG, and allocations are accounted for before execution. The richer port types and binding syntax below are future-version candidates only.

### Proposed schema and capability boundary

Keep v1 `scene_version: "1.0"` closed and unchanged. A future schema may use a new major scene version (candidate `2.0`) and an explicit, reviewed capability declaration for each required extension. Capability identifiers are negotiated as a fixed set by the client; schema version alone does not imply that a particular port type or node is supported. The owner must define the final version/capability negotiation and compatibility policy before approval.

Each registered node would declare named input/output ports with a single explicit type, direction, and cardinality in its versioned engine-owned descriptor. An edge names source port, destination port, and destination slot. The compiler checks node kind and descriptor version, port existence/direction, exact type equality, cardinality, and required slots before allocating resources. There is no implicit raster-to-grid conversion, coercion, or wildcard `any` port. A conversion is a separately reviewed node with its own bounded cost, version, and deterministic fixtures. Node and port registries are fixed in the engine; package data selects a known entry and cannot extend the registry.

| Proposed value type | Meaning and allowed direct connection |
| --- | --- |
| `raster` | Bounded pixel image/surface; connects only to a raster input. |
| `character-grid` | Bounded character cells and their declared attributes; connects only to a character-grid input. |
| `scalar-field` | Bounded scalar samples with declared domain/range and grid dimensions; matching scalar-field descriptor only. |
| `vector-field` | Bounded vectors with declared coordinate space, units, domain and range; exact descriptor match only. |
| `impulse-stream` | Ordered, normalized events with declared event kind and a finite per-frame count; matching event stream only. |
| `palette` | Finite, ordered color entries and declared color space; matching palette descriptor only. |
| `presentation-layer` | Bounded, ordered output layers with explicit blend/color-space metadata; matching layer input only. |

| Output → input | raster | character-grid | scalar-field | vector-field | impulse-stream | palette | presentation-layer |
| --- | --- | --- | --- | --- | --- | --- | --- |
| raster | yes | no | no | no | no | no | no |
| character-grid | no | yes | no | no | no | no | no |
| scalar-field | no | no | yes | no | no | no | no |
| vector-field | no | no | no | yes | no | no | no |
| impulse-stream | no | no | no | no | yes | no | no |
| palette | no | no | no | no | no | yes | no |
| presentation-layer | no | no | no | no | no | no | yes |

| Candidate arity | Required inputs | Rejection rule |
| --- | --- | --- |
| Source/generator | 0 unless its fixed descriptor explicitly declares inputs | Reject unexpected incoming edges. |
| Unary | 1 matching input at the descriptor's slot | Reject missing, duplicate, extra, or type-mismatched slots. |
| Binary | 2 matching inputs at the descriptor's slots | Reject missing, duplicate, extra, or type-mismatched slots. |
| Conversion | Exactly its named descriptor inputs | Check each port; never infer a conversion. |

These are design recommendations pending approval; v1 continues to use only its current character-frame contract and slot rules.

The table is a type/arity compatibility decision, not a v2 schema. Every connection must also satisfy the fixed descriptor's arity: generators/sources have zero inputs unless registered otherwise; unary nodes have one matching input; a binary node has exactly two matching inputs in defined slots. A multi-port conversion uses separately named ports and an explicit descriptor, not relaxed equality.

### Registry, graph validation, and failure cases

Only engine-reviewed node descriptors are eligible. Each has a stable kind and descriptor version, closed typed parameters/ranges, port declarations, deterministic tests, cost model, reduced-motion policy, export behavior, and explicit ownership of any state. Validation rejects unknown node kinds or descriptor versions, unknown ports/fields, absent or duplicate required slots, incompatible port types, cycles, unreachable work, and any cost that cannot be accounted for. It must do so before allocating node buffers.

Rejection examples (illustrative, future typed-graph validator cases; not v1 fixtures):

- A `raster` source connected directly to a `character-grid` cell effect is a type error. Insert a registered bounded conversion or reject; never reinterpret buffer bytes.
- A `kind: "third-party-glow"` absent from the fixed registry is unknown, even if its parameter JSON looks valid.
- An ordinary edge cycle is rejected. This proposal admits no feedback node or cycle exception; a separate future reviewed decision must define any bounded-feedback node and its contract before an exception is considered.
- A history request with no finite frame/byte bound, or whose checked aggregate exceeds budget, is rejected before allocation.
- An expression string such as `"sin(x)"` is not a binding/operator or a node. Reject it as an unknown field/value and never parse or execute it.

Cycle analysis runs after graph expansion and before allocation. FX-015 admits no bounded-feedback node or cycle exception: v1 and proposed typed graphs remain DAGs. A separate future reviewed decision must define any bounded-feedback node, finite state dimensions, reset/generation rules, deterministic scheduling, and resource bounds before any exception is considered.

### Binding proposal and capability provenance

Proposed bindings are typed data, not expressions. A binding names one fixed, normalized signal source, an expected source type/range, a sequence of allow-listed operators, and one target parameter with an explicit type/range. Each operator has bounded inputs and output; clamp and target validation are mandatory after composition. Recommended candidate limits below are not existing contract guarantees and require owner approval:

| Item | Recommended candidate bound / behavior |
| --- | --- |
| Normalized pointer position | Scalar `x`/`y` in `[0,1]`; available only through an approved pointer-position feed. |
| Normalized window speed | Scalar magnitude in `[0,1]`; available only through an approved window-motion feed. |
| Scale / offset | Finite coefficients, each in `[-8,8]`; reject NaN/infinity and out-of-range serialized values. |
| Clamp | Finite explicit low/high in the target's declared range, low ≤ high; recommend target parallax `[-4,4]` cells and wave amplitude `[0,10]`. |
| Easing | One fixed enum (linear, smoothstep, ease-in, ease-out); no user-defined curve. |
| Smoothing | Bounded temporal state; recommend duration `[0,2000]` ms, reset on feed loss, source-generation change, disable, and privacy/session boundary. |
| Dead zone | Finite fraction in `[0,0.5]` of the declared normalized input domain. |
| Evaluation | At most one fixed operator of each kind in a declared ordered chain; recommend no more than 8 operations and one output per binding. |

Each binding also declares its signal and capability identity; permission requirement; source and target type/range; every operator parameter and range; final clamp; behavior when the signal is absent, stale, denied, or revoked; and whether bounded temporal state is used. Capability provenance comes from the host's approved capability registry and normalized feed, never from a package-provided device name or OS query. A package cannot request, broaden, or trigger permission. If unavailable, the proposed default is a declared neutral value (zero-force/zero-offset); the node passes through or uses only a separately declared deterministic autonomous mode. Required-feed policy, consent UI, and the exact stale/revoked distinction remain owner decisions. Binding evaluation must not silently substitute a more sensitive feed.

Illustrative binding contracts (future-version proposals, not accepted v1 syntax):

- Pointer `x`, source range `[0,1]`, approved pointer-position capability → bounded parallax target `[-4,4]` cells using scale `8`, offset `-4`, clamp `[-4,4]`; missing, denied, stale, or revoked feed yields zero offset/pass-through. No prompt or broader pointer capture is implied.
- Window speed, normalized source range `[0,1]`, approved window-motion capability → wave-amplitude target `[0,10]` using scale `10` and clamp `[0,10]`; missing, denied, stale, or revoked feed uses the declared static base amplitude. No window geometry or other capability may be inferred from this scalar.

These examples distinguish bindings from existing built-in v1 reaction mappings, whose node owns its mapping, and from portable settings-registry references, which remain data references rather than signal expressions.

### Determinism, bounded resources, and lifecycle

For a future implementation, validation and resource planning precede allocation. Use checked arithmetic for dimensions, sample counts, per-node and aggregate bytes/work, event counts, and history. Overflow, unknown cost, or budget excess rejects the candidate graph; never wrap, allocate first, or silently reduce artistic constants. Reuse bounded buffers where practical. A node explicitly owns state, identifies its maximum bytes/frames, creation generation, reset conditions, and retirement point. Retire its resources only after its final output contribution; generation or dimension changes invalidate/migrate state according to a fixed reviewed rule. Histories have finite declared horizons and storage caps.

Deterministic fixtures should pin descriptor/version, seed, dimensions, normalized input events, timeline, and expected outputs. Test accepted boundary values and one-step-outside rejection, type/arity errors, missing/revoked feeds, checked-overflow and aggregate-budget rejection, graph cycles, state reset, and repeated-run identity. Any GPU tolerance must be documented per backend rather than presented as exact CPU identity.

Each new node declares reduced-motion and export semantics before registry admission. Reduced motion disables or caps motion/state according to a reviewed node-specific deterministic rule and must not activate another feed. Export uses the same declared graph semantics and bounded inputs; unavailable live capabilities resolve to the declared neutral/base behavior. No node may turn export into OS observation, permission prompts, network/file I/O, or an unbounded live dependency.

### Extensibility and effect-family classification

| Candidate family or effect | Proposed route | Boundary |
| --- | --- | --- |
| New raster/grid conversion or mixed-space effect | Feature-versioned capability plus reviewed node extension | Publish only with explicit typed ports, schema negotiation, deterministic fixtures, and checked cost. |
| New scalar/vector/impulse generator or field operation | Node extension | Fixed kind, versioned closed parameters, exact port types and bounded state/cost. |
| New palette transformation | Node extension | Fixed palette input/output descriptor and finite entry/cost bounds. |
| Presentation compositing/layer operation | Feature-versioned capability and node extension | Layer order/blend/color-space semantics must be explicit; no implicit presentation socket. |
| Additional procedural scene family that can be represented by existing v1 nodes | Feature-versioned scene preset, if it needs only existing registry semantics | Do not imply a new node or typed port; v1 remains exactly as specified. |
| Procedural family requiring new math, state, or port semantics | Node extension, advertised by a feature version | Reviewed engine implementation and descriptor required; arbitrary formulas are not portable. |
| Effect whose semantics cannot be represented portably or supported by a client | Baked fallback | Ship as bounded ordinary source media under source/media licensing and limits; do not disguise executable behavior as an asset. |

This classification is a recommendation per proposed effect, not approval of any family. A required unsupported feature must fail closed with a clear compatibility error; never silently reinterpret, partially execute, or downgrade into v1.

### Approval and unresolved product choices

**Decision record: proposed for review; no implementation, measured native limitation, or product approval is claimed.** Open decisions for the contract owner include: final major-version/capability negotiation shape and identifiers; whether to support all seven proposed port types and any conversion nodes; exact numeric bounds and operator ordering/chain limits; required-versus-optional feed behavior and stale-feed policy; whether a separate future decision should consider bounded feedback at all; resource budgets and export/reduced-motion behavior per node; and feature-version versus baked-fallback choice for each future family. Until these are resolved and approved, draft v1 alone is conforming and native limitations remain unverified where not measured.

### Migration and unsupported clients

The package/schema document records a future-version valid migration sketch and invalid v1 rejection examples. Compatibility policy: a client missing a required schema version or required typed-port/binding capability rejects the package without executing it or changing the active scene. A client may omit an optional extension only when the package explicitly declares a reviewed fallback with equivalent safe behavior; no client may silently downgrade, strip required semantics, or treat extension data as v1 fields. Migration to v1 is allowed only by an author/editor that rewrites the graph into supported v1 nodes or bakes the effect and then validates the resulting v1 document. No automatic lossy conversion is implied.

## SCN-011 compatibility decision: mathematical generators

**Design proposal only; no mathematical generator is implemented by this decision.** The current scene schema is `scene_version: "1.0"` with a fixed `kind` registry, closed node/parameter objects, and explicit slots. The existing `waves` and `noise-field` kinds are data-selected built-ins, not expression interpreters. The proposed `radial-wave` and `saddle-field` families described in [procedural scenes](procedural-scenes.md#scn-011-design-proposal-finite-mathematical-fields) require a versioned extension; they must not be inferred as capabilities of v1.

These deliberately invalid v1 fragments demonstrate the compatibility boundary:

```json
{
  "scene_version": "1.0",
  "seed": 1,
  "nodes": [{
    "id": "math",
    "kind": "math",
    "params": { "expression": "sin(x / 0)" }
  }],
  "output": "math"
}
```

Reject: `math` is not a v1 built-in kind, and `expression` is not a parameter in any registered node. Do not parse, evaluate, download, compile, or execute the string; it is data that fails schema/registry validation.

```json
{
  "scene_version": "1.0",
  "seed": 1,
  "nodes": [{
    "id": "field",
    "kind": "waves",
    "params": {
      "amplitude": 1,
      "frequency": 1,
      "speed": 0,
      "color": "#ffffffff"
    },
    "ports": { "output": "scalar-field" }
  }],
  "output": "field"
}
```

Reject: `ports` is an unknown node property. A client cannot add a typed scalar-field output or redefine graph typing through extra JSON fields; v1 keeps its fixed node registry, arity, and character-frame output contract.

```json
{
  "scene_version": "1.0",
  "seed": 1,
  "nodes": [{
    "id": "custom",
    "kind": "waves",
    "params": {
      "amplitude": 1,
      "frequency": 1,
      "speed": 0,
      "color": "#ffffffff",
      "callback": "https://example.invalid/generator"
    }
  }],
  "output": "custom"
}
```

Reject: `callback` is not a waves parameter, and URI/callback fields confer no execution rights. Packages may not supply code, shaders, callbacks, or network references for generator execution.

For any future extension, negotiate an explicit schema/feature version and accept only named reviewed built-ins with closed typed parameters; unsupported required features fail closed. It must add schema rejection cases for unknown kinds, fields, ports and expression-like payloads, plus deterministic fixtures and measured resource bounds before claiming conformance. These examples describe expected v1 rejection, not test output from an implementation or proof that unimplemented extension candidates run.
