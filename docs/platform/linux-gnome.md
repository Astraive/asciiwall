# GNOME integration and shell-safety boundary

## Why a separate adapter

GNOME Shell extensions use GJS and interact with Shell/Mutter concepts. Their code executes in a sensitive desktop process. [S22](../research/source-register.md#s22) A normal background window is not a general substitute for a GNOME wallpaper extension on Wayland.

Use a minimal, explicitly installed extension to manage the background actor and optional bounded metadata. Keep Rust conversion, media decoding, archive validation, and library downloads outside GNOME Shell. No user package code runs inside the extension.

## Presentation is an unresolved engineering gate

GNOME exposes image-content APIs that can accept pixel data and copy it into texture memory. That API existence does not establish an efficient high-frequency wallpaper transport. [S57](../research/source-register.md#s57)

Prototype bounded shared-memory or another appropriate local frame path with a small actor presenter. Avoid sending full video frames as base64 over D-Bus or JSON. A raw 3840 x 2160 RGBA frame is 33,177,600 bytes; 30 such frames per second is about 995 MB/s before extra copies. That arithmetic explains why a naive bridge can be expensive; it is not a benchmark.

An alternative glyph-grid presenter reduces transfer volume but creates a second presentation implementation. Investigate the shell's supported APIs and font/atlas behavior for the exact GNOME version. Neither approach is declared production-ready in this design.

## Metadata bridge

Only publish ephemeral bounds/state/input fields required by enabled effects. Do not publish window titles or screenshot pixels. Treat overview, workspace transitions, multi-monitor workspaces, minimized windows, and fractional scaling as distinct states.

Event callbacks should enqueue or coalesce data; no synchronous calls back into the engine that can block the shell. Unavailable data clears the capability rather than fabricating a window at zero coordinates.

## Extension lifecycle

`enable` creates owned actors, event subscriptions, and a bounded connection. `disable` removes every actor, source, timer, signal handler, and connection. Repeated enable/disable must be idempotent. When the controller exits or crashes, the extension should relinquish the wallpaper layer and reveal a safe host background.

Do not replace unowned shell objects permanently or monkey-patch unrelated internals to keep an animation alive. A shell upgrade can invalidate private assumptions. Publish an explicit tested version range and refuse unsupported releases gracefully.

## Permissions and packaging

Extension installation is an informed user operation, not an import side effect. Flatpak/native packaging affects communication and host access; it does not automatically grant extension control. [S46](../research/source-register.md#s46) Choose distribution and protocol arrangements through a packaging spike.

## Exit criteria

Demonstrate no shell-main-thread stalls, clean disable, recovery from media-worker crash, safe controller loss, correct lock behavior, workspace/overview layering, and acceptable frame-copy costs. Until then, GNOME support means experimental adapter research plus reliable preview/export, not full feature parity.
