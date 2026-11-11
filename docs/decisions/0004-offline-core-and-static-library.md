# ADR 0004 - Offline core and static community library

## Status

Proposed design decision.

## Options

A hosted account/service backend, a Git-only giant media repository, or an offline local app plus a separately published static catalog and release artifacts. The hosted backend conflicts with the user's constraints; storing every large video in normal Git creates distribution/maintenance problems. [S41](../research/source-register.md#s41)

## Decision

Core import/conversion/playback/edit/export/local search works without networking. `asciiwall-library` stores recipes/metadata/rights records and small previews. Versioned packages are distributed through release assets or explicit static hosting. [S42](../research/source-register.md#s42) Online catalog/download features are optional, manual, and separable from the engine.

No accounts, telemetry, remote runtime code, database server, or paid service is required. A local embedded index is permitted because it is not a server. Search runs locally over metadata.

## Consequences

Simple hosting and offline use, but no server-side popularity, automatic moderation service, or guaranteed immediate revocation. Signed metadata needs protected keys and rollback/expiration policy; offline clients cannot prove freshness. A simple signature design is not automatically TUF-complete. [S44](../research/source-register.md#s44)

## Revisit when

A clearly justified optional community service adds value without becoming a prerequisite for local use. Such a change requires an explicit privacy/product decision, not an unnoticed dependency introduced by a convenient library.
