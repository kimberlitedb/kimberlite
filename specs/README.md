# Kimberlite Formal Specifications

This directory contains formal specifications and verification artifacts for Kimberlite's consensus protocol, cryptography, and compliance properties.

## Overview

Kimberlite uses a **six-layer defense-in-depth verification approach** where every critical property is verified by at least two independent tools using different techniques:

```
┌──────────────────────────────────────────────────────────────────┐
│                    Kimberlite Verification Stack                  │
├──────────────────────────────────────────────────────────────────┤
│  Layer 1: Protocol Specifications (this directory)               │
│  ├─ TLA+ with TLAPS mechanized proofs (unbounded)               │
│  ├─ Ivy for Byzantine consensus modeling                        │
│  └─ Alloy for structural invariants (hash chains, quorum)       │
│                                                                   │
│  Layer 2-6: Code verification, types, compliance, runtime        │
│  (see docs/concepts/formal-verification.md for full overview)    │
└──────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
specs/
├── tla/              # TLA+ specifications (19 files + cfgs)
│   ├── VSR.tla                # Core Viewstamped Replication protocol
│   ├── VSR_Proofs.tla         # TLAPS mechanized proofs
│   ├── VSR.cfg / VSR_Small.cfg
│   ├── ViewChange.tla         # View change protocol
│   ├── ViewChange_Proofs.tla
│   ├── Recovery.tla           # Protocol-aware recovery
│   ├── Recovery_Proofs.tla
│   ├── Compliance.tla         # Compliance meta-framework
│   ├── Compliance_Proofs.tla
│   ├── ClockSync.tla, Reconfiguration.tla, ClientSessions.tla, Scrubbing.tla, RepairBudget.tla
│   └── compliance/            # 23 regulatory framework specs (HIPAA, GDPR, SOC2, ...)
├── ivy/              # Ivy Byzantine consensus model
│   └── VSR_Byzantine.ivy      # Byzantine fault model (f < n/3), 5 safety invariants
├── alloy/            # Alloy structural models
│   ├── Simple.als             # Setup smoke test
│   ├── HashChain.als          # Hash chain integrity (scope 10)
│   ├── HashChain-quick.als    # Same model at scope 5 for CI speed
│   └── Quorum.als             # Quorum intersection (scope 8)
├── coq/              # Coq cryptographic proofs
│   ├── Common.v               # Shared lemmas
│   ├── SHA256.v, BLAKE3.v, AES_GCM.v, Ed25519.v, KeyHierarchy.v
│   ├── MessageSerialization.v
│   └── Extract.v              # Code extraction to verified Rust wrappers
├── setup.md          # Tool installation guide
└── quickstart.md     # Quick-start verification commands
```

## Getting Started

### 1. Install Tools

See [setup.md](./setup.md) for complete installation instructions.

**Quick install on macOS:**
```bash
# TLA+ tools (includes TLC model checker)
brew install --cask tla-plus-toolbox

# TLAPS (proof system for unbounded verification)
brew install tlaplus/tlaplus/tlaps

# Ivy (Byzantine consensus verification)
pip3 install ms-ivy

# Alloy (structural modeling)
brew install alloy
```

### 2. Run Verification

```bash
# Quick verification (bounded, ~1 minute)
just verify-tla-quick

# Full TLA+ verification (bounded, ~5 minutes)
just verify-tla

# Mechanized proofs (unbounded, ~15 minutes)
just verify-tlaps

# All verification layers
just verify-all
```

## What We Verify

### Safety Properties (Always True)

