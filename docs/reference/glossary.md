# Glossary and naming conventions

## Project vocabulary

| Term | Meaning in this design |
|---|---|
| ASCII | The 7-bit character repertoire. Braille/block/emoji modes are Unicode extensions, not technically ASCII. |
| ASCII frame | A bounded two-dimensional grid of glyph IDs and foreground/background colors with timing/generation metadata. |
| ASCII clip | A precomputed sequence of character frames; retains glyph data but not necessarily original source detail. |
| Baked video | Raster video whose pixels already show ASCII art; glyph choices and grid structure are no longer editable data. |
| Glyph atlas | A texture containing pre-rasterized character masks, indexed by per-cell draw data. |
| Glyph table | An immutable mapping from glyph IDs to supported symbols, masks, metrics, and font fingerprint. |
| Cell aspect ratio | Character-cell width divided by height; needed to avoid stretching source imagery. |
| Density matching | Choose a glyph based primarily on its measured visual coverage/tone. |
| Structural matching | Choose a glyph using spatial shape/edge information as well as tone. |
| Temporal stabilization | A bounded preference for continuity between frames, reset at meaningful discontinuities. |
| PTS | Presentation timestamp; determines when decoded media should be shown. |
| VFR | Variable frame rate; frame durations are not all equal. |
| Generation | A monotonically changed identity used to reject stale asynchronous results after resets or edits. |
| Capability | A specifically available host feature, including permission, scope, freshness, and reason when unavailable. |
| Click-through | The wallpaper does not consume normal desktop pointer input; it does not imply a global observation feed. |
| Passive observer | A separate feed that observes supported events without changing their normal delivery. |
| Occlusion estimate | A conservative approximation of desktop coverage from metadata, not exact per-pixel visibility. |
| Layer-shell | A Wayland protocol family for desktop-layer surfaces on compositors that implement it. |
| WorkerW strategy | A shell-sensitive Windows desktop attachment approach, not a stable public live-wallpaper API. |
| Host adapter | Platform-specific component that owns wallpaper surfaces and reports supported events/capabilities. |
| Presenter | Component that draws frames into a native or shell-owned surface; wgpu is the default standalone path. |
| IPC | Interprocess communication; this design uses local same-user/session channels, not a web server. |
| Backpressure | A bounded producer/consumer policy that prevents queues and memory from growing without limit. |
| Surface lease | Owned lifetime of a valid native presentation target, invalidated on topology/host changes. |
| Immutable package | Imported content whose exact bytes are preserved; customization lives in a separate override. |
| Content digest | A cryptographic hash identifying exact bytes, not proving author identity by itself. |
| Signed catalog | Static metadata authenticated by a separately trusted key and referring to hashed artifacts. |
| Portable setting | A visual parameter that can travel with artwork without granting machine permissions or including private paths. |
| Effective setting | The actual value after precedence, safety, capabilities, accessibility, and power constraints. |
| Sandbox | An enforced restriction boundary; merely launching another process does not create one. |
| SBOM | Software bill of materials identifying components in a distributed build. |
| Golden test | Comparison against a reviewed expected output; tolerance and environment must be defined. |
| Fuzzing | Automated malformed/randomized input exploration under bounded isolated execution. |
| ADR | Architecture decision record documenting context, alternatives, decision, and consequences. |
| Spike | A minimal disposable experiment used to resolve one uncertain engineering assumption. |
| Conformance | Agreement with a defined format/contract and its test cases, not proof of every possible behavior. |

## Naming conventions

Documentation paths use lowercase kebab-case. Rust module files use the language's conventional underscore style. Configuration keys are dotted sections with kebab-case leaf names. JSON manifest fields use the exact snake_case names in the draft schemas. Do not translate machine identifiers when localizing the UI.

The word 'supported' always refers to tested behavior in a named environment; 'proposed' and 'experimental' are deliberate status labels. 'No backend' means no owned hosted application service is required, not that optional public downloads occur without external hosting.
