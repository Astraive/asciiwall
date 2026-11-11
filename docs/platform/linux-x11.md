# Linux X11 adapter

## Desktop window strategy

Create an override-policy-aware, non-activating native window and request the desktop window type through EWMH. EWMH describes window types and stacking information; the window manager and desktop-icon provider still decide real stacking. [S19](../research/source-register.md#s19) Do not assume `_NET_WM_WINDOW_TYPE_DESKTOP` is sufficient on every desktop.

Test against named desktop/window-manager combinations. Some icon managers own a full-screen desktop window that can cover the wallpaper; placing the app below every window could place it below that opaque surface. A bad stack order is a compatibility failure, not a reason to become topmost.

Set suitable input shape/regions for click-through behavior using supported X11 mechanisms. Avoid pointer grabs and never use focus stealing or synthetic click forwarding. Use `x11rb` for protocol integration where practical, with the renderer owning only the resources its native-surface path requires.

## Observation design

Treat input observation as a separate optional feature from hosting. Choose and test an X11 event-observation mechanism that does not interfere with normal desktop dispatch. Do not read `/dev/input` directly, require root, or change device permissions. The app needs mouse effects, not a privileged input logger.

Use EWMH stacking and desktop/state properties where supported to build a conservative window snapshot. Missing properties mean unknown. Distinguish logical desktop/workspace membership from monitor membership. Handle disappearing windows between a list query and a property read without crashing.

## Geometry and compositing

Convert root-window coordinates into monitor-local scene coordinates. Handle RandR topology changes, negative positions, rotation, screen scaling policy, and mirrored outputs. Fractional scale behavior must be tested for the selected desktop; do not apply a second scale factor to coordinates already transformed by the server.

Compositing and transparency differ by environment. An opaque wallpaper is the initial baseline. Alpha wallpaper windows are an optional adapter capability, not a universal requirement. Bound the effect area so a stale window rectangle cannot produce an enormous physics allocation.

## Failure modes

Loss of the X connection terminates the host cleanly. A display-manager restart invalidates all resources. Reconnecting is a supervisor action after session availability returns. Do not retry forever against a dead display socket.

Test desktop icon placement, right-click menus, drag-select, workspaces, compositor on/off where supported, monitor hotplug, screensaver/lock, and multiple instances. Honor per-user startup mechanisms rather than a root daemon. [S47](../research/source-register.md#s47)

## Release decision

Ship a support list of actual tested combinations. Other X11 environments may run but remain experimental. When safe placement cannot be established, fall back to a normal preview or explicitly chosen static image; never cover the user's desktop controls with a window that cannot be interacted with or dismissed.
