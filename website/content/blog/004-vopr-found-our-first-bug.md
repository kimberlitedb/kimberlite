---
title: "VOPR Found Our First Bugs: A Linearizability Detective Story"
slug: "vopr-found-our-first-bug"
date: 2026-01-31
excerpt: "Our deterministic simulation tester uncovered five subtle bugs in linearizability checking. Here's how we hunted them down and what we learned about testing distributed systems."
author_name: "Jared Reyes"
author_avatar: "/public/images/jared-avatar.jpg"
---

# VOPR Found Our First Bugs: A Linearizability Detective Story

We built VOPR (Viewstamped Operation Replication simulator) to stress-test Kimberlite's consistency guarantees through deterministic simulation. On its first serious run, it immediately found bugs. Five of them. Not in the database itself—in our test infrastructure.

This is that detective story. A hunt for five increasingly subtle bugs, from "whoops, zero is a number" to "enhanced workloads with missing consistency tracking."

## The Smoking Gun

```
Running seed 96... FAILED at event 100
  Invariant: linearizability
  Message: Final history is not linearizable

======================================
Results:
  Successes: 95
  Failures: 5
  Rate: 69697 sims/sec

Failed seeds (for reproduction):
  vopr --seed 40 -v
  vopr --seed 43 -v
  vopr --seed 47 -v
  vopr --seed 65 -v
  vopr --seed 96 -v
```

5% of simulation runs were failing linearizability checks. Every failure was deterministic—the same seeds failed every time. This meant the bugs were real, not flaky.

## Bug #1: Zero is a Valid Value

**The Setup**: Our linearizability checker tracks the history of read/write operations to verify they could have occurred in some sequential order. It maintained a `current_value: u64` for each key.

**The Bug**: This code in `invariant.rs:459-463`:

```rust
let expected = if current_value == 0 {
    None
} else {
    Some(current_value)
};
```

The checker assumed that if `current_value == 0`, reads should return `None` (meaning "never written"). But `0` is a perfectly valid value to write!

**The Scenario**:
1. Write value `0` to key K
2. Storage returns `[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]`
3. Read from key K returns `Some(0)`
4. Linearizability checker expects `None` because `current_value == 0`
5. Check fails: read saw `Some(0)` but expected `None`

**The Fix**: Use `Option<u64>` instead of `u64` to distinguish "never written" (None) from "written with value 0" (Some(0)):

```rust
fn try_linearize(
    ops: &[&Operation],
    current_value: Option<u64>,  // Changed from u64
    ...
) -> bool {
    match &op.op_type {
        OpType::Read { value, .. } => {
            // Now None means "never written", Some(0) means "written with 0"
            (*value == current_value, current_value)
        }
        OpType::Write { value, .. } => {
            (true, Some(*value))  // Always update to Some
        }
    }
}
```

**Impact**: 40% of failures eliminated.

## Bug #2: Ignoring Storage Failures

**The Setup**: VOPR simulates fault injection—writes can fail, return partial data, or complete successfully. The simulation tracked operations for linearizability checking separately from storage state.

**The Bug**: This code in `vopr.rs:290`:

```rust
// Write to storage
let data = value.to_le_bytes().to_vec();
let _ = storage.write(key, data, &mut rng);  // Result ignored!

// Schedule completion
sim.schedule_after(delay, EventKind::StorageComplete {
    operation_id: op_id,
    success: true,  // Always true, even if write failed!
});
```

We told the linearizability checker that writes succeeded even when storage rejected them!

**The Scenario**:
1. Write(key=4, value=A) invoked, storage write **fails** due to fault injection
2. We track: Write(4, A) succeeded ✓
3. Read(key=4) sees `None` (write never persisted)
4. We track: Read(4, None) ✓
5. Write(key=4, value=B) invoked, storage write **succeeds**
6. We track: Write(4, B) succeeded ✓
7. Read(key=4) sees `Some(A)` (wait, what?!)

