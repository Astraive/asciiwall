# The asciiwall-library repository

## Role and ownership

`asciiwall` contains the application, engine, format validators, schemas, and release tooling. `asciiwall-library` contains community wallpaper recipes, metadata, small previews, attribution, and the static catalog build. It does not fork conversion algorithms or ship an independent runtime.

The repository name is a proposal; this documentation task does not create a remote repository. Contributors can make local packages without submitting to the community collection.

```text
asciiwall-library/
  readme.md
  contributing.md
  code-of-conduct.md
  security.md
  licenses/
    repository-metadata-license.txt
  wallpapers/
    matrix-rain/
      manifest-source.json
      scene.json
      preview.png
      attribution.md
      assets.lock.json
      licenses/
    quiet-waves/
      manifest-source.json
      scene.json
      attribution.md
      assets.lock.json
  collections/
    minimal.json
    reactive.json
    low-power.json
  schemas/
    engine-schema-lock.json
  catalog/
    catalog.json               # generated on trusted release
    catalog.sig                # detached signature envelope
  tests/
    catalog-fixtures/
    license-fixtures/
  scripts/
    build-library.py
    check-attribution.py
  docs/
    authoring-guide.md
    review-policy.md
    license-policy.md
  .github/
    workflows/
      validate.yml
      release.yml
    issue-template/
    pull-request-template.md
```

All wallpaper directory slugs and documentation filenames are kebab-case. Source descriptors may omit computed inventory hashes only in the explicitly named build-input format; published `manifest.json` never omits them. The trusted packer resolves inputs and produces the strict package manifest.

## Asset strategy

Store original small procedural scenes and metadata in Git. Keep large media and built packages in versioned release artifacts or another explicit static artifact store. Ordinary Git history is a poor distribution path for a growing binary-video gallery. [S41](../research/source-register.md#s41) [S42](../research/source-register.md#s42)

Every fetched build asset has a pinned digest, exact origin, license proof, and size limit. No mutable 'latest video' URLs. Pull requests must not introduce arbitrary scripts, build commands, remote plugins, or dependency-install steps as wallpaper data.

## Engine compatibility

Pin the exact engine packer release/hash and schema digest used by the collection build. Updating that pin is a reviewed maintenance change. Library CI validates against that engine, not an unreviewed branch supplied by a contribution.

Use engine CLI `package pack`, `package validate`, and `bake` for previews where implemented. The commands in this blueprint are proposed interfaces, not installed tools today.

## Community workflow

A creator authors a recipe, adds rights/attribution, builds and validates locally, submits a PR, and receives automated plus human review. Acceptance means suitable for the curated collection, not proof of legal ownership or security perfection. Packages can be removed from future catalog versions without remotely deleting users' local files.
