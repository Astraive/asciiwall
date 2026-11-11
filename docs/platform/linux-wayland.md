# Wayland hosting and capability limits

## The architectural boundary

Wayland isolates ordinary clients from each other's surfaces and does not provide every client with a global coordinate space. [S15](../research/source-register.md#s15) A wallpaper client should be designed around that boundary, not around an X11 assumption that every window rectangle can be enumerated.

Where the compositor advertises layer-shell, create a background-layer surface per output, anchor it correctly, negotiate configure events, and manage output scale. `smithay-client-toolkit` provides client-side helpers; it cannot make an unsupported compositor advertise the protocol. [S16](../research/source-register.md#s16) [S23](../research/source-register.md#s23)

A normal maximized window is not a wallpaper fallback. Lack of the required background protocol means select a compositor-specific adapter, static wallpaper route, or preview-only mode.

## Click-through is not observation

An empty input region allows pointer input to pass to other surfaces. It also means the wallpaper surface receives no normal pointer interaction in that region. Layer-shell does not turn click-through into a global cursor or click feed. [S16](../research/source-register.md#s16)

Offer reactions only when a separate, legitimate compositor integration supplies them. Never silently request screen sharing, remote desktop control, privileged input devices, or root access to bypass this restriction. A portal designed for user-authorized capture/control is not a default wallpaper permission escape hatch.

## Window metadata limits

Foreign-toplevel protocols may expose titles, identifiers, state, or output association. They are not a universal stream of global window rectangles. [S17](../research/source-register.md#s17) [S18](../research/source-register.md#s18) Do not advertise window-edge collision solely because a toplevel-list protocol is available.

Use the minimum fields needed; discard titles. Fullscreen pause may be possible where state/output feeds exist even when geometry reactions are not. Capability records should capture that difference explicitly.

## Configure, frames, and outputs

Respect the compositor's configure/ack lifecycle. Render only after a valid size, and suspend presentation for zero-size/unmapped outputs. Use compositor frame callbacks/presentation pacing as appropriate; do not busy-loop when the surface is not being refreshed.

Output names and logical arrangement can change. Persist a best-effort local display match, not a claim that a connector label is globally stable. Rebuild coordinate transforms on scale/rotation changes and handle fractional scaling support explicitly. Do not infer global pointer coordinates from surface-local motion.

## Optional bridges

A named compositor bridge can deliver bounded metadata to the engine through same-user local IPC. Version the bridge, define exact fields and consent, and validate every message. Installing a bridge requires user action; a wallpaper package cannot install one.

Examples of bridge families include Plasma/KWin and GNOME Shell integrations, documented separately. These run in or near the shell and need stricter compatibility testing than standalone layer-shell surfaces.

## Expected feature downgrade

On plain layer-shell with no observer: procedural/image/video wallpaper, per-output settings, transitions, local clock reactions, and manual pause work. Global cursor effects, desktop click ripples, and geometric window avoidance do not. That is a supported limited mode, not an application malfunction.
