# Permissions and privacy by capability

## Principle

Rendering a wallpaper, observing pointer events, reading window rectangles, playing sound, analyzing a chosen audio file, and capturing system audio are separate capabilities. Do not ask for all permissions at first launch. Granting one does not authorize the others.

| Feature | Needed information | Default | Failure behavior |
|---|---|---|---|
| Image/video wallpaper | User-selected local files | Enabled after selection | Explain inaccessible/moved source |
| Pointer attraction | Current pointer position | Off until enabled | Disable reaction only |
| Click ripple | Passive button event and position | Off until enabled | No synthetic replay/capture |
| Window avoidance | Bounds/state, not content | Off until enabled | Disable or fade obstacles |
| Fullscreen pause | Relevant output/window state | Best effort | Configurable conservative fallback |
| File-audio reaction | Chosen audio samples | Later opt-in | Silent visuals continue |
| System audio or microphone | Protected capture stream | Out of v1 baseline | No automatic permission prompt |
| Online catalog | Explicit HTTP download | Off by default | Local library remains available |

## Scope and disclosure

The settings panel shows what is observed, where it is processed, and how to turn it off. Transient cursor/window state stays in memory. Logs contain capabilities and error codes, not coordinates, window titles, document paths, or media contents by default.

On Windows and X11, an API may be technically accessible without a system prompt. That does not remove the need for an application-level consent switch. On macOS, exact permission requirements depend on API/event type and OS version; verify rather than using one blanket permission label. [S11](../research/source-register.md#s11) [S12](../research/source-register.md#s12) On Wayland, absent protocol support is not fixed by repeatedly asking for consent. [S15](../research/source-register.md#s15) [S16](../research/source-register.md#s16)

## Revocation

Changing a permission toggle immediately unsubscribes the feed and clears recent input/window history. OS-level revocation updates the capability state. The UI must not silently retain a granted indicator after the OS rejects calls.

Wallpaper packages may declare optional needs and recommended fallbacks; they cannot grant permissions, enable networking, change autostart, or install a shell extension. Import never initiates an authorization prompt. Applying a package explains optional features before the user elects to enable them.

## Data minimization

No screenshots or OCR are required for window-aware effects. No keystrokes, clipboard, browser history, installed-app inventory, or analytics identifier. Diagnostics exports are previewable and redacted, with full paths available only through a separate explicit action.

A local-only claim is tested with network denied for import, conversion, playback, settings, and export. Dependency behavior and FFmpeg protocols must be included in that test; the absence of an HTTP client in the UI alone does not prove offline operation. [S32](../research/source-register.md#s32)
