# Capabilities and graceful fallbacks

## Capability contract

A capability record is not a boolean. It carries a stable name, availability state, scope, reason code, permission state, freshness policy, and adapter version. Examples include `wallpaper.surface`, `input.pointer-global`, `input.pointer-local`, `input.buttons-passive`, `windows.geometry`, `windows.fullscreen-state`, `session.lock-state`, `display.power-state`, and `presentation.static-fallback`.

The host computes effective capabilities from native support, granted permission, running session, sandbox limits, and adapter health. The engine does not infer availability from an OS name. A package can require basic render features and request optional signal capabilities, but it cannot grant itself permission.

## Implemented contract boundary (WPH-001)

`ascii-platform` now defines the v1.0 versioned, allocation-free capability snapshot (10 baseline feeds, at most 32 scoped records) with explicit unknown/denied/unsupported/available states, freshness, confidence, adapter identity/version, health, and host restrictions. Feature dependencies are resolved narrowly; only declared dependents become ineffective after a feed is revoked. Event batches and queues are fixed at 32 entries and report the snapshot revision that must be reconciled after overflow.

The portable surface lease registry binds an adapter-owned surface to a `MonitorIdentity` and non-wrapping `ascii-core::Generation`. Native handles remain private to the adapter; stale leases fail after retirement/recreation even if the operating system reuses a handle. Requested/effective presentation resolution downgrades to preview or a user-consented static wallpaper path, never to an always-on-top window.

This is a reusable contract and deterministic lifecycle model, not a production Windows host or a claim that any capability is supported on a named OS/session/GPU tuple. The Windows hierarchy probe is read-only; below-icon placement, focus preservation, Win+D, Explorer restart/recovery, and icon interaction remain unverified. No Linux or macOS adapter is included.

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
