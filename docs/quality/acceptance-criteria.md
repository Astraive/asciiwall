# Release acceptance criteria

## Milestone acceptance versus product acceptance

An engine-only release may ship CLI conversion without claiming wallpaper support. A Windows alpha may ship with macOS/Linux marked experimental or preview-only. Features are promoted only when their own capability and lifecycle tests pass. Do not block all useful releases waiting for universal Linux parity, and do not advertise untested parity to avoid that distinction.

## Core engine gate

Images convert deterministically under the documented CPU mode; aspect/color/alpha/glyph rules are tested; video uses timestamps and bounded queues; scene graph validation prevents cycles and over-budget allocations; GPU/CPU fallback behavior is explicit; static content stops continuous work.

## Desktop host gate

Wallpaper remains behind icons, never steals focus, preserves clicks/menus/dragging, survives output and session changes, and can always be stopped. Unsupported observation gives a clear fallback. Explorer/shell failure cannot produce an unclosable topmost surface. Native tests record exact supported environments.

## Package gate

Every source kind has valid fixtures; schema and semantic checks agree; extraction cannot escape staging; hashes and actual sizes match; import is transactional; export is portable and preserves attribution; unknown executable content is rejected; conflicting package identity does not overwrite existing bytes.

## Security/privacy gate

All local paths work with network denied. Permissions are separate from package data. Input/window history is not persisted. Decoders/parsers have tested isolation/resource controls for the supported distribution. Untrusted CI never receives signing secrets. A bad catalog signature blocks remote installation without breaking installed wallpapers.

## Usability gate

First-run, import, preview, Apply, pause, stop, customization, export, and recovery work without reading developer documentation. Requested/effective settings are visible. Keyboard-only operation and targeted screen-reader tests pass. Reduced motion and tray-less recovery are tested.

## Distribution gate

Install/update/uninstall work on clean machines. Architecture/codec inventory is accurate. Native helpers are included/signed appropriately. Licenses/notices/source obligations are reviewed. Autostart is opt-in and removable. Current app/config/package versions have a tested rollback/read-only policy.

## Evidence policy

P0 cases in the [matrix](edge-case-matrix.md) cannot be waived when they concern file escape, unauthorized observation, destructive loss, or unrecoverable desktop behavior. Other unsupported features can be excluded from the release's support claim with clear UI/docs.

Performance gates use measured named-machine baselines; no arbitrary universal memory/FPS guarantee. Known issues link to reproduction steps, affected versions, workaround, and ownership. This documentation pack does not claim these product gates have passed.
