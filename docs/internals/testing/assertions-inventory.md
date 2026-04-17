---
title: "Production Assertion Inventory"
section: "internals/testing"
slug: "assertions-inventory"
order: 3
---

# Production Assertion Inventory

This document is the authoritative source for **where production `assert!()` invariants live** and **how to audit them**. It replaces the unverifiable "38 production assertions" claim that appeared in earlier documentation.

The distinction between production (`assert!`) and development (`debug_assert!`) assertions matters: production assertions fail in release builds, debug assertions are compiled out. Every production assertion is a runtime invariant that crashes the process if violated — intentionally, because silent continuation on broken invariants is worse than a loud crash in a compliance-first system.

## Crates with production invariants

| Crate                 | Focus                                        | Dedicated assertion tests                                       |
|-----------------------|----------------------------------------------|------------------------------------------------------------------|
| `kimberlite-crypto`   | Cryptographic primitives and hash chains     | [`crates/kimberlite-crypto/src/tests_assertions.rs`](../../../crates/kimberlite-crypto/src/tests_assertions.rs) |
| `kimberlite-vsr`      | Consensus safety invariants                  | [`crates/kimberlite-vsr/src/tests_assertions.rs`](../../../crates/kimberlite-vsr/src/tests_assertions.rs)       |
| `kimberlite-kernel`   | State-machine postconditions                 | Inline `#[cfg(test)] mod tests` in `kernel.rs`                   |

## How to enumerate

Run the following from the workspace root to list every `assert!(` in non-test code:

```bash
rg -n '^\s*assert!\(' \
  --glob 'crates/kimberlite-{crypto,vsr,kernel}/src/**' \
  --glob '!**/tests_assertions.rs' \
  --glob '!**/tests.rs'
```

To count:

```bash
rg -c '^\s*assert!\(' \
  crates/kimberlite-{crypto,vsr,kernel}/src \
  --glob '!**/tests*'
```

Counts drift as the codebase evolves; a specific number here would bit-rot within weeks. The grep above is the source of truth.

## Categories

### Cryptography (`kimberlite-crypto`)

- **Hash chain integrity** — `chain.rs`: previous-hash matches, offset monotonicity.
- **Key hierarchy** — all-zero detection, key-derivation preconditions.
- **Ciphertext validation** — GCM tag length, nonce uniqueness on encrypt.
- **Verified-module proof certificates** — `verified/*.rs`: `ProofCertificate::is_complete()` gates the claim of mechanically-verified behaviour.

### Consensus (`kimberlite-vsr`)

- **Leader monopoly** — only the leader in a view may send `Prepare`.
- **View monotonicity** — view numbers never decrease (a promoted production assertion).
- **Commit-number monotonicity** — `commit_number` only advances.
- **View-change preserves commits** — no committed op is lost when changing views.
- **Recovery preserves commits** — recovery never re-orders or drops committed prefix.
- **Quorum validation** — `f+1` signatures required before commit.
- **Message replay detection** — `(view, op_number, replica_id)` triple uniqueness.
- **Op-number monotonicity** — appended op numbers are strictly increasing.
- **Commit bound** — committed ≤ prepared ≤ log size.

### State machine (`kimberlite-kernel`)

- **Stream existence after creation** — `CreateStream` postcondition.
- **Offset monotonicity after append** — `Append` postcondition.
- **Effect completeness** — each command emits its full effect vector.

## Testing policy

Every production `assert!` has a paired negative test in the crate's `tests_assertions.rs` (or `#[cfg(test)]` module for `kimberlite-kernel`). The test drives the code into the precondition violation and asserts the panic via `#[should_panic]`.

Adding a new production assertion requires:

1. The `assert!(...)` itself, with a message that identifies the invariant.
2. A `#[should_panic(expected = "…")]` test exercising the violation path.
3. A line in `CHANGELOG.md` under "Unreleased → Safety" noting the promotion.

See [assertions.md](./assertions.md) for the decision matrix (production vs debug) and performance rationale.

## Performance

Production assertions are cold branches in the common case. Throughput impact is bounded by the cost of a successful branch prediction; profiling on EPYC 7502P (2026-04) found the regression within benchmark noise. Raw numbers live in [`docs/internals/benchmarks/`](../benchmarks/) when that directory exists; until then, reproduce with:

```bash
just bench-baseline   # assertions off in release
just bench            # assertions on in release (default)
```

The difference is what we care about. Both must be reported together or the number is meaningless.

## History

- Earlier documentation claimed "38 critical assertions promoted to production". An audit in April 2026 found the real count at ~17 (see `CHANGELOG.md` Unreleased section) and promoted 6 additional consensus assertions. This document replaces that specific-but-wrong count with a grep-backed inventory.
