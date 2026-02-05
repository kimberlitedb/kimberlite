# VOPR Debugging Guide

Internal guide for debugging VOPR simulation failures and understanding failure patterns.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Model Verification Failures](#model-verification-failures)
3. [Common Failure Patterns](#common-failure-patterns)
4. [Reproduction Workflow](#reproduction-workflow)
5. [Understanding Traces](#understanding-traces)

## Quick Start

When VOPR reports failures:

1. **Note the seed**: Each failure includes a seed for reproduction
2. **Run with verbose**: `vopr --seed <N> -v` shows detailed event log
3. **Check the pattern**: Classify the failure (see Common Failure Patterns)
4. **Reproduce minimally**: Use the seed to reproduce deterministically
5. **Investigate**: Follow the specific troubleshooting steps

## Model Verification Failures

If you encounter "model_verification: read mismatch" failures, these indicate discrepancies between expected model state and actual storage state.

### Troubleshooting Steps

1. **Check fsync handling**: Ensure `model.pending` is cleared on fsync failure
   - Look for: `Fsync failed at Xms, clearing pending writes from model`
   - If missing, the model may have stale pending writes

2. **Check write reorderer**: Verify `get_pending_write()` returns correct data for read-your-writes
   - Writes should be readable immediately after submission, even if in reorderer queue
   - Check that read order is: `pending_writes` → `reorderer` → `blocks`

3. **Check checkpoint recovery**: Verify model state is synchronized after `RecoverCheckpoint` event
   - Both `model.pending` and `model.durable` should match checkpoint state
   - Look for: `Restored checkpoint X at Yms (Z blocks)`

4. **Reproduce with verbose**: `vopr --seed <N> -v` to see detailed event log
   - Trace the sequence of writes, fsyncs, and reads leading to the mismatch
   - Look for fsync failures, checkpoint restores, or crash events before the read

### Common Patterns

#### Pattern: `expected=Some(x), actual=None`

**Meaning**: Model expects a value but storage has nothing.

**Likely Causes**:
- Fsync failure not clearing model.pending
- Checkpoint restored but model.durable not synchronized
- Write was lost due to crash but model wasn't updated

**Debug Steps**:
1. Search verbose output for the key
2. Find the last write to this key
3. Check if fsync succeeded or failed
4. Verify model was updated correctly

#### Pattern: `expected=Some(x), actual=Some(y)` where x ≠ y

**Meaning**: Model expects one value but storage has a different value.

**Likely Causes**:
- Write reordering caused older value to overwrite newer value
- Read returned stale value from wrong layer (reorderer/pending/blocks)
- Corruption (bit flip) during read

**Debug Steps**:
1. Check if values differ by 1 (likely reordering issue)
2. Check if values are completely different (likely corruption)
3. Verify read order in storage.rs is correct
4. Check for read corruption events in verbose output

#### Pattern: `expected=Some(x), actual=Some(x+1)` (off-by-one)

**Meaning**: Model has an older value than storage.

**Likely Causes**:
- Multiple writes to same key, model has stale value
- Checkpoint recovery rebuilt model incorrectly
- Race condition in model update

**Debug Steps**:
1. Find all writes to this key in verbose output
2. Verify each write updated the model
3. Check for checkpoint restores that may have reset model

### Verification Logic

The model verification in VOPR follows this logic:

```rust
fn verify_read(&self, key: u64, actual: Option<u64>) -> bool {
    let expected = self.pending.get(&key).or_else(|| self.durable.get(&key));
    match (expected, actual) {
        (Some(expected), Some(actual)) => expected == &actual,
        (None, None) => true,
        (None, Some(_)) => true, // Acceptable after checkpoint recovery
        (Some(_), None) => false, // Data expected but missing - ALWAYS a bug
    }
}
```

**Key Points**:
- Pending writes take precedence over durable writes (read-your-writes)
- Missing data when model expects it is always a bug
- Extra data when model doesn't expect it is acceptable (checkpoint recovery scenario)

## Common Failure Patterns

### Byzantine Attacks

**Symptoms**: `byzantine_attack: leader equivocation detected`

**Meaning**: The protocol-level attack detection caught a malicious leader sending conflicting Prepare messages.

**Expected**: This is GOOD - it means the detection is working. Byzantine attacks should be caught, not cause silent corruption.

### Storage Corruption

**Symptoms**: `storage_corruption: checksum mismatch`

**Meaning**: A corrupted block was detected via CRC32 checksum.

**Expected**: Corruption should be detected by checksums and trigger PAR protocol.

### Invariant Violations

**Symptoms**: `replica_consistency: replicas have diverged`

**Meaning**: Replicas in the same view committed different values at the same offset.

**CRITICAL**: This indicates a consensus safety bug. Investigate immediately:
1. Reproduce with `--seed <N> -v`
2. Extract full trace with event log
3. Check if it's a simulation bug or protocol bug
4. File issue with minimal reproduction case

## Reproduction Workflow

1. **Capture the seed**: VOPR prints `vopr --seed X -v` for each failure

2. **Reproduce locally**:
   ```bash
   vopr --seed 12345 -v
   ```

3. **Extract detailed trace**:
   ```bash
   vopr --seed 12345 -v 2>&1 | tee failure-trace.log
   ```

4. **Analyze events**:
   - Search for the failing key: `grep "key=291" failure-trace.log`
   - Find all writes: `grep "Write.*key=291" failure-trace.log`
   - Find all reads: `grep "Read.*key=291" failure-trace.log`
   - Find fsyncs: `grep "Fsync" failure-trace.log`

5. **Minimize**:
   - Reduce `--max-events` to find minimum events needed
   - Disable fault types one by one to isolate cause
   - Simplify scenario if possible

## Understanding Traces

### Event Types

- `Write operation: key=X, value=Y` - Write submitted
- `Fsync completed at Xms` - Fsync succeeded, pending → durable
- `Fsync failed at Xms, clearing pending writes` - Fsync failed, pending lost
- `Restored checkpoint N at Xms` - Checkpoint recovery, model synchronized
- `Read operation: key=X, value=Y` - Read completed
- `model_verification: read mismatch` - Verification failed

### Timeline Reconstruction

To understand a failure, reconstruct the timeline:

1. List all events for the failing key chronologically
2. Track model state after each event:
   - Write → model.pending updated
   - Fsync success → model.pending → model.durable
   - Fsync failure → model.pending cleared
   - Checkpoint restore → model.durable rebuilt
3. Compare expected model state at read time with actual storage state
4. Identify the divergence point

### Example Timeline

```
T=0ms:    Write(key=10, value=1) → model.pending[10] = 1
T=10ms:   Fsync succeeded → model.durable[10] = 1
T=20ms:   Write(key=10, value=2) → model.pending[10] = 2
T=30ms:   Fsync FAILED → model.pending cleared
T=40ms:   Read(key=10) → storage returns 1, model expects... ?

Analysis:
- model.pending cleared at T=30ms
- model.durable still has 1 from T=10ms
- Read should return 1, model expects 1
- ✓ No mismatch - correct behavior
```

## Tips

1. **Use verbose mode liberally**: Overhead is minimal, insight is invaluable
2. **Trust the seeds**: Same seed = same execution, always
3. **Check recent changes**: If failures spike after a commit, bisect to find the breaking change
4. **Test with reduced faults**: Disable fault types one by one to isolate root cause
5. **Compare with passing seeds**: Run a passing seed with `-v` to see correct behavior

## Getting Help

If stuck:
1. Gather minimal reproduction case with seed
2. Extract full trace with `-v`
3. Document expected vs actual behavior
4. Post in #vopr-debugging channel with all context
5. Include VOPR version and commit SHA