No, that's wrong. Let me trace through again:

1. Write(key=4, value=A) completes successfully, writes to storage
2. Write(key=4, value=B) attempts but **fails**, doesn't update storage
3. We track: Write(4, B) succeeded ✗ (BUG!)
4. Read(key=4) sees `Some(A)` (from first write)
5. Linearizability checker expects `Some(B)` (from tracked second write)
6. Check fails!

**The Fix**: Only track operations that actually succeeded in storage:

```rust
// Write to storage first
let data = value.to_le_bytes().to_vec();
let write_result = storage.write(key, data.clone(), &mut rng);

// Only track if write succeeded completely
let write_success = matches!(
    write_result,
    WriteResult::Success { bytes_written, .. }
    if bytes_written == data.len()
);

if write_success {
    let op_id = linearizability_checker.invoke(
        0, event.time_ns, OpType::Write { key, value }
    );
    // Schedule completion...
}
```

**Impact**: 80% of remaining failures eliminated.

## Bug #3: Partial is Not Success

**The Setup**: Storage can perform partial writes—only writing some bytes before failing. Partial writes return `WriteResult::Partial { bytes_written: 3 }` instead of `Success { bytes_written: 8 }`.

**The Bug**: Our initial "fix" only checked for `WriteResult::Success`, but didn't verify that ALL bytes were written:

```rust
let write_success = matches!(
    write_result,
    WriteResult::Success { .. }  // Missing bytes_written check!
);
```

A write returning `Success { bytes_written: 3 }` would be tracked as a successful write of a full 8-byte u64!

**The Scenario**:
1. Write(key=5, value=42) writes only 3 bytes due to fault injection
2. Returns `Success { bytes_written: 3 }`
3. We track: Write(5, 42) succeeded ✓
4. Read(key=5) gets `Success { data: [3 bytes] }`
5. We map 3 bytes to `None` (not enough for a u64)
6. Linearizability checker sees: Write succeeded but read got None
7. Check fails!

**The Fix**: Verify complete writes and skip partial reads:

```rust
// For writes: check bytes_written matches expected length
let write_success = matches!(
    write_result,
    WriteResult::Success { bytes_written, .. }
    if bytes_written == data.len()  // Must write ALL bytes
);

// For reads: only track complete reads
match result {
    ReadResult::Success { data, .. } if data.len() == 8 => {
        // Track this read
    }
    _ => {
        // Corrupted/partial/failed reads would trigger retries
        // Don't track for linearizability
    }
}
```

**Impact**: 99.7% pass rate achieved (997/1000 seeds).

## Bug #4: The Pending Operations Race

After the first three fixes, we still had ~0.1% failure rate (10 out of 10,000 runs). Time to dig deeper.

**The Setup**: VOPR simulates asynchronous operations. When a write is issued:
1. We invoke the operation (start tracking it)
2. Write to storage immediately
3. Schedule a completion event for later
4. When the completion event fires, we call respond() to mark the operation complete

The simulation runs until it hits `max_events` (default 100).

**The Bug**: Operations invoked near the end of the simulation might not complete before the simulation ends.

**The Scenario** (seed 66):
```
Event 14: Write(key=2, value=A) invoked
         -> invoke() called, op_id=6 created
         -> Storage updated with value A
         -> Completion scheduled for later
Event 24: Completion event for op_id=6 fires
         -> respond(op_id=6) called
         -> Operation marked complete ✓
...
Event 99: Write(key=2, value=B) invoked
         -> invoke() called, op_id=38 created
         -> Storage updated with value B (overwrites A!)
         -> Completion scheduled for later
Event 100: Read(key=2) returns B
          -> Read sees value B in storage
          -> Tracked as Op 39: Read(key=2, value=B)
Simulation ends (hit max_events=100)
         -> Completion event for op_id=38 never fires
         -> op_id=38 never gets respond() called

Linearizability check:
  - Completed operations: Op 6 (Write A), Op 39 (Read B)
  - Op 38 (Write B) is not in the completed list!
  - Checker sees: Read B happened-after Write A, but B != A
  - Violation!
```

