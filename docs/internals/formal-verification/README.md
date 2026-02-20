---
title: "Formal Verification Internals"
section: "internals/formal-verification"
slug: "README"
order: 0
---

# Formal Verification Internals

This directory contains technical documentation for Kimberlite's 6-layer formal verification stack.

## Quick Navigation

**For Users:**
- **[Overview of all 6 layers](..//docs/concepts/formal-verification)** - Start here for a high-level introduction to formal verification and what it means for Kimberlite

**For Contributors & Researchers:**
- **[Protocol Specifications (Layer 1)](protocol-specifications.md)** - Technical details of TLA+, TLAPS, Ivy, and Alloy verification
- **[Complete Technical Report](../../../docs-internal/formal-verification/implementation-complete.md)** - Comprehensive documentation of all 6 layers with full technical details
- **[Traceability Matrix](../../traceability_matrix.md)** - TLA+ → Rust → VOPR mapping (100% coverage)

## Layer Documentation

| Layer | Location | Description |
|-------|----------|-------------|
| **Layer 1: Protocol Specs** | [protocol-specifications.md](protocol-specifications.md) | TLA+/TLAPS (25 theorems), Ivy (5 invariants), Alloy models |
| **Layer 2: Crypto Verification** | `specs/coq/` | 5 Coq specifications, 31 theorems |
| **Layer 3: Code Verification** | `crates/*/src/kani_proofs.rs` | 91 Kani proofs across all modules |
| **Layer 4: Type-Level Enforcement** | `crates/kimberlite-types/src/flux_annotations.rs` | 80+ Flux refinement type signatures |
| **Layer 5: Compliance Modeling** | `specs/tla/compliance/` | 8 TLA+ specs (6 frameworks + meta-framework) |
| **Layer 6: Integration** | `crates/kimberlite-sim/src/trace_alignment.rs` | Traceability matrix, VOPR validation |

## Verification Commands

```bash
# Layer 1: Protocol verification
just verify-tlaps    # TLA+ mechanized proofs
just verify-ivy      # Ivy Byzantine model
just verify-alloy    # Alloy structural models
just verify-local    # All protocol tools

# Layer 2: Cryptographic verification
cd specs/coq
coqc SHA256.v BLAKE3.v AES_GCM.v Ed25519.v KeyHierarchy.v

# Layer 3: Code verification
cargo kani --workspace

# Layer 5: Compliance verification
cd specs/tla/compliance
tlc HIPAA.tla GDPR.tla SOC2.tla PCI_DSS.tla ISO27001.tla FedRAMP.tla

# Layer 6: Traceability validation
cargo test --package kimberlite-sim --lib trace_alignment
```

## Achievement

Kimberlite is the **world's first database with complete 6-layer formal verification**, with:
- **136+ machine-checked proofs** (91 Kani + 25 TLA+ + 15 Coq + 5 Ivy)
- **100% traceability** (19/19 theorems mapped: TLA+ → Rust → VOPR)
- **6 compliance frameworks** formally modeled (HIPAA, GDPR, SOC 2, PCI DSS, ISO 27001, FedRAMP)

For questions or contributions, see [CONTRIBUTING.md](../../../CONTRIBUTING.md).
