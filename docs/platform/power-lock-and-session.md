# Power, lock, session, and shutdown policy

## Priority model

Maintain a set of active pause reasons rather than one mutable pause flag. User pause, lock, suspend, display sleep, policy, fullscreen, battery, and error can overlap. Clearing fullscreen must not resume a manually paused wallpaper.

Priority order: safety/invalid host, lock or suspend, user pause, explicit application rule, power policy, visibility optimization, normal playback. Policy may choose static instead of fully stopping. Explain the effective reason in the tray and diagnostics.

## Suspend and resume

On suspend: stop new decode requests, cancel nonessential jobs, persist bounded state, release fragile surfaces where required, and stop observer feeds. Do not block OS sleep to finish a transition or render. Windows exposes power notifications for this lifecycle. [S59](../research/source-register.md#s59)

On resume: refresh topology and capabilities; verify handles; rebase clocks; seek/restart media according to the selected resume policy; reset stale temporal history. Clamp simulation delta and never integrate hours of missed frames.

Lock screen is out of scope for v1. Wallpaper is for the logged-in desktop, not login/UAC/secure desktops. No capture or input data should cross the lock boundary. Session switching keeps assignments per user and avoids shared writable IPC endpoints.

## Battery and thermal behavior

Default battery profile reduces FPS and grid density, with an option to use a static snapshot. Low-battery threshold and hysteresis prevent rapid on/off oscillation. A missing battery or thermal sensor is unknown, not a zero reading. Do not require privileged sensors.

A hidden static image needs no continuous conversion/render loop. An animated source paused by policy should release or idle its decoder rather than silently decode and discard indefinitely. Depending on source/resume mode, retaining a bounded last frame is acceptable.

## Shutdown and exit animations

A user-requested stop may play a bounded exit animation while the host is healthy. OS shutdown, forced termination, crash, or permission loss takes the immediate safety path. Never delay logout for cosmetic effects. State this limitation in UI descriptions.

Restoring prior static wallpaper is conditional on ownership and consent. If AsciiWall never changed the OS wallpaper setting, destroying its surface should reveal the existing background without rewriting configuration.

## Autostart

Opt-in per-user startup only. Start after the desktop is ready, retry attach with a bounded backoff, and expose a safe-mode launch that loads no last wallpaper. Linux uses the desktop's user startup mechanism rather than a root service. [S47](../research/source-register.md#s47)

Detect repeated startup failures and suppress automatic loading until the user repairs or chooses another wallpaper. Never trap a user in a crash loop at login. Installation, removal, and autostart disabling must be testable independently of the current scene.
