# Support

## Current Status

AsciiWall is currently a **research, specification, and repository-infrastructure baseline**. **No executable desktop app, CLI binary, installer, or published wallpaper package collection is available yet.**

If you are looking for build or installation binaries today, none exist in this repository yet — see [README.md](README.md) and [CHANGELOG.md](CHANGELOG.md) for current repository contents and [docs/roadmap.md](docs/roadmap.md) for planned milestones.

## Where to Get Help

- **Orientation and specifications:** start with [docs/start-here.md](docs/start-here.md), [docs/plan.md](docs/plan.md), and [docs/platform-support.md](docs/platform-support.md).
- **Questions and design feedback:** use [Astraive/asciiwall Discussions](https://github.com/Astraive/asciiwall/discussions) (when enabled) rather than opening bug reports for general questions.
- **Repository scripts or documentation issues:** file a report via `.github/ISSUE_TEMPLATE/` (choosing **Bug report**, **Feature proposal**, **Backlog task**, **Research or risk spike**, or **QA verification or milestone gate**).
- **Security vulnerabilities:** **do not** ask for help publicly; follow [SECURITY.md](SECURITY.md) and use [private vulnerability reporting](https://github.com/Astraive/asciiwall/security/advisories/new).
- **Curated wallpaper recipes and artwork collection:** visit the companion repository [`Astraive/asciiwall-library`](https://github.com/Astraive/asciiwall-library).

## Platform Support Boundaries (Planned Application Policy)

Once AsciiWall implementations ship, platform support follows [docs/platform/support-policy.md](docs/platform/support-policy.md):

- **Support is a tested tuple, not an OS logo.** A platform claim specifies the exact OS build, session type, desktop/compositor version, GPU backend, packaging format, and verified capabilities.
- **No blanket cross-platform guarantee.** Windows is the first target; macOS and selected Linux adapters (X11, layer-shell Wayland, KDE Plasma, GNOME) advance independently as their acceptance gates pass.
- **Preview vs. wallpaper placement:** a working GPU preview window does not imply desktop wallpaper placement works on every shell or compositor.
- **Permission and capability limits:** missing compositor protocols (such as global window geometry on Wayland) or denied OS permissions disable optional reactions with an explicit explanation; core playback does not depend on global input hooks or network access ([docs/platform/permissions-and-privacy.md](docs/platform/permissions-and-privacy.md)).

## Preparing Bug and Diagnostic Reports (Once App Exists)

When filing bugs against future executable builds, follow [docs/quality/diagnostics-and-troubleshooting.md](docs/quality/diagnostics-and-troubleshooting.md):

1. Include expected vs. actual behavior, reproduction steps, and whether the issue occurs in preview or desktop placement.
2. Provide your exact environment tuple (OS build, session/compositor, GPU/driver, app commit/version).
3. Attach **sanitized** `asciiwall doctor --json` diagnostics only after reviewing and redacting private host paths or display details.
4. Never upload personal media, unconsented copyrighted assets, proprietary fonts, or memory dumps.
