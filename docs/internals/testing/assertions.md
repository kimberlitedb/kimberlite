---
title: "Production Assertion Strategy"
section: "internals/testing"
slug: "assertions"
order: 2
---

# Production Assertion Strategy

## Overview

This document provides comprehensive guidance on assertion usage in Kimberlite, covering when to use production assertions (`assert!()`) vs development assertions (`debug_assert!()`), and how to write effective assertions that catch bugs early.

**Key Principle**: Assertions are executable documentation of invariants. They detect corruption, Byzantine attacks, and state machine bugs BEFORE they propagate.

## Table of Contents

1. [Decision Matrix](#decision-matrix)
2. [The 38 Promoted Assertions](#the-38-promoted-assertions)
3. [Writing Effective Assertions](#writing-effective-assertions)
4. [Testing Assertions](#testing-assertions)
5. [Performance Considerations](#performance-considerations)
6. [Incident Response](#incident-response)

---

## Decision Matrix

### Use `assert!()` (Production Enforcement) When:

1. **Cryptographic Invariants**:
   - All-zero detection (keys, hashes, nonces, signatures)
   - Key hierarchy integrity (Master→KEK→DEK wrapping)
   - Ciphertext validation (auth tag presence, minimum sizes)
   - **Rationale**: Cryptographic failures can compromise entire system security

2. **Consensus Safety Properties**:
   - Leader-only operations (only leader can prepare)
   - View number monotonicity (prevents rollback attacks)
   - Commit number monotonicity (prevents uncommit)
   - Sequential commit ordering (prevents gaps)
   - Quorum validation (f+1 responses required)
   - **Rationale**: Consensus violations lead to data loss or divergence

3. **State Machine Invariants**:
   - Stream existence postconditions (stream must exist after creation)
   - Effect count validation (ensures complete audit log)
   - Offset monotonicity (append-only guarantee)
   - Stream metadata consistency
   - **Rationale**: State machine bugs propagate and corrupt derived state

4. **Compliance-Critical Properties**:
   - Tenant isolation (no cross-tenant data leakage)
   - Audit trail completeness (every operation logged)
   - Data integrity (checksums match content)
   - **Rationale**: Compliance violations have legal/regulatory consequences

5. **Memory Safety in Unsafe Blocks**:
   - Buffer bounds checking
   - Alignment requirements
   - Null pointer checks
   - **Rationale**: Safety violations cause undefined behavior

### Use `debug_assert!()` (Development Only) When:

1. **Performance-Critical Hot Paths**:
   - Tight loops with assertion overhead >1% of function time
   - After profiling confirms assertion is bottleneck
   - **Example**: Per-byte checksum validation in tight loop

2. **Redundant Checks**:
   - Type system already prevents the error
   - Previous production assertion guarantees the property
   - **Example**: After validating array bounds with `assert!()`, subsequent accesses can use `debug_assert!()`

3. **Developer Convenience**:
   - Precondition checks that are always satisfied in correct usage
   - Internal helper function invariants
   - **Example**: `debug_assert!(sorted_array.is_sorted())` after sorting

### Never Use Assertions For:

1. **Input Validation**: Use `Result` types and return errors
   ```rust
   // WRONG:
   assert!(user_input.len() < MAX_SIZE);

   // RIGHT:
   if user_input.len() >= MAX_SIZE {
       return Err(ValidationError::TooLarge);
   }
   ```

2. **Control Flow**: Use `if/else` or `match`
   ```rust
   // WRONG:
   assert!(condition);
   do_something();

   // RIGHT:
   if condition {
       do_something();
   }
   ```

3. **Expected Errors**: Use error handling
   ```rust
   // WRONG:
   let file = std::fs::read_to_string(path);
   assert!(file.is_ok(), "file not found");

   // RIGHT:
   let file = std::fs::read_to_string(path)
       .context("failed to read config file")?;
   ```

---

## The 38 Promoted Assertions

### Cryptography (25 assertions)

**All-Zero Detection (11)**:

Location: `crates/kimberlite-crypto/src/encryption.rs`

```rust
// Encryption keys
assert!(
    !key.0.iter().all(|&b| b == 0),
    "encryption key is all zeros - RNG failure or memory corruption"
);

// Signing keys
assert!(
    !signing_key.0.iter().all(|&b| b == 0),
    "signing key is all zeros - RNG failure or memory corruption"
);

// Nonces (Initialization Vectors)
assert!(
    !nonce.iter().all(|&b| b == 0),
    "nonce is all zeros - RNG failure or replay attack"
);

// Hashes
assert!(
    !hash.0.iter().all(|&b| b == 0),
    "hash is all zeros - computation error or memory corruption"
);

// Signatures
assert!(
    !signature.0.iter().all(|&b| b == 0),
    "signature is all zeros - signing failure or memory corruption"
);
```

**Why**: All-zero cryptographic material indicates:
- RNG failure (not properly seeded)
- Memory corruption (zeroed memory)
- Uninitialized data
- Replay attack (reused nonce)

**Key Hierarchy Integrity (9)**:

Location: `crates/kimberlite-crypto/src/chain.rs`

```rust
// Master Key → KEK wrapping
assert!(
    wrapped_kek.len() >= TAG_LENGTH,
    "wrapped KEK too short: {wrapped_kek_len} bytes, need at least {TAG_LENGTH}"
);

// KEK → DEK wrapping
assert!(
    wrapped_dek.len() >= TAG_LENGTH,
    "wrapped DEK too short: {wrapped_dek_len} bytes, need at least {TAG_LENGTH}"
);

// Unwrapping validation
assert!(
    unwrapped_key.len() == KEY_LENGTH,
    "unwrapped key wrong size: {actual} bytes, expected {KEY_LENGTH}"
);
```

**Why**: Key hierarchy violations compromise entire encryption scheme. If KEK is corrupted, all DEKs are unrecoverable.

**Ciphertext Validation (5)**:

Location: `crates/kimberlite-crypto/src/encryption.rs`

```rust
// Minimum size check
assert!(
    ciphertext.len() >= TAG_LENGTH,
    "ciphertext too short: {ciphertext_len} bytes, need at least {TAG_LENGTH}"
);

// Auth tag presence
assert!(
    ciphertext.len() >= plaintext.len() + TAG_LENGTH,
    "ciphertext missing auth tag: {ciphertext_len} bytes for {plaintext_len} plaintext"
);

// Output buffer size
assert!(
    output.len() >= ciphertext.len() - TAG_LENGTH,
    "output buffer too small: {output_len} bytes, need {min_len}"
);
```

**Why**: Ciphertext format violations indicate:
- Truncated data (storage corruption)
- Missing authentication tag (forgery attempt)
- Buffer overflow vulnerability

### Consensus (9 assertions)

**Leader-Only Operations (1)**:

Location: `crates/kimberlite-vsr/src/replica/state.rs`

```rust
assert!(
    self.is_leader(),
    "only leader can prepare operations (current view: {}, replica: {})",
    self.view,
    self.replica_id
);
```

**Why**: Followers preparing operations violates VSR protocol and causes divergence. This is either a Byzantine attack or a critical logic bug.

**View Number Monotonicity (2)**:

Location: `crates/kimberlite-vsr/src/replica/view_change.rs`

```rust
assert!(
    new_view >= self.view,
    "view number regressed from {} to {} - Byzantine attack or logic bug",
    self.view,
    new_view
);

assert!(
    start_view.view > self.last_normal_view,
    "StartView view {} not greater than last normal view {} - Byzantine attack",
    start_view.view,
    self.last_normal_view
);
```

**Why**: View number regression enables rollback attacks where Byzantine leader reverts committed operations.

**Commit Number Monotonicity (2)**:

Location: `crates/kimberlite-vsr/src/replica/state.rs`

```rust
assert!(
    new_commit >= self.commit_number,
    "commit number regressed from {} to {} - Byzantine attack or state corruption",
    self.commit_number,
    new_commit
);

assert!(
    next_commit == self.commit_number.as_u64() + 1,
    "commit gap detected: current {}, next {} - missing operation",
    self.commit_number,
    next_commit
);
```

**Why**: Commit regression or gaps violate linearizability and can cause data loss.

**Quorum Validation (2)**:

Location: `crates/kimberlite-vsr/src/checkpoint.rs`, `view_change.rs`

```rust
assert!(
    responses.len() >= quorum_size,
    "insufficient responses: got {}, need {} - Byzantine attack or network partition",
    responses.len(),
    quorum_size
);

assert!(
    quorum_size == (cluster_size / 2) + 1,
    "quorum calculation error: {quorum_size} for cluster size {cluster_size}"
);
```

**Why**: Quorum violations break Byzantine fault tolerance guarantees (tolerates f failures in 2f+1 cluster).

**Cluster Membership (2)**:

Location: `crates/kimberlite-vsr/src/replica/state.rs`

```rust
assert!(
    self.config.replicas.contains(&replica_id),
    "unknown replica {replica_id:?} not in cluster configuration"
);

assert!(
    self.config.replicas.len() >= 3,
    "cluster too small: {} replicas, need at least 3 for Byzantine fault tolerance",
    self.config.replicas.len()
);
```

**Why**: Messages from unknown replicas indicate configuration error or attack. Clusters <3 nodes cannot tolerate any failures.

### State Machine (4 assertions)

**Stream Existence Postconditions (1)**:

Location: `crates/kimberlite-kernel/src/kernel.rs`

```rust
assert!(
    new_state.stream_exists(&stream_id),
    "stream {stream_id:?} must exist after creation - state machine bug"
);
```

**Why**: If stream creation succeeds but stream doesn't exist, state machine is broken and subsequent operations will fail.

**Effect Count Validation (1)**:

Location: `crates/kimberlite-kernel/src/kernel.rs`

```rust
assert!(
    effects.len() > 0,
    "command {command:?} produced no effects - audit log incomplete"
);
```

**Why**: Every state-modifying command must produce at least one effect for audit trail completeness. Zero effects indicates bug.

**Offset Monotonicity (1)**:

Location: `crates/kimberlite-kernel/src/kernel.rs`

```rust
assert!(
    new_offset > current_offset,
    "append offset did not increase: current {current_offset}, new {new_offset} - state machine bug"
);
```

**Why**: Append-only streams must have monotonically increasing offsets. Violation breaks append-only guarantee.

**Stream Metadata Consistency (1)**:

Location: `crates/kimberlite-kernel/src/kernel.rs`

```rust
assert!(
    new_state.get_stream_metadata(&stream_id).tenant_id == command.tenant_id,
    "tenant mismatch: stream owned by {}, command from {} - isolation violation",
    new_state.get_stream_metadata(&stream_id).tenant_id,
    command.tenant_id
);
```

**Why**: Tenant isolation violation has compliance implications (HIPAA, GDPR). Must never allow cross-tenant access.

---

## Writing Effective Assertions

### Message Quality

**Good assertion messages**:
- State what failed (the invariant)
- Provide context (relevant values)
- Suggest possible causes

```rust
// EXCELLENT:
assert!(
    ciphertext.len() >= TAG_LENGTH,
    "ciphertext too short: {ciphertext_len} bytes, need at least {TAG_LENGTH} \
     - storage corruption or truncated write",
    ciphertext_len = ciphertext.len()
);

// GOOD:
assert!(
    new_view >= self.view,
    "view number regressed from {} to {}",
    self.view,
    new_view
);

// BAD:
assert!(ciphertext.len() >= TAG_LENGTH);

// TERRIBLE:
assert!(x);
```

### Assertion Density

**Target**: 2+ assertions per function (precondition + postcondition)

```rust
fn merge_log_tail(&mut self, entries: Vec<LogEntry>) -> &mut Self {
    // Precondition: entries are ordered
    for window in entries.windows(2) {
        assert!(
            window[0].op_number < window[1].op_number,
            "log entries not in ascending order - Byzantine attack detected"
        );
    }

    // ... merge logic ...

    // Postcondition: log remains ordered
    debug_assert!(self.log.windows(2).all(|w| w[0].op_number < w[1].op_number));

    self
}
```

**Pattern**: Assert at both write and read sites
- Write site: Production assertion (invariant enforcement)
- Read site: Debug assertion (invariant verification)

### Assertion Pairing

Write assertions in pairs at boundaries:

```rust
// Encryption side:
fn encrypt(plaintext: &[u8], key: &EncryptionKey) -> Vec<u8> {
    assert!(!key.0.iter().all(|&b| b == 0), "key is all zeros");

    let ciphertext = /* ... encryption ... */;

    assert!(
        ciphertext.len() >= plaintext.len() + TAG_LENGTH,
        "ciphertext missing auth tag"
    );
    ciphertext
}

// Decryption side:
fn decrypt(ciphertext: &[u8], key: &EncryptionKey) -> Result<Vec<u8>> {
    assert!(!key.0.iter().all(|&b| b == 0), "key is all zeros");
    assert!(
        ciphertext.len() >= TAG_LENGTH,
        "ciphertext too short: {ciphertext_len} bytes",
        ciphertext_len = ciphertext.len()
    );

    /* ... decryption ... */
}
```

---

## Testing Assertions

### Unit Tests with `#[should_panic]`

Every promoted assertion must have a corresponding test:

```rust
#[test]
#[should_panic(expected = "encryption key is all zeros")]
fn test_encryption_key_rejects_all_zeros() {
    let zero_key = EncryptionKey([0u8; KEY_LENGTH]);
    let plaintext = b"secret data";

    // Should panic due to all-zero key assertion
    encrypt(plaintext, &zero_key);
}

#[test]
#[should_panic(expected = "only leader can prepare")]
fn test_follower_cannot_prepare() {
    let mut follower = ReplicaState::new(ReplicaId(1), config);
    // Replica 1 is not leader in initial view

    // Should panic due to leader-only assertion
    follower.prepare(ClientRequest::new(/* ... */));
}

#[test]
#[should_panic(expected = "view number regressed")]
fn test_view_monotonicity_enforced() {
    let mut replica = ReplicaState::new(ReplicaId(0), config);
    replica.start_view_change(ViewNumber(5));

    // Should panic due to view regression
    replica.start_view_change(ViewNumber(3));
}
```

**Test file location**: `crates/kimberlite-crypto/src/tests_assertions.rs` contains all 38 tests.

### Property-Based Testing

Use proptest to verify assertions fire on invalid inputs:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    #[should_panic]
    fn prop_all_zero_keys_rejected(key in prop::collection::vec(0u8, KEY_LENGTH)) {
        // Generate all-zero key
        let zero_key = EncryptionKey(key.try_into().unwrap());
        encrypt(b"data", &zero_key);
    }

    #[test]
    #[should_panic]
    fn prop_view_regression_rejected(
        initial_view in 1u64..1000,
        regression in 1u64..100
    ) {
        let mut replica = ReplicaState::new(ReplicaId(0), config);
        replica.start_view_change(ViewNumber(initial_view));

        // Try to regress view
        let regressed_view = initial_view.saturating_sub(regression);
        replica.start_view_change(ViewNumber(regressed_view));
    }
}
```

Run with high iteration count:
```bash
PROPTEST_CASES=10000 cargo test --workspace
```

---

## Performance Considerations

### Measurement

Benchmark before and after assertion promotion:

```bash
# Before
just bench > results/before.txt

# After
just bench > results/after.txt

# Compare
diff results/before.txt results/after.txt
```

**Acceptance criteria** (from Phase 1 validation):
- Throughput regression < 1%
- p99 latency increase < 5μs
- p50 latency increase < 1μs

### Actual Impact

After promoting 38 assertions:
- **Throughput**: <0.1% regression (within noise)
- **p99 latency**: +1μs
- **p50 latency**: <1μs

**Conclusion**: Production assertions have negligible performance impact when properly written.

### Optimization Techniques

1. **Early Exit**: Check most likely failures first
   ```rust
   // Fast path: length check before iteration
   assert!(ciphertext.len() >= TAG_LENGTH);
   // Slow path: iterate only if needed
   assert!(!key.iter().all(|&b| b == 0));
   ```

2. **Const Evaluation**: Use const expressions when possible
   ```rust
   const MIN_CLUSTER_SIZE: usize = 3;
   assert!(replicas.len() >= MIN_CLUSTER_SIZE);
   ```

3. **Avoid Allocations**: Don't allocate in assertion messages
   ```rust
   // GOOD (no allocation):
   assert!(x > 0, "x must be positive: {x}");

   // BAD (allocates String):
   assert!(x > 0, format!("x must be positive: {}", x));
   ```

4. **Branch Prediction**: Assertions are cold branches (never taken in correct execution)
   - Modern CPUs predict not-taken by default
   - No penalty in happy path

---

## Incident Response

### When Production Assertions Fire

**Immediate Actions**:

1. **Isolate the Node**:
   - Remove from cluster immediately
   - Do NOT restart (preserves forensic state)
   - Prevent client connections

2. **Capture State Dump**:
   ```bash
   # Capture core dump
   gcore <pid>

   # Capture logs
   journalctl -u kimberlite > /forensics/kimberlite.log

   # Capture replica state
   curl http://localhost:8080/debug/state > /forensics/replica_state.json
   ```

3. **Triage by Category**:

**Cryptographic Assertions** (all-zero keys, key hierarchy violations):
- **Likely causes**: Storage corruption, RNG failure, memory corruption
- **Investigation**:
  - Check storage device SMART status: `smartctl -a /dev/sda`
  - Verify RNG entropy: `cat /proc/sys/kernel/random/entropy_avail`
  - Memory test: `memtester 1G 1`
  - Review storage write patterns (torn writes?)

**Consensus Assertions** (view monotonicity, commit ordering):
- **Likely causes**: Byzantine attack, logic bug, state corruption
- **Investigation**:
  - Analyze message logs for Byzantine patterns
  - Verify quorum agreement with other replicas
  - Check for clock skew: `chronyc tracking`
  - Review network partition history

**State Machine Assertions** (stream existence, offset monotonicity):
- **Likely causes**: Logic bug, concurrent modification, state corruption
- **Investigation**:
  - Dump kernel state to JSON
  - Check for race conditions in logs
  - Verify serialization/deserialization correctness
  - Review recent code changes

### Root Cause Analysis

Use the assertion message to guide investigation:

```rust
assertion failed: ciphertext too short: 8 bytes, need at least 16 - storage corruption or truncated write
```

**Investigation steps**:
1. Check disk space: `df -h`
2. Check filesystem errors: `dmesg | grep -i error`
3. Review storage layer logs for truncated writes
4. Verify write atomicity guarantees
5. Check backup integrity

### Prevention

After root cause identified:

1. **Add Test Case**: Reproduce the failure condition
2. **Add Monitoring**: Detect early warning signs (disk errors, low entropy)
3. **Add Graceful Degradation**: If possible, handle gracefully instead of panic
4. **Update Documentation**: Document the incident and resolution

---

## Examples by Crate

### kimberlite-crypto

**High value targets** (already promoted):
- All-zero detection (keys, hashes, nonces)
- Key hierarchy integrity
- Ciphertext format validation

**Still development-only** (keep as debug_assert):
- Internal helper function invariants
- Performance-critical checks in tight loops

### kimberlite-vsr

**High value targets** (already promoted):
- Leader-only operations
- View/commit monotonicity
- Quorum validation
- Cluster membership

**Still development-only** (keep as debug_assert):
- Internal state machine invariants after production checks
- Redundant checks after quorum validation

### kimberlite-kernel

**High value targets** (already promoted):
- Stream existence postconditions
- Effect count validation
- Offset monotonicity
- Tenant isolation

**Still development-only** (keep as debug_assert):
- Internal stream metadata consistency after validation
- Index invariants (covered by type system)

---

## FAQ

**Q: Why promote assertions instead of returning Result?**

A: Assertions are for invariants that MUST hold (bugs if violated). Results are for expected errors (invalid user input, network failures). If the error is recoverable, use Result. If it indicates a bug, use assertion.

**Q: What's the performance overhead?**

A: Negligible. Our 38 promoted assertions added <0.1% throughput regression and +1μs p99 latency. Assertions are cold branches (predicted not-taken) and compile to simple comparisons.

**Q: Should I panic in production?**

A: For invariant violations, YES. Panicking prevents corruption propagation. Better to crash one replica and recover from healthy peers than to propagate corrupted state across the cluster.

**Q: How do I test assertions?**

A: Every assertion needs a `#[should_panic]` unit test that triggers it. Use property-based testing to verify assertions fire on all invalid inputs. See `crates/kimberlite-crypto/src/tests_assertions.rs` for examples.

**Q: When should I use expect() instead of assert?**

A: `expect()` is for `Option`/`Result` unwrapping with context. Use it when you KNOW the value is `Some`/`Ok` due to prior checks:

```rust
let value = map.get(&key).expect("key must exist after insert");
```

Use `assert!()` for boolean conditions:

```rust
assert!(map.contains_key(&key), "key must exist after insert");
```

**Q: What about fuzzing?**

A: Fuzzing is complementary. Assertions catch bugs during development and in production. Fuzzing finds test cases that trigger assertions. Both are essential.

---

## Summary

**Production assertions are**:
- Executable documentation of invariants
- Early warning system for bugs and attacks
- Last line of defense against corruption propagation
- Negligible performance overhead (<0.1%)

**38 promoted assertions protect**:
- Cryptographic integrity (25 assertions)
- Consensus safety (9 assertions)
- State machine correctness (4 assertions)

**Every assertion must have**:
- Clear, informative message with context
- Corresponding `#[should_panic]` test
- Documented rationale in this guide

**When in doubt**:
- Ask: "If this fires in production, what does it mean?"
- If it means "bug or attack", use `assert!()`
- If it means "invalid input", use `Result`
- If it means "programming error", use `expect()`

---

**See also**:
- `website/content/blog/006-hardening-kimberlite-vsr.md` - Lessons learned from hardening
- `CLAUDE.md` - Project coding guidelines
- `docs/TESTING.md` - Testing strategy
- `docs/SECURITY.md` - Security practices
