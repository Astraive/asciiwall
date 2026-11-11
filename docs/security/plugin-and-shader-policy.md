# Plugin, script, and shader policy

## Stable v1: data, not code

A `.asciiwall` package may select built-in generators/effects, provide bounded parameters, include permitted local media/fonts, and declare optional visual reactions. It may not contain executable Rust, JavaScript, shell scripts, native libraries, WebAssembly, arbitrary WGSL, or external process commands.

This still allows extensive customization: colors, fonts, character sets, density, matching, source media, graph composition, seeds, timing, reactions, transitions, and quality settings. 'Fully customizable' does not mean 'run arbitrary code from any wallpaper download.'

## Why shaders are not automatically safe

A syntactically valid shader can consume excessive GPU time, allocate expensive resources through its host, trigger driver bugs, or hang a display path. Validation alone is not a dependable execution-time budget. Do not permit unreviewed shaders in the same process/device path that keeps the user's desktop responsive.

Built-in WGSL remains part of the reviewed engine release. It is versioned, tested, bounded by host resource limits, and not replaceable by a package. Custom color ramps or effect parameters are data, not source-code substitution.

## Future extension research

A future extension system could use a restricted WebAssembly ABI with no default WASI file/network/process access, memory limits, metering, deterministic imports, and a versioned frame/event interface. It still needs a threat model, timeout strategy, parser review, and native boundary tests before release.

Do not claim WebAssembly makes arbitrary plugins safe by itself. Native GPU access and external imports can erase isolation. Signed plugins authenticate an author/key, not good behavior.

## Developer mode

An advanced developer build may load local experimental code only through a clearly separate explicit mode, with prominent warnings and no association with `.asciiwall` auto-import. It should not be enabled by a package, catalog, config migration, or hidden URL.

Community-library publication remains data-only until the extension security model has an independent release decision. Keeping this boundary small is the recommended way to launch a dependable open-source wallpaper engine.

## Native shell integrations

Plasma/GNOME companion components are trusted application adapters, not wallpaper plugins in the package sense. Users install/update them through a reviewed distribution path. Their code must be minimal because a fault can affect the desktop shell. They never execute user-provided conversion or media parser code in the shell process.
