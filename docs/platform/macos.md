# macOS desktop host and permissions

## Hosting model

Use AppKit-owned borderless windows at an experimentally validated desktop-related level. Core Graphics exposes desktop and desktop-icon level keys, but their existence alone does not prove every Spaces/Stage Manager arrangement will behave as desired. [S10](../research/source-register.md#s10) The host must keep wallpaper below icons, out of ordinary application switching, non-activating, and click-through.

AppKit window creation, lifecycle, and relevant callbacks belong on the process main thread. Use `objc2` ownership conventions and `MainThreadMarker` to make that explicit. [S52](../research/source-register.md#s52) Closing settings must not terminate wallpaper hosts; quitting the application must.

Do not create one window with every collection-behavior flag blindly enabled. Spaces participation, full-screen assistance, stationary behavior, Mission Control, and Stage Manager interact; choose a documented compatible combination and test it on supported OS versions. [S09](../research/source-register.md#s09)

## Input observation

An ignoring-mouse-events wallpaper window cannot rely on its own view receiving desktop clicks. A separate passive observer can supply supported mouse events. Global AppKit event monitors see events directed to other applications, while local monitors have a different scope. They do not form a permission-free universal input system. [S11](../research/source-register.md#s11)

Test the exact APIs and event types against current macOS permissions. Do not require Accessibility or Screen Recording merely to render wallpaper. Do not claim every mouse-monitor API requires exactly the same permission as keyboard observation. Keyboard logging is out of scope entirely.

For cursor features, store only the current location and short simulation history in memory. Disable feeds when the session locks or consent is withdrawn. An unavailable monitor is a normal fallback, not a reason to repeatedly prompt.

## Window-aware behavior

Core Graphics window-list metadata is distinct from capturing window pixels. Some attributes may be available while names or contents are protected; availability is version/permission dependent. [S12](../research/source-register.md#s12) [S13](../research/source-register.md#s13) Query only needed bounds, state, and ephemeral identifiers. No screenshots, document titles, or application text are necessary for particles avoiding rectangular obstacles.

If reliable current-Space filtering or geometry is absent, disable the affected reaction. A visible-but-transparent window or an animation in Mission Control should not be treated as an exact opacity mask. Window-aware effects remain estimates even with metadata.

## Screens and lifecycle

Maintain per-screen coordinate transforms, scale factors, and rendering resources. Cocoa and Core Graphics coordinate conventions are not interchangeable: explicitly test origins, vertical inversion, negative coordinates, mirrored displays, rotations, and Retina/non-Retina combinations.

Sleep/resume, display sleep, hotplug, clamshell mode, full-screen Spaces, Fast User Switching, and Stage Manager each invalidate assumptions. Pause first; refresh topology and capabilities; restore only valid assignments. Do not simulate every missed animation step after a long sleep.

A static wallpaper fallback can use the per-screen desktop-image API, called on the main thread. [S14](../research/source-register.md#s14) Restoration follows ownership rules: never replace a user's newer external wallpaper selection.

## Distribution and acceptance

Sign the main app and bundled helpers, configure hardened runtime deliberately, and notarize the actual distributed artifact. Do not add broad entitlements merely to silence failures. [S45](../research/source-register.md#s45) Permission explanations must identify the feature requesting access and work without granting it.

Release tests include permission denied/revoked, all-desktop behavior, Mission Control, Stage Manager, Dock auto-hide, mixed-scale screens, process crash cleanup, and uninstall. Desktop-level windows are the selected approach; native validation is still required before declaring macOS production support.
