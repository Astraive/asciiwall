# Library, import, and export user flows

## Local library first

The default library is installed packages and personal imports. Filters include source kind, tags, license, compatibility, motion level, low-power suitability, and installed version. Favorite and usage metadata stay local. No popularity ranking or telemetry service is required.

Online browsing is a separately enabled static-catalog view. It displays the last refresh time, signature/trust state, and expiration. A cached catalog can be browsed offline, but not presented as definitely current.

## Package details

Show name, authors, claimed license, attribution, package ID/version/hash, source mode, engine features, optional capabilities, file size, installation status, and customizable versus baked properties. An unsigned package is 'unverified origin'; a bad hash is an integrity error. Do not collapse those states.

A package requiring unavailable features can be imported for storage or opened as a static preview where safe, but Apply explains why live operation is blocked. Optional pointer/window effects are shown as unavailable without rejecting the entire wallpaper.

## Drag and drop

Recognize `.asciiwall`, supported local images, and supported video sources. Never treat an executable renamed as an image as safe. Multiple files open a batch summary with per-item validation and cancel; one bad item does not hide the others' outcomes.

Dropping media creates a local source entry, not a redistribution-ready package. Dropping an archive imports immutable package content. External URLs are not accepted as live media sources in v1.

## Conflicts and deletion

Same hash deduplicates. Same ID/version with different bytes asks for explicit local-variant handling. Removing an installed package lists active assignments that depend on it. Stop or replace those assignments first, or retain the package until no references remain.

Deleting a library reference must not delete the user's original photograph/video. 'Delete managed copy' is a separately labelled destructive action. Cache cleanup never deletes imported source or export files.

## Export and sharing

Use a review wizard: choose portable source or baked mode; resolve all assets; inspect rights/attribution; choose included originals; review private metadata; validate; save locally. No upload happens after Save. Sharing to the community is a separate contribution workflow.

Batch export uses bounded concurrency, per-item output naming, overwrite protection, and a result summary. A failed item does not produce a success-named partial archive. Cancel preserves completed valid outputs and deletes incomplete temporary files.
