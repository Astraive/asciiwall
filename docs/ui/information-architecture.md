# Desktop application information architecture

## Primary navigation

Library contains installed packages, user imports, favorites, tags, compatibility filters, and optional online catalog. Editor contains preview, source/conversion, style, scene/effects, interaction, transitions, and export. Displays contains assignments, mirror/span/sync, scale diagnostics, and per-output status. Settings contains performance/power, permissions, storage, accessibility, startup, and advanced diagnostics.

Keep ordinary users on sensible presets while making detailed controls searchable. A large settings surface is useful only if the current effective behavior is explainable.

## Persistent status

Show the selected output, active package/version, playback state, effective FPS/profile, pause reasons, missing capabilities, and unsaved changes. Do not use a green 'running' label when the host is in preview-only mode or a static fallback.

The system tray/menu bar offers Open, Pause/Resume, Next local playlist entry where supported, Display assignments, Refresh/Repair, and Quit. Tray availability is platform-dependent; relaunching the executable must always reopen controls, even on tray-less desktops. [S51](../research/source-register.md#s51)

## Main flows

First launch: explain local operation; choose a built-in safe scene; preview; select display; Apply. No account, network permission, global-input prompt, or library download is required.

Advanced reactive flow: choose an effect; see its needed capability and scope; preview with local synthetic input; explicitly enable observation where supported; apply. Permission denial leaves the rest of the app usable.

Import flow: inspect; validate; add to library; optionally open editor; Apply separately. Export flow: resolve assets; review rights/size/privacy; choose destination; package and revalidate.

## Error design

Separate invalid media, unsupported capability, missing file, denied permission, decoder crash, graphics failure, and package conflict. Each message includes a safe short explanation and one useful action. Avoid raw native stack traces in the main interface.

Show progress for long work with cancel. Progress is phase-based when exact duration is unknown; never fabricate a percentage from an untrusted media duration. Keep the last valid wallpaper while a candidate loads.

## Navigation and state

Closing a panel preserves a draft only after explaining that it is not applied. Switching output in the editor should prompt to save/discard an unsaved assignment draft, not apply it to the next output accidentally. Keyboard shortcuts have visible labels and never require global keyboard monitoring.

No remote webview is required. The selected UI stack is native Rust egui with explicit accessibility testing; use shared engine previews rather than a separate web-only ASCII renderer.
