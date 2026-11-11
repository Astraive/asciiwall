# KDE Plasma integration and presentation bridge

## Host-owned wallpaper surface

Plasma offers a native wallpaper-plugin model using QML. [S20](../research/source-register.md#s20) The plugin is the desktop presenter; the Rust application remains the engine, editor, package manager, and controller. Plasma desktops/activities and screen assignments have host-managed configuration. [S21](../research/source-register.md#s21)

Do not assume a QML wallpaper item provides a native surface that can be handed directly to wgpu. Qt Quick owns its scene graph and render-thread lifecycle. An external renderer needs a deliberate integration point. [S56](../research/source-register.md#s56)

## Spike two presentation paths

| Path | Advantages | Costs and gate |
|---|---|---|
| Rust renders offscreen RGBA; bridge uploads bounded frames | Closest pixel parity to reference renderer | GPU readback/copies can dominate; prove throughput and shell responsiveness |
| Rust sends glyph grid/atlas; small Qt-native presenter draws it | Much smaller per-frame data; host-native composition | A second presentation implementation needs conformance tests; no duplicate conversion algorithms |

Start by measuring a low-resolution offscreen path, not by claiming zero-copy. For production, prefer a compact glyph bridge if its maintenance and fidelity are acceptable. Qt graphics resources must be touched on the correct render thread. [S56](../research/source-register.md#s56) Do not hard-wire an OpenGL-only path and then claim Vulkan/software-backend support.

The bridge should render immutable snapshots. IPC delivery must never block the shell's UI thread waiting for a decoder. A stale frame stays visible briefly, then switches to a safe fallback when the controller disappears.

## Windows and pointer data

A wallpaper plugin alone is not a promise of arbitrary window geometry. A separate, versioned KWin/compositor bridge may be necessary for window-state/rectangle events. Prototype against the current documented scripting/extension APIs before marking these capabilities available.

Keep input observation and presentation separate. No interception of normal desktop clicks; do not change selection, icon launching, panels, or context menus. The integration publishes confidence and scope so unsupported desktop-only classification disables reactions rather than guesses.

## Activities and multi-screen ownership

Bind assignments to the actual Plasma desktop/activity context. Changing wallpaper through Plasma settings should not cause an endless controller-versus-shell overwrite loop. Define one authority per assignment, observe external changes, and ask before reclaiming it.

A detached screen releases its presenter. Switching activities restores that activity's assignment without duplicating decoder processes. Shared source conversion is possible only when effective settings/grid/time agree.

## Packaging and failure isolation

Ship the plugin as an explicit companion component. Version its local protocol independently from engine releases and refuse incompatible handshakes. Do not download or install it from a wallpaper package. Do not run user shaders, FFmpeg, or package parsers inside the Plasma process.

Release gate: plugin enable/disable, Plasma restart, activity switch, disconnect/reconnect screen, controller crash, stale IPC, and repeated configuration edits must not freeze or crash the shell. Until that and frame-transport benchmarks pass, label this adapter experimental rather than claiming completed Linux parity.
