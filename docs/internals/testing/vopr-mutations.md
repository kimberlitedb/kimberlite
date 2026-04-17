---
title: "VOPR Canary Mutations"
section: "internals/testing"
slug: "vopr-mutations"
order: 4
---

# VOPR Canary Mutations

Canary mutations are **intentional bugs** that VOPR must catch. Each is guarded by a Cargo feature flag; none are compiled into production builds. The purpose is negative verification: if VOPR never rejects a known-bad mutation, one of three things is true — the invariant is missing, the invariant isn't running, or the invariant is too weak to catch the mutation.

Source: [`crates/kimberlite-sim/src/canary.rs`](../../../crates/kimberlite-sim/src/canary.rs).

## Canary inventory

| # | Feature flag                     | Mutation                                                          | Invariant that must detect it                                                    |
|---|----------------------------------|-------------------------------------------------------------------|-----------------------------------------------------------------------------------|
| 1 | `canary-skip-fsync`              | Drop `fsync` on 0.1 % of writes                                  | `StorageDeterminismChecker` → `storage_determinism`                              |
| 2 | `canary-wrong-hash`              | Flip the high byte of a hash-chain link                          | `HashChainChecker` → `hash_chain_integrity`                                      |
| 3 | `canary-commit-quorum`           | Commit with `f` replicas instead of `f+1`                        | VSR `Agreement` / `PrefixConsistency` — currently under expansion                |
| 4 | `canary-idempotency-race`        | Record idempotency key *after* apply (TOCTOU window)             | Idempotency checker → no duplicate effect                                        |
| 5 | `canary-monotonic-regression`    | Allow view or op number to decrease                              | `ViewMonotonicity` + `OpNumberMonotonicity` production `assert!()`               |

## Running canaries

Each canary is a Cargo feature on `kimberlite-sim`:

```bash
# Run VOPR with one canary — expected to trigger a detection
cargo test -p kimberlite-sim --features canary-skip-fsync -- --nocapture
cargo test -p kimberlite-sim --features canary-wrong-hash -- --nocapture
cargo test -p kimberlite-sim --features canary-commit-quorum -- --nocapture
cargo test -p kimberlite-sim --features canary-idempotency-race -- --nocapture
cargo test -p kimberlite-sim --features canary-monotonic-regression -- --nocapture
```

An integration-level runner exercising all five lives at `just vopr-canaries` (see justfile).

## Detection expectations

- **Canary 1 (skip-fsync):** surfaces as replica-divergence traces after simulated crashes. Detected within ~10 k iterations on a 3-replica cluster.
- **Canary 2 (wrong-hash):** surfaces immediately — the next hash-chain verification fails.
- **Canary 3 (commit-quorum):** currently surfaces as an invariant violation when prefix-agreement fires across replicas that haven't seen the commit. Strengthening this to catch it at the commit-issuing site is tracked in `ROADMAP.md`.
- **Canary 4 (idempotency-race):** surfaces as a duplicate side-effect the second time an operation is retried across the TOCTOU window.
- **Canary 5 (monotonic-regression):** panics in the `assert!()` on the next message that observes the regression.

## Why we report mutation detection instead of "tests passed"

Test suites green on correct code tell you nothing about whether the tests can catch bugs. A mutation-detection rate is a negative test of the test suite itself.

When README or CLAUDE.md text cites a detection rate, it should be grounded in a recent run of `just vopr-canaries`; the specific numbers drift as invariants gain and lose strength.

## Adding a canary

1. Add a feature flag on `kimberlite-sim/Cargo.toml` named `canary-<what>`.
2. Guard the mutation with `#[cfg(feature = "canary-<what>")]`.
3. Provide a no-op fallback under `#[cfg(not(feature = "canary-<what>"))]`.
4. Add a row to the table above.
5. Add a test that runs VOPR with the feature enabled and asserts a specific invariant fires.
