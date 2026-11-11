# Security Policy

## Current Status and Supported Versions

AsciiWall is currently in a **pre-implementation design, planning, and repository-infrastructure baseline** (`main`). **No executable application binaries, desktop hosts, media workers, package importers, or release artifacts have been shipped yet.**

| Target / Release | Supported | Notes |
| --- | --- | --- |
| Released binaries (`0.x` / `1.x`) | None yet | No compiled release exists; security controls are specified in `docs/security/` and tracked as backlog prerequisites. |
| `main` (docs, schemas, repo scripts) | Active baseline | Design specifications, draft schemas, and Python/PowerShell repository tooling only. |

Accordingly, runtime media-worker confinement, archive extraction sandboxes, fuzz campaigns, and binary signing are **planned release gates** ([docs/quality-gates.md](docs/quality-gates.md)), not yet tested production defenses.

## Reporting a Vulnerability (Private Route Only)

**Do not open a public GitHub issue, pull request, or discussion** for an undisclosed vulnerability, crash-inducing media/archive file, path-traversal proof of concept, privilege-escalation flaw in CI/workflows, or supply-chain compromise.

Report vulnerabilities privately through:

1. **GitHub Private Vulnerability Reporting (preferred):** submit a draft security advisory at [Astraive/asciiwall Security Advisories](https://github.com/Astraive/asciiwall/security/advisories/new).
2. **Maintainer private fallback:** if GitHub Private Vulnerability Reporting is unavailable or disabled, contact repository maintainer `@NeerajCodz` (`https://github.com/NeerajCodz`) privately before posting any details publicly.

### What to include

- Affected repository (`Astraive/asciiwall` or `Astraive/asciiwall-library`), commit SHA (or future release version/target triple), and component
- Exact reproduction steps using a **minimized, rights-cleared, synthetic fixture**
- Expected vs. observed security boundary behavior and potential impact
- Environment tuple (OS build, session/compositor, GPU/driver, or Python/PowerShell tool versions) when applicable

**Privacy guardrail:** Never attach personal videos/images, proprietary fonts, tokens, or unredacted local filesystem paths to a report.

## Response, Triage, and Remediation Policy

- Repository maintainer `@NeerajCodz` reviews incoming reports, determines affected scope against [docs/security/threat-model.md](docs/security/threat-model.md), and coordinates remediation.
- P0 destructive-data, archive-escape, unauthorized-observation, desktop-lockout, and security failures are **non-waivable** ([docs/quality-gates.md](docs/quality-gates.md)) and cannot be shipped under a "beta" label.
- Security updates and advisories publish affected versions and exact artifact/package hashes without collecting user media, input traces, or silent telemetry ([docs/security/supply-chain-and-updates.md](docs/security/supply-chain-and-updates.md)).

## Architectural Security Boundaries

Future AsciiWall releases follow these design invariants:

- **Offline invariant:** Core import, conversion, preview, playback, and export work with all network access denied ([docs/security/privacy-and-network-policy.md](docs/security/privacy-and-network-policy.md)).
- **Data-only v1 packages:** `.asciiwall` v1 packages contain declarative metadata and approved local assets only — never executable code, JavaScript, shell commands, WebAssembly, or arbitrary user shaders ([docs/security/plugin-and-shader-policy.md](docs/security/plugin-and-shader-policy.md)).
- **Untrusted parser isolation:** Media decoding and archive extraction are designed around restricted workers, bounded dimensions/bytes/time, and strict path checks ([docs/security/untrusted-media-and-packages.md](docs/security/untrusted-media-and-packages.md)).
- **CI privilege separation:** Untrusted pull requests never receive release signing secrets and never execute privileged `pull_request_target` checkouts ([docs/delivery/ci-and-release.md](docs/delivery/ci-and-release.md)).
- **Explicit non-goals:** AsciiWall is not a sandbox against an already compromised operating system, malicious GPU driver, local administrator account, or modified application binaries. Offline clients also cannot immediately learn about newly revoked catalog entries without an explicit catalog refresh.
