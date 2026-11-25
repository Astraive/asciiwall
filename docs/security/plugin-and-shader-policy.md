# Plugin, script, and shader policy

## Stable v1: data, not code

A `.asciiwall` package may select built-in generators/effects, provide bounded parameters, include permitted local media/fonts, and declare optional visual reactions. It may not contain executable Rust, JavaScript, shell scripts, native libraries, WebAssembly, arbitrary WGSL, or external process commands.

This still allows extensive customization: colors, fonts, character sets, density, matching, source media, graph composition, seeds, timing, reactions, transitions, and quality settings. 'Fully customizable' does not mean 'run arbitrary code from any wallpaper download.'

## Why shaders are not automatically safe

A syntactically valid shader can consume excessive GPU time, allocate expensive resources through its host, trigger driver bugs, or hang a display path. Validation alone is not a dependable execution-time budget. Do not permit unreviewed shaders in the same process/device path that keeps the user's desktop responsive.

Any future built-in WGSL is engine-owned, reviewed and versioned with the engine release, bounded by host resource limits, and not replaceable by a package. Custom color ramps or effect parameters are data, not source-code substitution. No package-provided shader executor is implemented.

## Future extension research

A future extension system could use a restricted WebAssembly ABI with no default WASI file/network/process access, memory limits, metering, deterministic imports, and a versioned frame/event interface. It still needs a threat model, timeout strategy, parser review, and native boundary tests before release.

Do not claim WebAssembly makes arbitrary plugins safe by itself. Native GPU access and external imports can erase isolation. Signed plugins authenticate an author/key, not good behavior.

## Developer mode

No developer mode is implemented. Any future developer-only code loading must require a clearly separate explicit mode, prominent warnings, and no association with `.asciiwall` auto-import. A package must never enable it; neither should a catalog, config migration, or hidden URL.

Community-library publication remains data-only until the extension security model has an independent release decision. Keeping this boundary small is the recommended way to launch a dependable open-source wallpaper engine.

## Portable validation evidence and limits

`ascii-package` validates bounded manifest and scene documents. Its v1 scene schema accepts only the closed built-in node registry and bounded declarative parameters; integration tests exercise accepted built-in scene controls and reject Rust, JavaScript, shell, native-library, and process-command node payloads. Existing scene tests also reject arbitrary WGSL, WebAssembly, and script node kinds. Manifest settings reject a package-supplied `developer-mode` value.

These tests establish only the portable document-validation boundary. The current repository has no application package loader or developer-mode implementation; catalog activation and hidden-URL paths are not package-validator inputs. Archive extraction, runtime execution, and native bridge behavior have not been audited or accepted by these tests.

## Native shell integrations

Plasma/GNOME companion components are trusted application adapters, not wallpaper plugins in the package sense. Users install/update them through a reviewed distribution path. Their code must be minimal because a fault can affect the desktop shell. They never execute user-provided conversion or media parser code in the shell process.

The trusted-adapter distinction is policy, not a completed native audit: Plasma/GNOME companion code and its distribution/update path require separate review. No claim is made here that shell integrations or native media/conversion boundaries have passed security acceptance.
