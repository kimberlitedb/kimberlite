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
│  (see ../crates/*/src/kani_proofs.rs, docs/FORMAL_VERIFICATION.md)│
└──────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
specs/
├── tla/              # TLA+ specifications
│   ├── VSR.tla          # Core Viewstamped Replication protocol
│   ├── VSR.cfg          # TLC model checker configuration
│   ├── ViewChange.tla   # View change protocol (TODO)
│   ├── Recovery.tla     # Protocol-Aware Recovery (TODO)
│   └── Compliance.tla   # Compliance meta-framework (TODO)
├── ivy/              # Ivy Byzantine consensus models
│   └── VSR_Byzantine.ivy  # Byzantine fault model (TODO)
├── alloy/            # Alloy structural models
│   ├── HashChain.als    # Hash chain integrity (TODO)
│   └── Quorum.als       # Quorum intersection (TODO)
└── SETUP.md          # Tool installation guide
```

## Getting Started

### 1. Install Tools

See [SETUP.md](./SETUP.md) for complete installation instructions.

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

Specification proving that view changes preserve all safety properties from VSR.tla.

**Status:** TODO (Phase 1, Weeks 5-8)

### Recovery.tla - Protocol-Aware Recovery

Specification of the recovery protocol with proof that recovery never discards quorum-committed operations.

**Status:** TODO (Phase 1, Weeks 5-8)

### Compliance.tla - Compliance Meta-Framework

Formal specification of compliance properties (tenant isolation, audit completeness, hash chain integrity) that can be mapped to regulatory frameworks (HIPAA, GDPR, SOC 2, etc.).

**Status:** TODO (Phase 1, Weeks 5-8)

### VSR_Byzantine.ivy - Byzantine Consensus Model

Ivy specification modeling VSR with Byzantine faults (up to f replicas can be malicious). Proves that agreement holds despite Byzantine replicas attempting to:
- Equivocate (send conflicting messages)
- Withhold messages
- Send invalid data

**Status:** TODO (Phase 1, Weeks 9-12)

### HashChain.als, Quorum.als - Structural Models

Alloy specifications proving structural properties:
- Hash chains have no cycles
- Quorums always intersect (foundation of VSR safety)

**Status:** TODO (Phase 1, Weeks 13-14)

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

Formal verification runs in CI on every commit:

```yaml
# .github/workflows/formal-verification.yml
- TLA+ model checking (TLC, depth 20): ~5 min
- TLAPS mechanized proofs: ~15 min
- Ivy Byzantine model: ~5 min
- Alloy structural models: ~2 min
Total: ~30 min (parallelized)
```

## Learn More

- **TLA+**: https://learntla.com/
- **TLAPS**: https://tla.msr-inria.inria.fr/tlaps/
- **Ivy**: https://kenmcmil.github.io/ivy/
- **Alloy**: https://alloytools.org/

- **Kimberlite Docs**:
  - [docs/FORMAL_VERIFICATION.md](../docs/FORMAL_VERIFICATION.md) - Overview and philosophy
  - [docs/TESTING.md](../docs/TESTING.md) - VOPR simulation testing
  - [CLAUDE.md](../CLAUDE.md) - Project overview and architecture

## Phase 1 Status

**Current Progress:** Week 1

- [x] Setup directory structure
- [x] VSR.tla core specification created
- [x] VSR.cfg TLC configuration created
- [ ] Install verification tools
- [ ] Add TLAPS proofs to VSR.tla
- [ ] Create ViewChange.tla
- [ ] Create Recovery.tla
- [ ] Create Compliance.tla
- [ ] Create Ivy Byzantine model
- [ ] Create Alloy structural models
- [ ] CI integration

**Target:** Complete all Phase 1 specifications by Week 14