| Property | Description | Verified By |
|----------|-------------|-------------|
| **Agreement** | Replicas never commit conflicting operations at the same offset | TLA+ TLAPS, Ivy, VOPR |
| **PrefixConsistency** | Committed log prefixes are identical across replicas | TLA+ TLAPS, VOPR |
| **ViewMonotonicity** | View numbers never decrease | TLA+ TLAPS, Flux, VOPR |
| **ViewChangePreservesCommits** | View changes preserve all committed operations | TLA+ TLAPS, Ivy |
| **LeaderUniqueness** | Exactly one leader per view | TLA+ TLAPS, Ivy |
| **RecoveryPreservesCommits** | Recovery never loses committed operations | TLA+ TLAPS |
| **TenantIsolation** | Tenants cannot access each other's data | TLA+ Compliance, Flux, Kani |
| **AuditCompleteness** | All operations appear in immutable audit log | TLA+ Compliance |
| **HashChainIntegrity** | Hash chain has no cycles or breaks | Alloy, Kani, VOPR |

### Liveness Properties (Eventually True)

| Property | Description | Verified By |
|----------|-------------|-------------|
| **EventualCommit** | Proposed operations eventually commit (with quorum) | TLA+ (fairness) |
| **EventualProgress** | System makes progress with live quorum | TLA+ (fairness) |

## Specifications

### VSR.tla - Core Consensus Protocol

The primary specification modeling Viewstamped Replication as implemented in `crates/kimberlite-vsr/`.

**Key theorems (proven with TLAPS):**
- `Agreement` - No conflicting commits at same offset
- `PrefixConsistency` - Log prefixes match across replicas
- `ViewMonotonicity` - Views only increase
- `ViewChangePreservesCommits` - View changes are safe
- `LeaderUniqueness` - One leader per view
- `RecoveryPreservesCommits` - Recovery is safe

**Run verification:**
```bash
# Bounded model checking (explores states up to depth 20)
tlc -workers auto -depth 20 specs/tla/VSR.tla

# Mechanized proofs (unbounded, mathematical certainty)
tlapm --check specs/tla/VSR.tla:Agreement
```

### ViewChange.tla - View Change Protocol

Specification proving that view changes preserve all safety properties from
VSR.tla. Theorem `ViewChangePreservesCommitsTheorem` (proven in
`ViewChange_Proofs.tla`) is the anchor — no committed op is lost during a
view change.

**Status:** Implemented. TLC in PR CI (small config); TLAPS + full config
on EPYC.

### Recovery.tla - Protocol-Aware Recovery

Specification of the recovery protocol with proof that recovery never
discards quorum-committed operations. Theorem
`RecoveryPreservesCommitsTheorem` (in `Recovery_Proofs.tla`).

**Status:** Implemented. TLC in PR CI (Recovery.cfg); TLAPS on EPYC.

### Compliance.tla - Compliance Meta-Framework

Formal specification of compliance properties (tenant isolation, audit
completeness, hash chain integrity) with 23 regulatory frameworks mapped on
top (HIPAA, GDPR, SOC 2, FedRAMP, ...). See `specs/tla/compliance/`.

**Status:** Implemented. TLC in PR CI (depth 8 for state-space budget);
TLAPS on EPYC.

### VSR_Byzantine.ivy - Byzantine Consensus Model

Ivy specification modeling VSR with Byzantine faults (up to f replicas
where f < n/3). Proves 5 safety invariants despite equivocation, fake
messages, and withholding. Signature / replay detection tracking added in
Phase 5 (see traceability matrix rows 9–10).

**Status:** Implemented. Upstream `kenmcmil/ivy v0.1-msv` has Python 2/3
incompatibility so CI is aspirational (nightly, non-blocking).

### HashChain.als, Quorum.als - Structural Models

Alloy specifications proving structural properties:
- `HashChain.als` — hash chain has no cycles (scope 10 full / scope 5 CI).
- `Quorum.als` — any two quorums of size f+1 intersect (scope 8 full).

**Status:** Implemented. Alloy runs in PR CI and on EPYC.

## Understanding TLA+ Output

### TLC Model Checker (Bounded)

```bash
$ tlc -workers auto -depth 20 specs/tla/VSR.tla

TLC2 Version 2.18
...
Model checking completed. No errors found.
6405234 states generated, 2873912 distinct states found.
```

