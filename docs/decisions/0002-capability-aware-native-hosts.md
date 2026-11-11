# ADR 0002 - Capability-aware native wallpaper hosts

## Status

Proposed design decision.

## Options

One generic borderless window across platforms; platform-specific desktop hosts with a shared engine; or depending entirely on another wallpaper application's hosting. The generic-window approach cannot guarantee desktop layering or global input on all systems. [S15](../research/source-register.md#s15) [S16](../research/source-register.md#s16) [S19](../research/source-register.md#s19)

## Decision

Use native Windows/AppKit/X11/layer-shell adapters and explicit Plasma/GNOME companions where needed. Expose host surface, pointer position/buttons, window state/rectangles, power/session, and presentation capabilities independently. A normal preview is always distinct from a verified wallpaper host.

Default standalone rendering uses wgpu. Shell-owned presenters may need a small native bridge because their host owns the scene graph. Keep conversion/timing/settings shared and separately test presentation fidelity. [S56](../research/source-register.md#s56) [S57](../research/source-register.md#s57)

## Consequences

More integration code and a support matrix, but truthful feature behavior. Missing global geometry disables window avoidance instead of prompting for unrelated capture privileges. Click-through remains passive and does not imply receipt of global clicks.

Platform failures degrade to static/preview or a limited capability set. No topmost overlay, root input access, injected shell code, or hidden permission workaround is accepted as a normal fallback.

## Revisit when

A compositor introduces an appropriate documented protocol or a platform exposes a supported live-wallpaper API. Add a new adapter/strategy behind the same capability contract, with evidence rather than assumptions.
