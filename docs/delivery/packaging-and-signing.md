# Packaging, signing, installation, and removal

## Release composition

A release contains the desktop controller, native host/helper binaries, restricted media worker, CLI, approved built-in scenes/font notices, exact native codec libraries where bundled, schema/version metadata, and third-party notices. Do not download a missing runtime silently on first use.

Publish architecture-specific artifacts with an explicit codec/backend/support inventory. 'Supports MP4' alone is incomplete because a container can hold unsupported codecs. Verify actual installed encoders/decoders through `doctor` and smoke fixtures.

## Windows

Offer a per-user installer and a portable distribution where feasible. Register `.asciiwall` file association only with consent; opening a package starts inspection/import, never automatic activation or executable hooks. Autostart is opt-in and removable.

Sign production binaries/installers through a controlled release process. Test on clean machines without developer runtime leftovers. Verify helper discovery, native DLL loading paths, architecture mismatch handling, uninstall while running, and no administrator requirement for normal wallpaper behavior.

Portable mode must use an explicit writable data directory and show when the media/cache size exceeds available disk space. It must not change system-wide settings merely because the executable was launched from a USB drive.

## macOS

Bundle helpers and libraries with correct runtime paths; sign nested components and the app using the supported distribution process. Hardened runtime, entitlements, and notarization are part of distribution, not an optional UI checkbox. [S45](../research/source-register.md#s45)

Test first launch, quarantine/Gatekeeper behavior, permission denial, moving the app after installation, universal versus architecture-specific builds, and removal of login items. Do not add broad permissions to fix an uninvestigated failure.

## Linux

Begin with a documented native/portable package for selected host combinations. AppImage or distribution packages may be appropriate, but native media/graphics dependencies and ABI baselines must be verified. Flatpak has host-access restrictions and does not automatically install or control a shell extension. [S46](../research/source-register.md#s46)

Package Plasma/GNOME companion adapters separately with explicit compatibility and installation steps. A generic Linux tarball does not establish support for every desktop. Desktop entries/file associations/autostart follow the relevant per-user desktop conventions. [S47](../research/source-register.md#s47)

## Codec and font compliance

Record exact FFmpeg build flags and source/notices obligations. [S33](../research/source-register.md#s33) Verify whether a release includes GPL-enabled components or platform-native codecs and label the distribution accordingly. Review any patent/distribution obligations for target jurisdictions rather than making a blanket legal claim.

Only include fonts with reviewed redistribution rights and required notices. [S53](../research/source-register.md#s53) This documentation delivery contains no font binaries.

## Updates and rollback

Initial binary updates can be manual signed downloads. Verify integrity before replacing the app, keep a recoverable prior version where appropriate, and do not rewrite incompatible configs destructively on downgrade. Package updates are immutable installs, not executable updates.

## Removal

Stop owned surfaces/workers, remove owned startup/file-association entries where appropriate, and ask separately about local settings/cache/managed library. Never delete original referenced user media. Test uninstall after a crash and after the user changed wallpaper externally; restoration must respect ownership.