- **States generated**: How many states TLC explored
- **Distinct states**: Unique states (after deduplication)
- **No errors**: All invariants held for all explored states
- **Depth 20**: Explored all execution traces up to 20 steps

### TLAPS Proof System (Unbounded)

```bash
$ tlapm --check specs/tla/VSR.tla:Agreement

Checking theorem Agreement... OK
  Obligation 1... proved by SMT
  Obligation 2... proved by SMT
  Obligation 3... proved by induction
```

- **Unbounded**: Proves property for ALL possible executions (not just depth 20)
- **Mathematical proof**: Uses SMT solvers and induction
- **Higher confidence**: Goes beyond testing to mathematical certainty

## Redundant Verification

Every critical property is verified by **at least 2 independent tools**:

```rust
// Example: Agreement property
✓ TLA+ TLAPS proof (unbounded, SMT-based)
✓ Ivy model (Byzantine-aware, quorum reasoning)
✓ VOPR simulation (runtime, 27 scenarios)

// Example: Tenant isolation
✓ TLA+ Compliance spec (formal model)
✓ Flux refinement types (compile-time enforcement)
✓ Kani SMT proof (code verification)
```

If one tool misses a bug, another catches it.

## Trace Alignment

We validate that our TLA+ specs match the actual Rust implementation by comparing traces:

```bash
# Run test that validates VOPR traces match TLA+ traces
cargo test --test tla_trace_alignment
```

This ensures the specification accurately models the code (or vice versa).

## CI Integration

Two workflows run the stack:

```yaml
# .github/workflows/formal-verification.yml  (PR-blocking)
- TLA+ model checking (TLC, small cfg, depth 10): ~5 min
- Alloy structural models (HashChain-quick scope 5 + Quorum scope 6): ~2 min
- Coq cryptographic proofs (6 files + 2 optional): ~10 min
- Kani bounded model checking (unwind 32, workspace): ~30 min
- MIRI UB detection (storage, crypto, types --lib): ~20 min
Total: ~60 min (parallelized)

# .github/workflows/formal-verification-aspirational.yml  (nightly)
- TLAPS mechanized proofs: ~15 min (continue-on-error)
- Ivy Byzantine model: ~5 min (continue-on-error, Python 2/3 issue)

# Hetzner EPYC runner (on-demand / nightly)
- `just fv-epyc-all` runs the full-capacity versions of every layer
  (VSR.cfg depth 20, HashChain.als scope 10, Kani unwind 128, TLAPS
  stretch 10000, VOPR 100k iterations) in ~3–4 hours.
  See docs-internal/design-docs/active/fv-epyc-deployment.md.
```

Supply-chain hashes (both pinned in CI and in
`tools/formal-verification/epyc/bootstrap.sh`):
- `tla2tools.jar` v1.8.0: `4c1d62e0f67c1d89f833619d7edad9d161e74a54b153f4f81dcef6043ea0d618`
- `alloy-6.2.0.jar`: `6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d`

## Traceability

Each safety/liveness property is mapped to its spec theorem and Rust
enforcement site in
[docs/internals/formal-verification/traceability-matrix.md](../docs/internals/formal-verification/traceability-matrix.md).
When you change a property spec or its Rust implementation, update the
matrix.

## Learn More

- **TLA+**: https://learntla.com/
- **TLAPS**: https://tla.msr-inria.inria.fr/tlaps/
- **Ivy**: https://kenmcmil.github.io/ivy/
- **Alloy**: https://alloytools.org/

- **Kimberlite Docs**:
  - [docs/concepts/formal-verification.md](../docs/concepts/formal-verification.md) - Overview of all 6 layers
  - [docs/internals/formal-verification/protocol-specifications.md](../docs/internals/formal-verification/protocol-specifications.md) - Layer 1 technical details
  - [docs-internal/formal-verification/implementation-complete.md](../docs-internal/formal-verification/implementation-complete.md) - Complete technical report
  - [docs/TESTING.md](../docs/TESTING.md) - VOPR simulation testing
  - [CLAUDE.md](../CLAUDE.md) - Project overview and architecture

