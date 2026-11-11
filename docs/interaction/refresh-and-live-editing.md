# Refresh semantics and live customization

## Five explicit operations

| Operation | Meaning | Does not mean |
|---|---|---|
| Redraw | Present current state again | Re-decode media or reset time |
| Reload source | Re-read changed local media/scene after validation | Reattach the desktop window |
| Restart scene | Reset seed/time/history according to user policy | Re-import package or clear library |
| Reattach desktop | Rebuild lost native host/surface relationship | Destroy all settings |
| Rescan library | Discover local package metadata changes | Download a remote catalog automatically |

Expose these through named UI actions and `asciiwall refresh --scope ...`. Windows Explorer's desktop Refresh action is not a universal cross-platform event that AsciiWall can depend on intercepting. Treat shell invalidation as a host-health issue, not a hidden command binding.

## Live editing pipeline

Classify each setting by its invalidation cost: uniform-only, scene-state, conversion, glyph-atlas, source-decoder, surface-recreate, or controller-restart. Changes are validated first and debounced where expensive. Uniform color adjustments may update immediately; font/grid changes prepare replacement resources before swapping.

Every asynchronous job carries an assignment generation and settings fingerprint. Discard completions from older generations. A delayed conversion must never overwrite a newer color/charset choice. Cancel expensive work cooperatively and isolate decoders that cannot cancel promptly.

## Watching local files

Watch only user-selected files or managed library roots. Events can be duplicated, coalesced, or describe a file mid-write. Debounce, verify stable readable bytes, hash, validate, and then atomically replace the active source. Keep the last valid frame if the new file is truncated or temporarily inaccessible.

Do not follow a file-watch event into arbitrary newly referenced paths. Packaged assets are immutable after import; changing a source recipe creates a new package build, not an invisible mutation of a trusted package hash.

## Editor transactions

Maintain a draft separate from the active assignment. Undo/redo changes the draft, not global safety permissions. Auto-preview is optional and bounded. Apply commits validated effective settings; Cancel returns to the saved assignment. A crash must not convert an unfinished slider drag into permanent configuration corruption.

Offer reset-current-control, reset-section, reset-preset-overrides, and factory-reset as distinct actions. Factory reset requires confirmation and does not delete user source media or exported artwork. Cache clear removes derived data only.

## Stress conditions

Test source replaced during decode, file removed then restored, network-mounted local path unavailable, font deleted, settings changed during seek, refresh during GPU loss, repeated shell restart, and multiple editors targeting the same assignment. Use revision checks so a stale settings window cannot silently overwrite a newer CLI change.
