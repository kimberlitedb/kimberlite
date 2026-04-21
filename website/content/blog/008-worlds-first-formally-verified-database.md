---
title: "Kimberlite's Six-Layer Formal Methods Stack"
date: "2026-02-05"
author: "Jared Reyes"
summary: "Kimberlite's verification stack spans six layers — TLA+ protocol specs, Coq crypto proofs, Alloy structural models, Ivy Byzantine invariants, Kani bounded model checking, and MIRI undefined-behavior detection. Honest accounting of what's PR-gated, what's nightly, and what's specification-only."
tags: ["formal-verification", "milestone", "compliance"]
---

> **Retitled & rewritten April 2026.** This post originally claimed
> "world's first" / "most formally verified database ever built" and cited
> 136+ machine-checked proofs. Those numbers conflated specification
> artifacts with mechanically-verified theorems. Kimberlite is *not* the
> first database to use formal methods — TigerBeetle uses Ivy, FoundationDB
> has record-layer proofs, and seL4 (an operating-system kernel, not a
> database) has been fully verified in Isabelle/HOL since 2009. What we
> *do* have is a six-layer stack wired into CI that combines protocol
> specs, crypto proofs, structural models, Byzantine invariants, bounded
> model checking, and UB detection — with the honest delta between "spec
> exists" and "theorem discharges in CI" documented row-by-row in the
> [traceability matrix](/docs/internals/formal-verification/traceability-matrix).
> This rewrite replaces the original body with numbers that match that
> matrix. See `CHANGELOG.md § v0.4.2 "truth-in-advertising patch"` and
> `§ v0.5.0 "blog 008 rewrite-in-place"` for the lineage.

Today I'm sharing where Kimberlite's formal-methods program actually
stands: six verification tools running in CI, covering protocol
specifications, cryptographic primitives, structural invariants, Byzantine
consensus, bounded model checking of Rust code, and undefined-behavior
detection.

The precise counts — which theorems run on every PR versus which ones
need the Hetzner EPYC box to discharge — are in the traceability matrix.
The short version, as of v0.5.0:

- **5 TLAPS theorems** mechanically discharge at `--stretch 3000`
  (`ViewMonotonicity`, `SafetyProperties`, `TenantIsolation`,
  `AccessControlCorrectness`, `EncryptionAtRest`). Since v0.5.0 they are
  PR-gated at bounded `--stretch 300`; full-stretch runs continue
  nightly.
- **4 Category-A TLAPS files** verify file-level at `--stretch 3000` on
  the nightly aspirational workflow (16 Category-A theorems total across
  `Compliance_Proofs.tla`, `Recovery_Proofs.tla`, `ViewChange_Proofs.tla`,
  `VSR_Proofs.tla`).
- **5 Ivy Byzantine invariants** are PR-gated on `VSR_Byzantine.ivy`.
- **~91 Kani bounded-model-checking harnesses** are PR-gated.
- **5 Coq crypto specifications** (SHA-256, BLAKE3, AES-GCM, Ed25519,
  Key Hierarchy) verify with hand-written Rust wrappers; full
  extraction-to-Rust is v1.0.0 work.
- **MIRI** runs on storage, crypto, and types crates on every PR.

Everything else from the old post — "136+ proofs", "most verified
database", "no other database can match" — was promotional language that
conflated specification lines with mechanically-verified theorems. The
matrix is the source of truth; this post tells you how the tools fit
together.

## What formal methods do, and don't do, for Kimberlite

Testing proves presence of bugs, not absence. Formal methods — mechanized
proofs, exhaustive bounded model checking, structural analysis — can
prove absence within their modeled scope. The operative word is *scope*:
every one of our tools has bounds (a finite state space, a small-model
hypothesis, a bounded unwinding depth, a specification abstraction).
Proofs in scope rule out entire bug classes; out-of-scope bugs still
need tests, VOPR (our deterministic simulation framework), and
continuous fuzzing.

That's how Kimberlite is built: proofs where they pay off, VOPR for the
liveness and fault-injection properties TLA+ machinery doesn't reach,
libfuzzer for unmodeled input surfaces, and production assertions for
invariants we can't encode statically.

## The six layers

### Layer 1: Protocol specifications (TLA+, Ivy, Alloy)

- **TLA+ TLC (PR-gated):** Bounded exhaustive model checking of VSR,
  ViewChange, Recovery, and Compliance specs. Small configs
  (MaxView=2, MaxOp=2) explore every reachable state at depth 10.