## Status

**All 6 Layers Active** (status refreshed 2026-04-17)

Spec authorship:
- [x] Protocol specifications — VSR.tla, ViewChange.tla, Recovery.tla, Compliance.tla, ClockSync.tla, Reconfiguration.tla, ClientSessions.tla, Scrubbing.tla, RepairBudget.tla, plus 23 regulatory framework specs under `compliance/`.
- [x] TLAPS theorem statements — VSR_Proofs.tla (9 theorems), ViewChange_Proofs.tla (3), Recovery_Proofs.tla (4), Compliance_Proofs.tla (10). See the "TLAPS discharge status" row below for which proofs are mechanically verified vs. `PROOF OMITTED`.
- [x] Ivy Byzantine model — VSR_Byzantine.ivy (5 safety invariants).
- [x] Alloy structural models — HashChain.als (scope 10 + scope 5 quick), Quorum.als (scope 8), Simple.als (smoke).
- [x] Coq cryptographic proofs — Common.v, SHA256.v, BLAKE3.v, AES_GCM.v, Ed25519.v, KeyHierarchy.v, MessageSerialization.v, Extract.v (31+ theorems).
- [x] Kani bounded model checking — 143 harnesses across 8 crates.
- [x] VOPR property annotations — ~91 `always!` / `sometimes!` / `never!` / `reached!` markers across 7 crates via `kimberlite-properties`.

TLAPS discharge status (as of 2026-04-17):
- **Mechanically verified (tlapm 1.6.0-pre, stretch 3000):** `ViewMonotonicityTheorem` in VSR_Proofs.tla. That's the only theorem across the four proof files for which tlapm has accepted a proof end-to-end.
- **`PROOF OMITTED` with named outstanding obligation:** every other theorem in VSR_Proofs/ViewChange_Proofs/Recovery_Proofs/Compliance_Proofs. Each `PROOF OMITTED` is paired with a preceding comment that names the specific unproven obligation, per the epistemic-honesty policy in the traceability matrix.
- Bounded model checking (TLC) at depth 8–20 continues to verify the underlying *invariants* (TypeOK, Agreement, PrefixConsistency, ...) on every PR, which is a complementary layer — tlapm adds unbounded coverage but is currently discharged for only one theorem.

CI & infrastructure (refreshed 2026-04-17):
- [x] PR-blocking CI: TLC, Alloy, **Ivy**, Coq, Kani, MIRI.
- [x] Aspirational CI: TLAPS (nightly — only `ViewMonotonicityTheorem`; other theorems remain `PROOF OMITTED`).
- [x] EPYC Hetzner runner: full-capacity verification via `just fv-epyc-all`.
- [x] Traceability matrix: see `docs/internals/formal-verification/traceability-matrix.md` (17 rows mapping spec → Rust site → layers).
- [x] Supply-chain pinning: real SHA-256 on `tla2tools.jar` and `alloy-6.2.0.jar` in both CI and the bootstrap script.

Known limitations (tracked in ROADMAP.md):
- Ivy `v0.1-msv` upstream has Python 2/3 incompatibility; CI runs under
  pinned workaround but full verification depends on a successor tool.
- TLAPS proof engineering: most VSR/ViewChange/Recovery/Compliance
  theorems remain `PROOF OMITTED`. Each omitted proof's preceding
  comment names the specific obligation that would need to discharge
  (typically a case-split per Next action plus a companion invariant).
  TLC model checking covers the same properties bounded; tlapm adds
  unbounded coverage that is a future effort.
- Coq → Rust extraction: `kimberlite-crypto::verified::*` modules are
  hand-written wrappers that embed `ProofCertificate` constants citing
  Coq theorems rather than using auto-generated code. Works today;
  `coq-of-rust` integration is a future item.
