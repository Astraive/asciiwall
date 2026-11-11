# Code, media, fonts, and attribution

## Separate rights layers

The proposed engine code license is MIT OR Apache-2.0, subject to dependency review. Wallpaper artwork, source photographs/videos, fonts, audio, and thumbnails have their own licenses. The library repository's metadata license does not automatically relicense every asset inside it.

This is an engineering compliance checklist, not legal advice or a final distribution opinion. Before release, review the exact bundled binaries, source obligations, notices, and jurisdiction-specific codec/patent considerations with qualified advice where needed.

## Asset ledger

For every asset record origin URL or original-author declaration, author/rights holder, license identifier and text, permission evidence where applicable, changes made, redistribution constraints, and attribution display text. Keep private proof separate when it contains personal information, but publish enough provenance for users to understand their obligations.

CC BY attribution and indication of modifications matter; CC BY-SA can impose share-alike terms on adaptations. [S54](../research/source-register.md#s54) [S55](../research/source-register.md#s55) A photograph rendered into ASCII is not automatically free of its source rights. Reject 'found on Google' as a license.

## Fonts

A font must allow the intended redistribution. Retain required license files and observe reserved-name conditions when modifying OFL fonts. [S53](../research/source-register.md#s53) Users can select local fonts for personal wallpaper without automatically receiving permission to bundle them in exported packages.

The export UI distinguishes a local font reference from a redistributable embedded font. If redistribution is uncertain, use the built-in approved font or ask the user to provide an authorized alternative. This documentation ZIP contains no font binaries.

## FFmpeg and native dependencies

FFmpeg licensing depends on its configure options and enabled components. [S33](../research/source-register.md#s33) Record configure flags, source version, license notices, and how users obtain corresponding source where required. Do not assume a permissively licensed Rust wrapper makes every linked codec library permissive.

Static and dynamic linking, optional GPL components, platform codec use, and installer redistribution need review for the actual build. A license scanner alone cannot decide every combined-work obligation.

## Publishing policy

Prefer original procedural scenes, CC0 assets, and clearly documented CC BY material for a low-friction starter collection. Support other compatible licenses only with explicit policy and complete attribution. Avoid noncommercial/no-derivatives restrictions in the default curated collection unless the project's distribution policy intentionally supports them.

Catalog search can filter licenses but is not a legal approval service. Preserve attribution in package export, the details panel, and any accompanying export metadata where practical. Plain-text or PNG exports may need a separate attribution file; do not assume a visual watermark is required or sufficient for every license.

## Takedowns and provenance disputes

Maintain a contact/report path and hash-based audit trail. Remove disputed entries from future catalogs while investigating, retain internal evidence, and inform affected users where possible. Offline copies cannot be retroactively guaranteed deleted, and the app should not pretend otherwise.