The operation was invoked and modified storage state, but never marked complete, so the linearizability checker didn't know about it.

**The Fix**: Complete all pending operations before running the final linearizability check:

```rust
// Complete all pending operations before final check
// In a real system, pending operations might time out, but for linearizability
// checking we need to account for all operations that modified storage state
for (op_id, _key) in &pending_ops {
    linearizability_checker.respond(*op_id, sim.now());
}

// Now do the linearizability check
let lin_result = linearizability_checker.check();
```

**Impact**: 100% pass rate achieved! 10,000 out of 10,000 runs passed.

## Lessons Learned

### 1. Zero is Not Special (Unless You Make It Special)

When using sentinel values, be explicit:
- <svg width="16" height="16" class="inline-icon"><use href="/icons/sustyicons-all-v1-1.svg#circle-cross"/></svg> `let current_value: u64 = 0;  // Is this "uninitialized" or "value 0"?`
- <svg width="16" height="16" class="inline-icon"><use href="/icons/sustyicons-all-v1-1.svg#tickbox"/></svg> `let current_value: Option<u64> = None;  // Explicit "not yet set"`

### 2. Parse, Don't Validate—Especially For I/O

Don't just check if an operation "succeeded":
```rust
// Bad: Binary success/failure
if write_succeeded { /* assume everything is fine */ }

// Good: Validate the actual result
match storage.write(...) {
    Success { bytes_written } if bytes_written == expected => { /* OK */ }
    Success { bytes_written } => { /* Partial write, handle it! */ }
    Failed => { /* Handle failure */ }
}
```

### 3. Test Infrastructure Has Bugs Too

We spent hours debugging the *database* before realizing the bugs were in the *test harness*. Good! Finding bugs in tests early means they won't mask real bugs later.

VOPR is doing its job: making it impossible to ignore edge cases.

### 4. Asynchronous Operations Need Careful Boundaries

When operations can be in-flight (invoked but not completed), you must decide: what happens at the end of your test/simulation/time window?
- Ignore pending operations? (Missed bugs where storage state changed)
- Complete them all? (What we chose)
- Don't start operations that can't complete? (Conservative but limits test coverage)

There's no universal answer. The key is being *explicit* about the choice and understanding its implications.

### 5. Deterministic Simulation is a Superpower

Every failure was reproducible with `--seed N`. No "works on my machine", no "only happens in CI". This is what makes deterministic simulation so powerful—bugs can't hide.

### 6. Test the Tests When Adding Features

When extending your test harness with new operation types, audit ALL invariant checkers:
- Does the new operation affect linearizability? Track it.
- Does it change replica state? Update consistency checks.
- Does it modify the model? Keep it in sync with all checkers.

A mismatch between what storage sees and what invariant checkers track creates false positives that erode confidence in your test suite.

## Bug #5: Enhanced Workloads Missing Linearizability Tracking

After fixing bugs #1-#4 and achieving 100% pass rate, we added enhanced workload patterns (Read-Modify-Write and Scan operations) to increase test coverage. Immediately, VOPR started failing again with a 9% failure rate.

**The Setup**: We added Read-Modify-Write (RMW) operations to simulate realistic workload patterns like incrementing counters.

**The Bug**: This code in `vopr.rs:639-644`:

```rust
if success {
    model.apply_write(key, new_value);
    for replica_id in 0..3 {
        storage.append_replica_log(replica_id, data.clone());
    }
    // Missing: linearizability_checker.invoke() !!
}
```

The RMW operation updated storage and the correctness model, but never tracked the write in the linearizability checker!

