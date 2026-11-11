# Static catalog schema and trust model

## Backend-free distribution

A catalog is a static JSON file plus a detached signature, distributed with release assets or static hosting. The app does not require a hosted API, user account, database server, analytics endpoint, or search backend. Search/filter runs locally after an explicit catalog download.

Online catalog access is optional and off by default. Local packages and cached metadata continue working with the network blocked. Hosting a public static file still uses external infrastructure; 'no backend' does not mean downloads occur without a network host.

## Catalog schema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:asciiwall:schema:catalog:1.0",
  "title": "AsciiWall draft static catalog 1.0",
  "type": "object",
  "properties": {
    "catalog_version": {
      "const": "1.0"
    },
    "catalog_id": {
      "type": "string",
      "minLength": 1,
      "maxLength": 120
    },
    "sequence": {
      "type": "integer",
      "minimum": 1
    },
    "published_at": {
      "type": "string",
      "format": "date-time"
    },
    "expires_at": {
      "type": "string",
      "format": "date-time"
    },
    "entries": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string",
            "minLength": 1,
            "maxLength": 120
          },
          "version": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80
          },
          "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 120
          },
          "summary": {
            "type": "string",
            "minLength": 1,
            "maxLength": 300
          },
          "source_kind": {
            "enum": [
              "scene",
              "image",
              "video",
              "ascii-clip",
              "rendered-video"
            ]
          },
          "license": {
            "type": "string",
            "minLength": 1,
            "maxLength": 120
          },
          "authors": {
            "type": "array",
            "items": {
              "type": "string",
              "minLength": 1,
              "maxLength": 120
            },
            "maxItems": 16,
            "minItems": 1
          },
          "tags": {
            "type": "array",
            "items": {
              "type": "string",
              "minLength": 1,
              "maxLength": 40
            },
            "maxItems": 16,
            "minItems": 0
          },
          "engine_min_version": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80
          },
          "features_required": {
            "type": "array",
            "items": {
              "type": "string",
              "minLength": 1,
              "maxLength": 80
            },
            "maxItems": 32,
            "minItems": 0
          },
          "capabilities_required": {
            "type": "array",
            "items": {
              "type": "string",
              "minLength": 1,
              "maxLength": 80
            },
            "maxItems": 16,
            "minItems": 0
          },
          "capabilities_optional": {
            "type": "array",
            "items": {
              "type": "string",
              "minLength": 1,
              "maxLength": 80
            },
            "maxItems": 16,
            "minItems": 0
          },
          "package": {
            "type": "object",
            "properties": {
              "url": {
                "type": "string",
                "pattern": "^https://",
                "maxLength": 2048
              },
              "sha256": {
                "type": "string",
                "pattern": "^[a-f0-9]{64}$"
              },
              "size": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1073741824
              }
            },
            "required": [
              "url",
              "sha256",
              "size"
            ],
            "additionalProperties": false
          },
          "preview": {
            "type": "object",
            "properties": {
              "url": {
                "type": "string",
                "pattern": "^https://",
                "maxLength": 2048
              },
              "sha256": {
                "type": "string",
                "pattern": "^[a-f0-9]{64}$"
              },
              "size": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1073741824
              }
            },
            "required": [
              "url",
              "sha256",
              "size"
            ],
            "additionalProperties": false
          }
        },
        "required": [
          "id",
          "version",
          "name",
          "summary",
          "source_kind",
          "license",
          "authors",
          "tags",
          "engine_min_version",
          "features_required",
          "capabilities_required",
          "capabilities_optional",
          "package"
        ],
        "additionalProperties": false
      },
      "maxItems": 10000,
      "minItems": 0
    }
  },
  "required": [
    "catalog_version",
    "catalog_id",
    "sequence",
    "published_at",
    "expires_at",
    "entries"
  ],
  "additionalProperties": false
}
```

## Illustrative catalog

The URL below deliberately uses `example.invalid`; the hash and size are illustrative placeholders for an unpublished archive. It is schema-valid but is not a real download or a verified release receipt. Production catalogs must derive every package/preview hash and size from actual final artifacts.

```json
{
  "catalog_version": "1.0",
  "catalog_id": "org.asciiwall.community",
  "sequence": 1,
  "published_at": "2026-11-11T00:00:00Z",
  "expires_at": "2026-12-11T00:00:00Z",
  "entries": [
    {
      "id": "org.asciiwall.examples.matrix-rain",
      "version": "1.0.0",
      "name": "Matrix Rain - Documentation Fixture",
      "summary": "A proposed built-in rain scene with optional passive click ripples; not a published wallpaper.",
      "source_kind": "scene",
      "license": "CC0-1.0",
      "authors": [
        "AsciiWall documentation example"
      ],
      "tags": [
        "procedural",
        "rain",
        "monochrome"
      ],
      "engine_min_version": "0.1.0",
      "features_required": [
        "scene-v1"
      ],
      "capabilities_required": [],
      "capabilities_optional": [
        "pointer-buttons"
      ],
      "package": {
        "url": "https://example.invalid/asciiwall/matrix-rain-1.0.0.asciiwall",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "size": 4096
      }
    }
  ]
}
```

## Semantic checks

Bound the catalog file to 16 MiB before parsing. Validate timestamps as real UTC instants with expiration after publication; check unique ID/version pairs; parse SemVer; validate tag/ID syntax using package rules; validate package source-kind/feature consistency. Remote metadata is not proof the actual package matches; revalidate its manifest after download.

Restrict downloads to administrator/user-approved HTTPS origins. Validate every redirect, forbid URL userinfo, local/private-network targets for the built-in remote catalog, and enforce size/time budgets. A catalog cannot add arbitrary endpoints, executable update channels, or local paths. Do not fetch previews automatically before network consent.

## Signature profile

Proposed v1 detached envelope: fixed scheme `ed25519`, a trusted `key_id`, and a Base64-encoded 64-byte signature. Sign the exact UTF-8 catalog bytes prefixed by the ASCII domain separator `asciiwall-catalog-v1` followed by one zero byte. Do not reserialize JSON before verification. Parse an envelope with strict byte/field limits and verify using a reviewed implementation with strict validation. [S60](../research/source-register.md#s60)

Trusted public keys arrive with a signed application release or an explicit user trust action, never from an unsigned catalog that vouches for itself. Store the highest accepted sequence for each trusted catalog to detect ordinary rollback. Define key rotation/revocation before public launch; it requires authenticated trust-root updates, not just a new key name in the same catalog.

This simple signed catalog is not a full TUF implementation and does not solve every compromise/freeze attack. [S44](../research/source-register.md#s44) An offline client cannot prove its cached catalog is current. Expired metadata may remain browsable with a warning; new remote installation is blocked until a fresh trusted catalog or explicit advanced recovery action. Installed local artwork remains usable.
