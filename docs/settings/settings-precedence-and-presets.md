# Settings precedence, presets, and customization

## Composition order

Compute requested settings in this order, lowest priority first: engine defaults; package visual suggestions; selected user preset; per-assignment overrides; temporary session edits. Then apply hard safety limits, permission/capability constraints, user accessibility policy, and power/resource policy to derive effective settings.

Safety is not merely another layer that a higher-priority package can overwrite. Permissions are held outside portable visual data. A package requesting pointer repulsion does not enable pointer observation. Machine paths, autostart, remote catalogs, and app rules never come from package suggestions.

## Requested versus effective

Each control can explain its source, requested value, effective value, and overriding rule. Example: requested 60 FPS from a package; effective 10 FPS due to battery profile. The editor must not silently rewrite the package's stored value to 10, because the original request should return when the policy clears.

An unsupported feature should be marked unavailable or preview-only, with the exact missing capability. Do not gray out the entire settings page because one observer is unavailable.

## Preset model

A preset is a named versioned partial set of portable visual controls plus optional local assignment preferences. Separate portable and local sections physically. Package export includes only the portable subset. A preset can inherit from one other local preset, with cycle detection and a bounded depth of four.

Resolve inheritance into a concrete snapshot when exporting. Exporting an identifier that only exists on the author's machine would not be portable. Keep the original package immutable; a user's customization is a separate override document.

## Reset behavior

Reset control removes its override, revealing the next lower layer. Reset section removes overrides for that section. Reset assignment returns to its chosen preset/package. Factory reset clears app configuration after confirmation but does not delete library assets or user files. Cache clear is not any of those operations.

Undo/redo operates on editor drafts with a bounded command history. Apply commits a revision; Cancel discards uncommitted edits. A permission grant, file deletion, library download, or autostart change is a separate action and is not hidden in slider undo history.

## Custom UI parameters

A scene can expose labelled controls mapped to allowlisted built-in parameters. The descriptor names type, bounds, step, default, group, and help text. It cannot create arbitrary native widgets, execute code, or point at application-only settings. The engine validates that the mapping target exists and both bounds intersect safely.

For v1, the scene schema exposes the core visual controls and built-in node parameters only. More expressive node editors are later UI work, not an unrestricted scripting language smuggled through settings.

## Sharing

Save a customized wallpaper as a new local variant. Preserve attribution and base package identity in provenance. A portable export contains resolved visual choices and permitted assets, not diagnostic logs, local path history, machine identifiers, app rules, or permissions.
