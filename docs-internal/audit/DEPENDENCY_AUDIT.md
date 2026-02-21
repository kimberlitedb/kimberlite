# Dependency Audit Exceptions

This document tracks each advisory that is intentionally accepted in
`deny.toml` or `security.yml`. Each entry records the rationale, risk
assessment, and resolution timeline.

All entries are reviewed quarterly. Entries older than 90 days without
resolution must be escalated to the maintainers.

---

## Active Exceptions

### RUSTSEC-2025-0141

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2025-0141 |
| **Crate** | (see advisory) |
| **Severity** | TBD |
| **Accepted** | Pre-v0.4.0 |
| **Approved by** | Maintainers |

**Rationale:** Pre-existing ignore from before v0.4.0. Transitive dependency
with no direct exploitable path through Kimberlite's public surface area.

**Resolution:** Tracked in ROADMAP under dependency updates.

---

### RUSTSEC-2025-0134

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2025-0134 |
| **Crate** | (see advisory) |
| **Severity** | TBD |
| **Accepted** | Pre-v0.4.0 |
| **Approved by** | Maintainers |

**Rationale:** Pre-existing ignore from before v0.4.0. No exploitable path
through Kimberlite.

**Resolution:** Tracked in ROADMAP under dependency updates.

---

### RUSTSEC-2026-0007

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2026-0007 |
| **Crate** | `bytes` < 1.11.1 |
| **Severity** | Medium |
| **Accepted** | 2026-02 |
| **Approved by** | Maintainers |

**Rationale:** `bytes` is pulled in transitively via `tower-http`, `reqwest`,
and `opentelemetry`. The advisory affects a specific API path not used by
Kimberlite. Waiting for upstream crates to update before we can update.

**Resolution:** Update when `tower-http` and `reqwest` release new versions
with updated `bytes` bounds.

---

### RUSTSEC-2026-0009

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2026-0009 |
| **Crate** | `time` < 0.3.47 |
| **Severity** | Medium |
| **Accepted** | 2026-02 |
| **Approved by** | Maintainers |

**Rationale:** `time` is pulled transitively via `jsonwebtoken` and `printpdf`.
The vulnerability affects locale-parsing code; Kimberlite does not use locale-
dependent time parsing. Medium severity with no known exploitable path.

**Resolution:** Update `jsonwebtoken` and `printpdf` when they release
compatible `time` versions.

---

### RUSTSEC-2023-0089

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2023-0089 |
| **Crate** | `atomic-polyfill` (unmaintained) |
| **Severity** | Informational (unmaintained) |
| **Accepted** | 2026-02 |
| **Approved by** | Maintainers |

**Rationale:** `atomic-polyfill` is flagged as unmaintained. It is a
transitive dependency via `postcard` and `heapless`, which Kimberlite uses for
internal serialization. No security vulnerability is reported — only
maintenance status. The crate is stable and the API is frozen by design.

**Resolution:** Wait for `postcard`/`heapless` to migrate away from this
dependency.

---

### RUSTSEC-2024-0436

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2024-0436 |
| **Crate** | `paste` (unmaintained) |
| **Severity** | Informational (unmaintained) |
| **Accepted** | 2026-02 |
| **Approved by** | Maintainers |

**Rationale:** `paste` is flagged as unmaintained. It is a transitive
dependency via `ratatui` (used in the VOPR TUI). No security vulnerability
reported. The macro functionality is stable.

**Resolution:** Wait for `ratatui` to migrate away from `paste`.

---

### RUSTSEC-2026-0002

| Field | Value |
|-------|-------|
| **Advisory** | RUSTSEC-2026-0002 |
| **Crate** | `lru` 0.12.5 |
| **Severity** | Medium (unsound IterMut) |
| **Accepted** | 2026-02 |
| **Approved by** | Maintainers |

**Rationale:** `lru` 0.12.5 has unsound `IterMut` behavior. Kimberlite does
not use `IterMut` on any `lru` cache directly. The affected API is only
reachable through a transitive dependency. Upgrading requires waiting for the
upstream crate to update.

**Resolution:** Update `lru` directly or wait for dependent crate to update.
Target: v0.5.0.

---

## Resolved Exceptions

*No resolved exceptions yet. Entries are moved here when the advisory is
resolved (dependency updated or advisory retracted).*

---

## Exception Process

To add a new exception:

1. Open a PR with the advisory details filled in above.
2. Include the advisory URL, affected crate, and severity.
3. Describe why the vulnerability is not exploitable through Kimberlite.
4. Get approval from at least one maintainer.
5. Add the advisory ID to `security.yml` cargo audit ignore list.
6. Set a resolution target in ROADMAP.md.

Exceptions are audited quarterly. Any exception older than 6 months without
a resolution plan must be re-evaluated.