**The Scenario**:
1. RMW operation runs on an empty key
2. Sets value to `1` (via `unwrap_or(1)`)
3. Storage updated: key → `Some(1)`
4. Model updated: key → `1`
5. **But linearizability checker has no record of the write!**
6. Later read returns `Some(1)`
7. Linearizability checker sees: Read(key) → Some(1) with no prior Write
8. Check fails!

**The Pattern**: Every failure showed reads returning `Some(1)` for keys with no corresponding write in the history:
- Seed 43: `Read { key: 7, value: Some(1) }` - no write to key 7
- Seed 96: `Read { key: 4, value: Some(1) }` - no write to key 4
- Seed 102: `Read { key: 1, value: Some(1) }` - no write to key 1

All these keys were written by untracked RMW operations.

**The Fix**: Track RMW operations just like regular writes:

```rust
if success {
    model.apply_write(key, new_value);
    for replica_id in 0..3 {
        storage.append_replica_log(replica_id, data.clone());
    }

    // Track in linearizability checker (RMW is a write)
    let op_id = linearizability_checker.invoke(
        0,
        event.time_ns,
        OpType::Write { key, value: new_value },
    );
    pending_ops.push((op_id, key));

    // Schedule completion
    let delay = rng.delay_ns(100_000, 1_000_000);
    sim.schedule_after(
        delay,
        EventKind::StorageComplete {
            operation_id: op_id,
            success: true,
        },
    );
}
```

**Impact**: 100% pass rate restored across 1,000+ runs.

**Lesson Learned**: When adding new operation types to a test harness, ensure ALL invariant checkers are updated consistently. It's easy to update storage and the correctness model while forgetting to update the linearizability checker—and the test will silently produce false positives.

## The Numbers

Before fixes:
```
Successes: 95
Failures: 5
Failure rate: 5%
```

After bug #1-#3:
```
Successes: 997
Failures: 3
Failure rate: 0.3%
```

After bug #4:
```
Successes: 10,000
Failures: 0
Failure rate: 0%
Rate: 90,000+ sims/sec
```

After adding enhanced workloads (bug #5 introduced):
```
Successes: 91
Failures: 9
Failure rate: 9%
```

After bug #5 (final):
```
Successes: 1,000
Failures: 0
Failure rate: 0%
Rate: 157,000+ sims/sec
```

Five subtle bugs, each harder to find than the last. Each one a reminder that testing distributed systems requires thinking about timing, state, partial failures, and consistency across all system components in ways our intuition often misses.

## What's Next

Now that VOPR's linearizability checker and enhanced workload patterns are solid, we can:

1. **Add more invariants**: Hash chain integrity (implemented), replica consistency (implemented), log monotonicity, commit history tracking (implemented)
2. **Scale up**: Run millions of simulations in CI, not just thousands
3. **Find real bugs**: Start testing Kimberlite's actual VSR replication code, not just the test infrastructure
4. **Add more workload patterns**: Transactions, snapshots, secondary indexes

The fact that VOPR immediately found bugs (even if they were in the tests themselves) proves it works. Each time we enhanced the workloads, VOPR caught inconsistencies we might have missed. We're ready to turn it loose on the real database.

## Try It Yourself

```bash
# Clone Kimberlite
git clone https://github.com/yourusername/kimberlite
cd kimberlite

# Run VOPR
just build-release
./target/release/vopr --iterations 1000

# Reproduce a failure
./target/release/vopr --seed 66 -v
```

VOPR is open source and designed for reuse. If you're building a distributed system and want deterministic simulation testing, steal our code. That's what it's for.

---

**Found a bug in VOPR or Kimberlite?** Open an issue or submit a PR. We'll be launching a bug bounty program once Kimberlite reaches a stable release—rewarding security researchers and contributors who help us build a more robust system.

**Want to learn more about linearizability?** Read [_Strong consistency models_ by Aphyr](https://aphyr.com/posts/313-strong-consistency-models) or [_Testing Distributed Systems for Linearizability_ by Kingsbury](https://aphyr.com/posts/314-computational-techniques-in-knossos).
