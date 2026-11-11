# Feasibility and capability matrix

This is a **design-time expectation matrix**, not a tested support chart. `Conditional` means an API, permission, host plugin, or native spike is required. `No generic path` means do not promise the feature without a separately maintained integration.

| Capability | Windows shell host | macOS native host | Linux X11 | Generic layer-shell | Plasma plugin | GNOME extension |
| --- | --- | --- | --- | --- | --- | --- |
| Local image/video conversion | Yes | Yes | Yes | Yes | Yes | Yes |
| Ordinary preview window | Yes | Yes | Yes | Yes | Yes | Yes |
| Live wallpaper surface | Conditional shell embedding | Conditional native layering | WM-dependent | Protocol-dependent | Host plugin path | Extension experiment |
| Behind normal desktop icons | Must verify shell variant | Must verify Finder layering | Desktop-manager-dependent | Not guaranteed with another desktop layer | Native containment path | Extension/icon integration dependent |
| Global cursor position | Conditional passive API | Conditional observation | Conditional X input | No generic path | Host/bridge-dependent | Host extension-dependent |
| Desktop clicks without consuming them | Conditional observer + hit policy | Conditional global/local monitors | Conditional X input | Not with an empty input region alone | Host forwarding required | Host forwarding required |
| Other-window rectangles | Conditional metadata | Conditional metadata | WM metadata | No generic path | Separate host/KWin bridge | Shell metadata bridge |
| Exact pixel occlusion | Not promised | Not promised | Not promised | Not promised | Not promised | Not promised |
| Per-window content capture | Out of scope | Out of scope | Out of scope | Out of scope | Out of scope | Out of scope |
| Shell refresh interception | Not guaranteed | No equivalent common event | No common event | No common event | Host-specific | Host-specific |
| Login/lock-screen wallpaper | Out of scope | Out of scope | Out of scope | Out of scope | Out of scope | Out of scope |

Windows exposes image wallpaper and window-management APIs, but the WorkerW technique is not a supported live-wallpaper API contract. [S01](source-register.md#s01) [S02](source-register.md#s02) [S03](source-register.md#s03)

macOS exposes desktop-level constants and collection behaviors; global event monitoring, window metadata, and screen capture are separate facilities with different restrictions. [S09](source-register.md#s09) [S10](source-register.md#s10) [S11](source-register.md#s11) [S12](source-register.md#s12) [S13](source-register.md#s13)

X11 hints describe intent, while Wayland restricts ordinary clients and requires explicit compositor protocols. Plasma and GNOME host integrations are different code paths, not aliases for the same generic adapter. [S15](source-register.md#s15) [S16](source-register.md#s16) [S17](source-register.md#s17) [S19](source-register.md#s19) [S20](source-register.md#s20) [S22](source-register.md#s22)

## Runtime representation

Report each capability as `available`, `permission-required`, `unavailable`, `experimental`, or `temporarily-unavailable`. Include a reason, scope, event freshness, and optional remediation. A feature can be available for the settings preview but unavailable for wallpaper mode.

Unsupported optional capabilities degrade to a safe deterministic fallback. Required capabilities reject activation before replacing the existing wallpaper. Never hide missing support by requesting broad unrelated permissions.