- **TLA+ TLAPS (mixed):** 5 theorems PR-gated at `--stretch 300`,
  16 Category-A theorems at `--stretch 3000` on the nightly
  workflow, full `--stretch 10000` runs on EPYC.
- **Ivy (PR-gated):** `VSR_Byzantine.ivy` proves 5 safety invariants
  tolerating f < n/3 malicious replicas.
- **Alloy (PR-gated):** Quick-scope variants of `HashChain.als`,
  `Quorum.als`, `Simple.als` run on every PR; full scope 10 runs
  nightly.

Key properties covered (the traceability matrix names the specific
theorems and which tool discharges each):
Agreement; PrefixConsistency; ViewMonotonicity; OpNumberMonotonicity;
LeaderUniqueness; ViewChangePreservesCommits; RecoveryPreservesCommits;
QuorumIntersection; MessageReplayDetection; SignatureNonRepudiation.

### Layer 2: Cryptographic verification (Coq)

Five specifications in `specs/coq/`:
SHA-256 (collision resistance, determinism, hash-chain genesis
integrity); BLAKE3 (same + tree-hash properties); AES-256-GCM
(authenticated encryption, integrity of ciphertext modification);
Ed25519 (EUF-CMA unforgeability); KeyHierarchy (tenant-isolation
injectivity).

Today we run Coq against the specifications and ship hand-written Rust
wrappers in `crates/kimberlite-crypto/src/verified/`. Coq-to-Rust
extraction is planned for v1.0.0 and is called out in the roadmap.

### Layer 3: Bounded model checking of Rust code (Kani)

About 91 Kani harnesses live under `src/kani_proofs.rs` modules across
the consensus, storage, and crypto crates. They drive SMT solvers over
bounded unwindings (typical unwind 32) to rule out specific bug classes
in the real compiled code:

Offset monotonicity (stream offsets never decrease); stream uniqueness
(no duplicate stream IDs); hash-chain integrity (tamper detection);
CRC32 corruption detection; cross-module type consistency;
view-number monotonicity; recovery preserves the committed prefix;
quorum intersection geometry; message-dedup rejects replays.

Kani runs on every PR via `.github/workflows/formal-verification.yml`.

### Layer 4: Refinement-type annotations (Flux)

About 80 `#[flux(...)]` refinement-type signatures sit ready in the
consensus and types crates. They encode tenant isolation, offset
monotonicity, quorum geometry, and view-number monotonicity at the type
level. Today they compile as annotations and are validated against the
Kani harnesses above; when the upstream Flux compiler stabilises we'll
turn on zero-runtime-overhead refinement checking in CI. Status:
specification-complete, compiler-pending.

### Layer 5: Compliance meta-framework (TLA+)

`specs/tla/Compliance.tla` and `Compliance_Proofs.tla` state the core
compliance properties (tenant isolation, access-control correctness,
encryption at rest, audit completeness, hash-chain integrity) once, and
then a meta-theorem layer (`HIPAA_ComplianceTheorem`,
`GDPR_ComplianceTheorem`, `SOC2_ComplianceTheorem`, plus
`MetaFrameworkTheorem` tying them together) shows the framework-specific
requirements follow from the core properties by construction.

The five "core" compliance theorems are PR-gated via TLAPS as of v0.5.0.
The framework-composition meta-theorems are Category-A at `--stretch
3000` nightly. PCI DSS, ISO 27001, and FedRAMP exist as specification
artifacts and need PR-gating before we describe them as mechanically
verified.

### Layer 6: Undefined-behavior detection (MIRI) + Integration (VOPR)

**MIRI** runs on `kimberlite-storage`, `kimberlite-crypto`, and
`kimberlite-types` on every PR, catching memory-safety and pointer-
aliasing bugs in any `unsafe` code (Kimberlite denies `unsafe_code` at
the workspace level, but the FFI crate is excluded from that deny — MIRI
is the belt-and-braces check).

**VOPR** (our deterministic simulation framework) covers what formal
methods can't: liveness properties (eventual progress, eventual commit),
tenant-isolation behaviour under fault injection, cluster
reconfiguration dynamics, and adversarial Byzantine sequences. VOPR
isn't a proof — it's exhaustive exploration of a bounded execution
space with the same determinism guarantees. The canary mutation suite
(five injected bugs) validates that VOPR actually catches regressions
at 100% detection rate.

## How Kimberlite compares — honestly

