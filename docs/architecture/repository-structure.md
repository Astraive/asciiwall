# Application repository structure

The target layout uses a Cargo workspace. Create crates when their boundaries are exercised, not dozens of empty crates on day one. `apps` are executable compositions; reusable engine code lives in `crates`; shell adapters remain explicit. Cargo workspace dependencies and one committed lockfile keep application builds aligned. [S34](../research/source-register.md#s34)

```text
asciiwall/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  readme.md
  contributing.md
  security.md
  license-mit
  license-apache
  apps/
    desktop/                 # controller, egui UI, tray, local commands
      Cargo.toml
      src/{main.rs,app.rs,supervisor.rs,commands.rs}
    host/                    # native surfaces, render loop, simulation
      Cargo.toml
      src/{main.rs,host.rs,session.rs}
    media-worker/            # bounded untrusted-media decoding jobs
      Cargo.toml
      src/{main.rs,decode.rs,limits.rs}
    cli/                     # batch conversion, packages, local control
      Cargo.toml
      src/{main.rs,convert.rs,package.rs,control.rs}
  crates/
    ascii-core/              # frames, grids, colors, ids, clocks
    ascii-convert/           # our Rust image-to-glyph algorithms
    ascii-media/             # decoder abstraction, image + FFmpeg adapters
    ascii-render/            # wgpu pipelines, atlas, offscreen rendering
      shaders/               # first-party WGSL only in stable packages
    ascii-scene/             # generators, effects, graph, transitions
    ascii-runtime/           # scheduling, policy, resource budgets
    ascii-platform/          # native host/event capability interfaces
      src/{windows,macos,x11,wayland}/
    ascii-package/           # archive validation, schema, import/export
    ascii-library/           # local catalog, optional remote index client
    ascii-config/            # typed settings, overrides, migrations
    ascii-ipc/               # versioned local envelopes and frame handles
    ascii-ui/                # reusable egui editor and preview panels
  integrations/
    plasma/                  # QML plugin + minimal reviewed bridge
    gnome/                   # GJS extension; no conversion in Shell
    compositor-bridges/      # individually versioned optional adapters
  schemas/                   # extracted machine-readable schema files
  assets/
    icons/
    bundled-font-manifests/  # provenance/license metadata, not arbitrary fonts
    first-run-posters/
  presets/
  tests/
    fixtures/{images,video,packages}/
    golden/{grids,renders}/
    contracts/
    platform/
  fuzz/
    fuzz_targets/
  benches/
  xtask/                     # Rust build/package/catalog automation
  docs/                      # organized documentation from this pack
  packaging/{windows,macos,linux}/
  scripts/                   # thin platform-specific signing wrappers
  .github/workflows/
```

Rust-required names such as `Cargo.toml` are exceptions to documentation naming; every Markdown document in the delivered pack is kebab-case. Source module names follow Rust conventions, not Markdown naming rules.

## Dependency direction

`ascii-core` knows nothing about files, UI, FFmpeg, native windows, or network access. Conversion and scene generation depend on core. Rendering consumes core/scene presentation data. Platform adapters expose typed capabilities and surface lifetimes; they do not own artistic algorithms. Executables wire components together.

`ascii-library` must not become a second engine. Catalog previews and build checks invoke the same engine CLI. Tests can use fake platform and fake decoder adapters. Do not use `cfg` branches throughout the conversion loop; contain platform-specific compilation at adapter boundaries.

## Separation from the artwork repository

Do not place a growing community video collection under `assets/` in the engine repository. Keep a small, properly licensed test corpus and bundled first-run scenes. The second repository owns catalog review and artwork release workflows, described separately.
