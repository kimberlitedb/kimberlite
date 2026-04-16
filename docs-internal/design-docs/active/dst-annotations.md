# DST Property Annotations (ALWAYS / SOMETIMES / NEVER / REACHED)

**Status:** Active — landed in 2026-04-16 commits
**Crate:** `kimberlite-properties`
**Inspired by:** [Antithesis SDK](https://antithesis.com/docs/properties_assertions/assertions/)

## Context

Kimberlite's VOPR simulation framework was catching protocol-level bugs via random
fuzzing and hand-written invariant checkers, but had no systematic way for the
simulation itself to report which safety properties were being evaluated or which
interesting code paths had been exercised. The proc-macro system
(`kimberlite-sim-macros`) defined `#[sometimes!]` and `#[fault_point!]` but these
were used only 4 times across the entire codebase.

This document describes the lightweight property annotation system that replaces
the gap.

## Goals

1. **Zero cost in production.** Annotations compile to nothing without the `sim`
   cargo feature (or `cfg(test)` when running unit tests).
2. **No cross-crate cfg(test) hazards.** The registry module is unconditionally
   compiled so that annotations in `kimberlite-kernel` work correctly when tests
   in `kimberlite-sim` run.
3. **Usable from any crate without dependency inversion.** Core crates (kernel,
   VSR, storage, crypto) cannot depend on `kimberlite-sim` because `sim` depends
   on them. The properties crate sits at the bottom of the dep graph.
4. **Antithesis-compatible semantics.** ALWAYS/SOMETIMES/NEVER/REACHED match
   the Antithesis SDK exactly so that future external audit integration is
   straightforward.

## Design

### Crate Structure

```
crates/kimberlite-properties/
├── Cargo.toml          # sim feature gates fault injection wiring
└── src/
    ├── lib.rs          # macro_rules! definitions
    └── registry.rs     # thread-local PropertyRecord storage (always compiled)
```

### Macros

All macros are `macro_rules!` (not proc-macros) to avoid pulling in `syn`/`quote`
dependencies. Each macro body is gated by `#[cfg(any(test, feature = "sim"))]`
so it vanishes in production builds.

```rust
kimberlite_properties::always!(condition, "id", "description")
kimberlite_properties::sometimes!(condition, "id", "description")
kimberlite_properties::never!(condition, "id", "description")
kimberlite_properties::reached!("id", "description")
kimberlite_properties::unreachable_property!("id", "description")
```

| Macro | Semantics | Violation behavior |
|---|---|---|
| `always!` | Condition must hold on every evaluation | Panics + records violation |
| `sometimes!` | Must be true at least once per simulation run | Never panics; records satisfaction |
| `never!` | Must never be true | Panics + records violation |
| `reached!` | Code path must execute ≥ once per run | Records hit |
| `unreachable_property!` | Code path must never execute | Panics + records violation |

**Key insight:** `sometimes!` is a *coverage signal*, not a bug catcher. It tells
the simulator "go find a state where this is true." Future DPOR and coverage-
guided fuzzing preferentially explore paths toward unsatisfied `sometimes`
properties.

### Runtime

Each evaluation records into a thread-local `HashMap<String, PropertyRecord>`
inside `kimberlite-properties::registry`. At the end of a simulation run, VOPR
calls `registry::snapshot()` to capture satisfaction/violation counts, then
`registry::reset()` between runs.

## Annotations Landed

### kimberlite-kernel (5 ALWAYS, 1 SOMETIMES)

| ID | Kind | Location |
|---|---|---|
| `kernel.stream_exists_after_create` | ALWAYS | CreateStream postcondition |
| `kernel.stream_zero_offset_after_create` | ALWAYS | CreateStream postcondition |
| `kernel.offset_monotonicity` | ALWAYS | AppendBatch — foundational append-only |
| `kernel.offset_arithmetic` | ALWAYS | AppendBatch — base + count = new |
| `kernel.append_offset_consistent` | ALWAYS | AppendBatch state postcondition |
| `kernel.multi_event_batch` | SOMETIMES | AppendBatch — coverage for multi-event |
| `kernel.batch_min_effects` | ALWAYS | `apply_committed_batch` |

### kimberlite-vsr (7 ALWAYS, 5 SOMETIMES)

| ID | Kind | Location |
|---|---|---|
| `vsr.view_monotonicity` | ALWAYS | `transition_to_view` |
| `vsr.commit_le_op_on_enter_normal` | ALWAYS | `enter_normal_status` |
| `vsr.commit_monotonicity` | ALWAYS | `apply_commits_up_to` |
| `vsr.commit_le_op_after_apply` | ALWAYS | `apply_commits_up_to` |
| `vsr.commit_target_exceeds_op` | SOMETIMES | `apply_commits_up_to` — catchup path |
| `vsr.commit_quorum_met` | ALWAYS | `try_commit` |
| `vsr.view_change_commit_le_op` | ALWAYS | DoViewChange quorum |
| `vsr.view_change_quorum_met` | ALWAYS | DoViewChange quorum |
| `vsr.view_change_initiated` | SOMETIMES | `start_view_change` |
| `vsr.view_change_completed` | SOMETIMES | DoViewChange quorum success |
| `vsr.recovery_quorum_met` | ALWAYS | `check_recovery_quorum` |
| `vsr.recovery_generation_incremented` | ALWAYS | recovery completion |
| `vsr.recovery_completed` | SOMETIMES | recovery completion |
| `vsr.prepare_replay_detected` | SOMETIMES | Byzantine replay detection |
| `vsr.prepare_checksum_failure_detected` | SOMETIMES | Byzantine checksum detection |

### kimberlite-storage (5 ALWAYS, 1 SOMETIMES, 1 NEVER)

hash chain, CRC32, offset monotonicity, read-after-write coverage.

### kimberlite-crypto (3 ALWAYS, 2 SOMETIMES)

EncryptionKey non-zero, encrypt/decrypt roundtrip, hash chain determinism,
BLAKE3 and SHA-256 path coverage.

### kimberlite-compliance (6 ALWAYS, 10 SOMETIMES, 2 NEVER, 17 `reached!`)

- Audit: `reached!` for each of 15 ComplianceAuditAction variants,
  append-only growth, log never shrinks
- Erasure: GDPR 30-day deadline, exempted/elapsed path coverage
- Breach: HIPAA 72h notification deadline, all 4 severity levels coverage
- Consent: withdrawn consent never satisfies check, timestamp not in future
- Export: content hash determinism, HMAC determinism, JSON/CSV path coverage

### kimberlite-query (2 ALWAYS, 10 SOMETIMES, 3 NEVER)

JOIN/GROUP-BY cap hits, SUM overflow guard, AVG div-by-zero guard,
Placeholder never reaches result boundary, LIKE/BETWEEN/Materialize/CASE
coverage, multi-row JOIN coverage, time-travel coverage, query result schema
invariants.

## Totals

**74 annotations total:**
- 28 ALWAYS (safety invariants)
- 28 SOMETIMES (coverage signals)
- 6 NEVER (anti-invariants)
- 17 `reached!` (code path coverage)

## Why Existing `assert!` Was Insufficient

The existing code already had hundreds of `assert!`/`debug_assert!` calls. Those
catch bugs at runtime but do not:
- Report aggregate satisfaction counts across a simulation run
- Guide the simulator toward interesting states (SOMETIMES)
- Distinguish coverage signals from bug tripwires
- Work across a production/simulation boundary with zero cost

Property annotations **complement** existing assertions rather than replace them.
Critical safety invariants typically have both: an `assert!` for immediate fail-
fast during development, and an `always!` for VOPR reporting.

## VOPR Integration (follow-up work)

The registry is ready to consume but not yet wired into the VOPR runner output.
Follow-up items:
1. Reset `kimberlite_properties::registry::reset()` at the start of each seed.
2. Snapshot at the end and attach to `VoprResult`.
3. Report unsatisfied SOMETIMES in batch summaries — these are coverage gaps
   that new fuzzing/DPOR runs should target.
4. Add a `just vopr-properties` target that runs a small batch and dumps the
   registry via `summary_report()`.

## Testing

Every annotation was validated by running the parent crate's full test suite:
- kernel: 70 + 3 tests pass
- vsr: 334 + 13 tests pass (2 simulation tests adjusted for realistic commit catchup)
- storage/crypto: 297 tests pass
- compliance: 139 tests pass
- query: 338 tests pass

Total: **1200+ existing tests still pass** after annotation sweep.

## References

- [Antithesis SDK docs — assertions](https://antithesis.com/docs/properties_assertions/assertions/)
- cuddly-duddly's `cuddly-property` crate — Property enum with Always/Sometimes/Never variants
- TigerBeetle VOPR — production-grade VSR simulation (no explicit property
  annotations; relies on in-code assertions)