We are not the first. A short walk through the actual state of the art:

- **seL4** (microkernel, not a database) is fully verified in
  Isabelle/HOL top-to-bottom, including binary-level refinement. That
  bar is still the high-water mark; we don't claim to meet it.
- **TigerBeetle** uses Ivy for consensus invariants and a deterministic
  simulator that predates VOPR. They inspired our approach.
- **FoundationDB** publishes record-layer proofs and famously tests with
  a deterministic simulator that finds bugs other databases can't see.
- Academic systems (CertiKOS, PnVer, IronKV) demonstrate end-to-end
  verification of specific properties.

What we do claim: Kimberlite combines **six verification tools into one
CI pipeline** — protocol specs, Byzantine consensus, structural models,
crypto proofs, bounded model checking of the actual Rust, and UB
detection — and publishes a row-by-row traceability matrix that maps
every theorem to the Rust site where it's enforced. If you find a
property in the matrix, you can find the `.tla`/`.als`/`.ivy`/`.v`
file, the Rust source line with the production assertion or harness,
and the CI step that discharges it on PR or nightly.

That combination — not any single tool — is what we think is worth
publicising, and it's what the six-layer framing is really about.

## What this means for users

For developers: tenant-isolation enforcement is provable at the type,
Rust, TLA+, and Coq layers — the compiler rejects cross-tenant
operations before they reach the wire. Hash chains are tamper-evident
(SHA-256 + BLAKE3 dual-hash) with Coq proofs of chain-hash integrity.
Audit logs are append-only by construction (no destructive update
operations in the kernel state machine).

For compliance officers: the compliance meta-framework generates
traceable mappings between HIPAA / GDPR / SOC 2 / PCI DSS / ISO 27001 /
FedRAMP requirements and the five core compliance properties. Those
five are mechanically verified at PR time as of v0.5.0. The framework
mappings themselves are formalised in TLA+; whether you want a
third-party audit on top is an organisational question, not a
verification one — and we flag that explicitly in our
`docs/concepts/compliance.md`.

For engineering leaders: formal methods are an insurance layer, not a
replacement for tests, code review, or operational vigilance. We run
all three. The matrix tells you where verification reaches and where
VOPR, fuzzing, or testing carries the load instead.

## Reproducing what's in this post

Everything here is reproducible from a clean checkout:

```bash
# Protocol verification
just verify-tla              # TLA+ TLC (PR-gated bounded runs)
just verify-tlaps            # TLAPS mechanized proofs (slow; nightly-grade)
just verify-ivy              # Ivy Byzantine model (5 invariants)
just verify-alloy            # Alloy structural models

# Cryptographic verification
cd specs/coq && coqc SHA256.v BLAKE3.v AES_GCM.v Ed25519.v KeyHierarchy.v

# Code verification
cargo kani --workspace       # Kani bounded model checking
cargo +nightly miri test --package kimberlite-storage

# VOPR deterministic simulation
just vopr-quick              # 100-iteration smoke
just vopr-full 10000         # all scenarios, 10k iterations each

# Traceability
open docs/internals/formal-verification/traceability-matrix.md
```

On a laptop, the PR-gated subset (TLC small configs + Kani + Alloy
quick-scope + Ivy + Coq + MIRI + the 5 TLAPS theorems at
`--stretch 300`) runs in about 40 minutes end-to-end. The nightly
aspirational subset (full-stretch TLAPS) runs in about 2 hours on
ubuntu-latest runners; EPYC runs the full `--stretch 10000` regime in a
few hours.

## What's next

1. Move more Category-B TLAPS theorems to PR-gating (follow-up to the
   v0.5.0 promotion; tracked in ROADMAP under `TLA+ Liveness
   Infrastructure`).
2. Finish the Coq → Rust extraction that retires the hand-written
   `crates/kimberlite-crypto/src/verified/` wrappers (v1.0.0).
3. Turn on Flux refinement checking in CI once the upstream compiler
   stabilises.
4. Ivy migration to Apalache, removing the Python 2/3 compat workaround
   on the nightly runner (v1.0.0).

The honest picture, plus clear roadmap: that's what we think is worth
publishing. If a theorem in this post isn't where you'd expect it,
cross-check the traceability matrix — it's normative, this post is
narrative.

---

**Questions or feedback?** Open an issue on [GitHub](https://github.com/kimberlitedb/kimberlite)
or reach out on social media.

**Want to contribute?** See the [contributor guide](/docs-internal/contributing/).
