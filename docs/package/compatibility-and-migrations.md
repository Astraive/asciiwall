# Package compatibility and migrations

## Independent versions

Track application SemVer, package-format version, scene version, clip binary version, catalog schema, settings/config version, and IPC version independently. Incrementing the app version does not automatically change every format.

A package can be syntactically valid but incompatible with the installed engine, disabled codec build, unavailable font feature, or current desktop capabilities. The library stores these states separately and explains them before Apply.

## Compatibility algorithm

Validate format first; compare engine minimum using a SemVer parser; resolve source-implied and declared engine features; validate scene/clip versions; confirm supported codecs and budgets; compute required/optional platform capabilities; then derive effective settings and fallbacks.

Unknown required feature means no activation. Unknown optional visual extension is not silently executed; v1's closed schema rejects unknown fields. A package using a new node kind must use the appropriate updated format/feature declaration, even if its metadata says it works on an old engine.

## Fallbacks

An optional static fallback can be displayed with user consent when live activation is incompatible. Import for storage is allowed independently from activation. Do not silently substitute a completely different wallpaper and call it success.

Missing pointer/window access should make optional effects pass through, not reject the entire scene. A package that truly requires interaction should say so and may be preview-only on a limited host.

## Migration policy

Never mutate imported immutable bytes or rewrite a published version in place. Migrate into a new local derived variant, retain source hash/provenance, and validate the output. Preserve license and attribution files. Back up user overrides separately from package contents.

Migration cannot invent missing source media, reconstruct editable glyphs from baked video, or grant permissions. A destructive conversion requires an explanation and explicit action.

## Version conflict and rollback

Same ID/version/hash is the same package. Same ID/version/different hash is an integrity/provenance conflict. Different versions coexist until the user removes them. Assignments pin an exact installed version/hash; a catalog refresh never silently changes running artwork.

Rolling back the engine may make a package or config unsupported. Read-only inspection and static export should remain possible where safe, but never pretend an old parser understands a newer binary format.

## Format-freeze gate

Freeze v1 only after at least one valid example of each source kind, malformed-package fixtures, deterministic pack tests, Windows/macOS/Linux path tests, clean-machine import, and library catalog verification agree. This document set supplies draft schemas/examples; implementation conformance remains future work.
