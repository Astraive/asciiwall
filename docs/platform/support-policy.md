# Platform support policy

## Support is a tested tuple, not an operating-system logo

Track OS build, session type, desktop/compositor version, GPU backend, packaging format, and enabled capabilities. A successful preview window does not establish successful wallpaper placement. A successful wallpaper does not establish cursor observation or window geometry access. This policy is a proposed release contract, not a claim that AsciiWall has already passed native testing.

| Tier | Proposed meaning | Release requirement |
|---|---|---|
| Supported | Named combinations pass the release matrix | Installation, placement, input preservation, suspend, recovery, removal tested |
| Experimental | Useful implementation with known integration gaps | Prominent status and an exit/fallback path |
| Preview/export only | Engine works but desktop host does not | Normal app window and static export remain usable |
| Unsupported | No safe implementation or incompatible ABI | Explain exact reason, do not keep retrying |

Initial development order is Windows, macOS, then selected Linux hosts. Do not publish a blanket Windows 10/11 or all-Wayland promise. Select minimum versions after the native spikes and record them in release metadata. Older systems may be community-supported without receiving the same test guarantee.

## Capability contract

Expose wallpaper-surface, pointer-position, pointer-buttons, window-rectangles, window-state, lock-state, battery-state, per-output-scale, static-wallpaper-setting, and tray-availability independently. Permission refusal is a valid normal configuration. Core rendering must not depend on global input access.

Each integration supplies its identity/version, supported capabilities, restrictions, and health. The UI displays requested versus effective behavior. For example: 'Window repulsion disabled: compositor does not expose window rectangles.' It must not show an enabled control that silently does nothing.

## Linux-specific policy

X11 desktop hints, Wayland layer-shell, Plasma wallpaper plugins, and GNOME extensions are different products within the integration layer. A normal Wayland client cannot simply obtain arbitrary global geometry; layer-shell availability must be discovered. [S15](../research/source-register.md#s15) [S16](../research/source-register.md#s16) [S19](../research/source-register.md#s19) [S20](../research/source-register.md#s20) [S22](../research/source-register.md#s22)

For Qt/GNOME hosted adapters, the shell owns presentation. The shared engine can supply frames, glyph data, and settings, but a host-native presenter may be necessary. Do not claim every adapter can embed the same native wgpu surface. [S56](../research/source-register.md#s56) [S57](../research/source-register.md#s57)

## Release evidence

Keep machine-readable test records with exact environment, test date, binary hash, driver version, outcomes, and issue links. An OS update can regress an undocumented integration even when no engine code changes. Windows issue reports demonstrate this risk but do not establish how frequent it is. [S03](../research/source-register.md#s03) [S50](../research/source-register.md#s50)

A broken adapter should fail into a safe static image or preview, never a topmost full-screen window. Per-user startup is opt-in; no admin service is needed. Remote sessions, virtual machines, and software-rendering drivers get separate coverage rather than inheriting desktop claims.
