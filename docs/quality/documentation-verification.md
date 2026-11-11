# Documentation verification report

**Verification date:** 2026-11-11. **Scope:** the Markdown research/design delivery and its embedded reference contracts, not an implemented AsciiWall application.

## Checks actually performed

| Check | Result |
|---|---|
| Documentation inventory | 100 Markdown files |
| Filename and directory policy | All delivered names are lowercase kebab-case |
| Delivery contents | Markdown only; no font binaries, downloaded art, application executables, or build scripts |
| Markdown structure | Titles present and fenced code blocks balanced |
| Internal links and heading fragments | All referenced local targets and fragments resolve |
| Source-reference labels | All labels resolve to the 62-entry source register |
| Embedded JSON parsing | All nine JSON blocks parse as strict JSON |
| Schema validation | Manifest, scene, and catalog schemas pass Draft 2020-12 schema checks |
| Positive fixtures | Manifest, scene, and illustrative catalog pass their schemas, with format checking |
| Asset fixture integrity | Exact example scene/license byte sizes and SHA-256 values match the manifest |
| Negative schema fixtures | 12 expected rejections confirmed, including nonportable permissions, unknown source/node, bad ranges, and invalid catalog URL/sequence |
| Reference graph checks | Valid example accepted; five malformed graph mutations rejected |
| Setting registry | 172 unique keys; every declared default fits its type/range; 80 portable v1 visual settings |
| Work-unit graph | 40 work units with an acyclic dependency graph |
| Edge-case inventory | 120 unique planned scenario IDs |
| Clip-layout arithmetic | Header 64 bytes, index entry 40 bytes, cell 12 bytes; a 2x2 reference fixture has consistent offsets and CRC |
| Cross-document review | v1 scene schema distinguished from later typed-graph extensions; IPC envelope names reconciled |
| Final archive | ZIP CRC check and byte-for-byte comparison against every source Markdown file |

Schema/graph/layout checks use independent document-preparation reference code. They are not Rust implementation tests and do not validate an unimplemented native extractor or decoder.

## Content status

The [settings registry](../settings/settings-reference.md), [schemas](../package/manifest-schema.md), [task backlog](../delivery/implementation-backlog.md), and [failure matrix](edge-case-matrix.md) are proposed contracts/requirements. The library includes 36 original recipe briefs, not 36 generated or published wallpapers. The catalog example uses an intentionally nonresolving example URL and illustrative archive metadata; it is not a real release.

The `.asciiwall` and `.ascii-clip` formats remain drafts. Performance numbers are proposed budgets or arithmetic examples. Dependency versions are researched snapshots, not a compiled lockfile.

## Not performed

No Rust application build, native Windows/macOS/Linux wallpaper run, GPU/power benchmark, real codec sandbox test, fuzz campaign, screen-reader certification, installer signing/notarization, remote repository creation, actual wallpaper publication, or production signature/key-management test was performed for this documentation delivery.

The native-platform, security, accessibility, and release tests in this pack must be executed during implementation before the product can claim those capabilities. External documentation can change after the research date; internal link verification does not guarantee permanent availability of every external source URL.

## Reading the evidence correctly

A schema-valid package example can still fail semantic, archive, rights, platform, or resource checks in a real application. A reference byte-layout calculation is not proof that an unimplemented decoder is memory-safe. The documentation checks establish a consistent, navigable specification baseline; the implementation plans and acceptance gates define the next evidence required.
