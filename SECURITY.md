# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.4.x   | ✅ Active development |
| < 0.4   | ❌ Not supported   |

Kimberlite is currently in **Developer Preview** (v0.4.x). Security fixes are
applied to the current release series only.

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

### Option 1: GitHub Private Security Advisory (Preferred)

Use [GitHub's private vulnerability reporting](https://github.com/kimberlitedb/kimberlite/security/advisories/new)
to submit a report. This is the fastest path to a coordinated fix.

### Option 2: Email

Send a report to **security@kimberlite.dev** with:
- A description of the vulnerability
- Steps to reproduce
- Potential impact assessment
- Any suggested mitigations you've identified

Encrypt sensitive reports with our PGP key (available at
`https://kimberlite.dev/.well-known/security.txt`).

## Response Timeline

| Stage | Timeline |
|-------|----------|
| Initial acknowledgement | Within 48 hours |
| Severity assessment | Within 5 business days |
| Fix development | Within 90 days (critical: within 30 days) |
| Coordinated disclosure | After fix is available |

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure).
We will credit reporters in release notes unless they prefer to remain anonymous.

## Scope

### In Scope

The following components are in scope for security reports:

- **Database engine** — kernel, storage, log integrity (crates/kimberlite-kernel, kimberlite-storage)
- **Cryptographic primitives** — hash chains, encryption, signatures (crates/kimberlite-crypto)
- **SQL parser** — injection, parser bugs with security implications (kimberlite-sql)
- **FFI layer** — memory safety, ABI correctness (crates/kimberlite-ffi)
- **CLI** — command injection, privilege escalation (crates/kimberlite-cli)
- **SDKs** — Python, TypeScript client libraries (sdks/)
- **Multi-tenant isolation** — cross-tenant data access
- **RBAC/ABAC** — authorization bypass vulnerabilities

### Out of Scope

- Documentation-only issues (typos, factual inaccuracies)
- Vulnerabilities in transitive dependencies with no exploitable path through Kimberlite
- Best-practice suggestions without a concrete exploit
- Issues requiring physical access to the machine
- Social engineering

## Bug Bounty

Kimberlite does not currently operate a formal bug bounty program. We plan to
open a bug bounty program at the v1.0 release. Reports submitted now will be
tracked and eligible reporters will be recognized in the hall of fame when the
program opens.

## Security Architecture

Kimberlite's security model is designed around:

- **Immutable audit trail** — All data is an append-only, hash-chained log. Tampering is detectable.
- **Per-tenant encryption** — AES-256-GCM encryption with per-tenant key hierarchy.
- **Formal verification** — 136+ mathematical proofs on protocol correctness (TLA+, Alloy, Ivy).
- **Production assertions** — 38 critical invariants checked at runtime for cryptography and consensus.
- **VOPR simulation** — Deterministic simulation testing with Byzantine fault injection.

See [docs/concepts/architecture.md](docs/concepts/architecture.md) for full details.

## Dependency Audit Exceptions

Some advisories in transitive dependencies are intentionally accepted. Each
exception is documented with rationale in
[docs-internal/audit/DEPENDENCY_AUDIT.md](docs-internal/audit/DEPENDENCY_AUDIT.md).
