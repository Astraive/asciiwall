# Capabilities and graceful fallbacks

## Capability contract

A capability record is not a boolean. It carries a stable name, availability state, scope, reason code, permission state, freshness policy, and adapter version. Examples include `wallpaper.surface`, `input.pointer-global`, `input.pointer-local`, `input.buttons-passive`, `windows.geometry`, `windows.fullscreen-state`, `session.lock-state`, `display.power-state`, and `presentation.static-fallback`.

The host computes effective capabilities from native support, granted permission, running session, sandbox limits, and adapter health. The engine does not infer availability from an OS name. A package can require basic render features and request optional signal capabilities, but it cannot grant itself permission.

## Fallback order

For pointer effects, prefer approved passive observation; otherwise use host-forwarded/local events where compatible with desktop input; otherwise use a documented idle animation or disable the node. Do not create an invisible input-catching overlay to fake click-through interaction.

For window awareness, prefer a coarse geometry feed with quality markers; otherwise use a host-supported fullscreen/visibility signal; otherwise use user-configured policies or remain active conservatively. A missing window list must not be treated as proven total visibility.

For desktop hosting, fall back to a static exported image through a supported OS path where practical, or preview-only mode. Never fall back by placing a normal always-on-top window over the user's applications. Windows and macOS expose supported image-wallpaper paths distinct from the proposed live host. [S01](../research/source-register.md#s01) [S14](../research/source-register.md#s14)

## Package compatibility

Validate required capabilities before activation. Optional capabilities use per-node fallbacks declared in the scene. Unsupported optional data never causes a permission prompt at import time. The UI shows a compatibility summary such as: 'Runs on this desktop; window-collision effect disabled; pointer effect available after explicit opt-in.'

## Debuggability

`asciiwall doctor --json` should report observed capabilities and reasons, not just a platform string. Include which probe failed and safe next steps. Do not include hardware serials, full window names, or sensitive process command lines in a default support bundle.

## Regression protection

Adapters can quarantine a failed feature independently. A broken window-geometry bridge should not stop image wallpapers; a missing tray should not terminate the engine; denied audio capture should not affect a non-audio scene. Keep fallback behavior covered by the same golden and integration tests as the preferred path.
