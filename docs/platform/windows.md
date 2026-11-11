# Windows wallpaper and interaction adapter

## Native placement

The supported Windows wallpaper-management interface manages image/slideshow settings; it is not a documented API for a custom continuously rendered GPU wallpaper. [S01](../research/source-register.md#s01) The practical live approach is a native window attached into the Explorer desktop hierarchy, commonly involving Progman/WorkerW/DefView. This is an explicitly shell-sensitive integration, not a stable public wallpaper contract. [S03](../research/source-register.md#s03) [S50](../research/source-register.md#s50)

Implement a small adapter that discovers current desktop windows, validates their relationships, and chooses a tested attach strategy. Never assume a hard-coded parent, one WorkerW layout, one magic-message sequence, or a numeric window handle remains valid after Explorer restarts. Use bounded discovery timeouts. Keep all unsafe Win32 calls inside reviewed modules.

Before attach, establish the intended DPI-awareness context, styles, non-activation behavior, and ownership. `SetParent` alone does not fix incompatible styles or DPI-awareness modes. [S02](../research/source-register.md#s02) Do not inject code into Explorer, patch its memory, subclass foreign processes, or require administrator privileges.

## Surface lifecycle

Create hidden; configure size and rendering; present a valid first frame; attach; verify placement and non-focusability; then reveal. Failed verification detaches and shows an explanation in the controller. A window must never temporarily appear above normal applications during recovery.

Observe display changes and re-enumerate outputs. The dimensions in a display-change message are not a full multi-monitor topology description. [S58](../research/source-register.md#s58) Explorer termination invalidates shell references; discard them and rediscover. Use a bounded exponential backoff, with a circuit breaker after repeated failures.

Validate Win+D, Show Desktop, Task View, virtual-desktop changes, desktop icon toggling, auto-hide taskbars, custom scale, desktop refresh, shell restarts, and remote-session transitions. Keep a lightweight controller window/message pump alive even when the settings UI is closed.

## Pointer and window feeds

A passive raw-input registration is a candidate for background mouse button observation. Do not use flags that suppress normal legacy delivery or capture the device. Raw relative movement is not a global cursor location; obtain and transform desktop coordinates separately. [S07](../research/source-register.md#s07) [S08](../research/source-register.md#s08)

Use an out-of-context WinEvent hook plus an initial snapshot to maintain window metadata. Callbacks enqueue bounded events and return immediately; they never perform conversion or GPU work. The observer needs a functioning message loop. [S04](../research/source-register.md#s04)

Normalize window rectangles deliberately: `GetWindowRect` can be DPI-virtualized and include invisible resize borders; DWM attributes offer different bounds. [S05](../research/source-register.md#s05) [S06](../research/source-register.md#s06) Exclude own windows, shell chrome, cloaked/minimized/off-desktop windows where reliably detectable. Fullscreen is a heuristic over output overlap/state, not an infallible game detector.

## Desktop-only clicks

Passive observation sees more than the wallpaper. A desktop-only reaction requires a trustworthy hit-policy result. Desktop icons, context menus, drag-selection, and app windows must still receive their original input. Never capture then replay clicks to emulate pass-through. When hit testing is uncertain, suppress the reaction; do not guess that the click was on empty wallpaper.

## Session and recovery constraints

Stop observation/rendering at lock, suspend, secure-desktop transitions where detectable, and session disconnect. Do not attempt to render into UAC/login desktops. Power events inform the scheduler; the app must not keep the computer awake. [S59](../research/source-register.md#s59)

On normal exit, destroy only owned surfaces and restore only settings the app actually changed. If the user changed static wallpaper independently, do not overwrite their newer choice. Forced termination cannot guarantee an exit animation or restoration callback. Keep a safe static background configured only with explicit user consent.

## Acceptance gate

A Windows release must survive 20 attach/detach cycles, Explorer restart, mixed-DPI monitor movement, lock/unlock, and rapid Apply changes without losing icon interaction, taking focus, leaking windows, or leaving a topmost surface. These are proposed tests, not results already measured.
