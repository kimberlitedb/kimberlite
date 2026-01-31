# VOPR Overnight Testing Guide

This guide explains how to run VOPR overnight to find bugs in Kimberlite's core implementation.

## Quick Start

### 1. Build VOPR (Release Mode)

```bash
just build-release
```

### 2. Run a Quick Test First

Before running overnight, verify everything works:

```bash
# Run 1000 iterations (~1-2 minutes)
./scripts/vopr-quick.sh 1000
```

If this completes successfully, you're ready for overnight testing.

### 3. Run Overnight Test

```bash
# Make script executable
chmod +x ./scripts/vopr-overnight.sh

# Run overnight test (100,000 iterations = 8-12 hours)
./scripts/vopr-overnight.sh
```

### 4. Check Results in the Morning

Results are saved to `./vopr-results/YYYYMMDD-HHMMSS/`:

```bash
# View summary
cat ./vopr-results/*/summary.txt

# View failures (if any)
cat ./vopr-results/*/failures.log
```

## Advanced Usage

### Custom Configuration

Set environment variables before running:

```bash
# Run 500k iterations (~40 hours)
VOPR_ITERATIONS=500000 ./scripts/vopr-overnight.sh

# Use a specific seed for reproducibility
VOPR_SEED=12345 ./scripts/vopr-overnight.sh

# Custom output directory
VOPR_OUTPUT_DIR=/var/log/vopr ./scripts/vopr-overnight.sh

# Combine options
VOPR_ITERATIONS=200000 \
VOPR_SEED=42 \
VOPR_OUTPUT_DIR=./my-test \
./scripts/vopr-overnight.sh
```

### Resume After Interruption

The overnight script saves checkpoints automatically. If interrupted (Ctrl+C or system reboot), just re-run:

```bash
# The script will resume from the last checkpoint
./scripts/vopr-overnight.sh
```

### Run in Background (tmux/screen)

For long-running tests, use tmux or screen:

```bash
# Start tmux session
tmux new -s vopr

# Run test
./scripts/vopr-overnight.sh

# Detach: Ctrl+B, then D
# Reattach later: tmux attach -t vopr
```

Or run in background with nohup:

```bash
nohup ./scripts/vopr-overnight.sh > vopr.out 2>&1 &

# Check progress
tail -f vopr.out
```

## Manual VOPR Usage

### Basic Commands

```bash
# Run with specific seed
./target/release/vopr --seed 12345

# Run many iterations
./target/release/vopr -n 10000 -v

# Enable/disable fault types
./target/release/vopr --faults network     # Only network faults
./target/release/vopr --faults storage     # Only storage faults
./target/release/vopr --no-faults          # No faults (baseline)

# Check determinism
./target/release/vopr --check-determinism -n 1000

# Enable full trace (high overhead, debugging only)
./target/release/vopr --enable-trace --seed 42 -v
```

### Checkpointing

```bash
# Save checkpoint to file
./target/release/vopr -n 100000 --checkpoint-file checkpoint.json -v

# Resume from checkpoint (if interrupted)
./target/release/vopr -n 100000 --checkpoint-file checkpoint.json -v
```

### JSON Output

```bash
# Get machine-readable output
./target/release/vopr --json -n 1000 > results.ndjson

# Parse with jq
cat results.ndjson | jq 'select(.status == "failed")'
```

## Understanding Results

### Success

If no failures are found, you'll see:

```
╔════════════════════════════════════════════════════════════════╗
║                    VOPR Test Summary                            ║
╚════════════════════════════════════════════════════════════════╝

Results:
--------
Successes:          100000
Failures:           0
Success Rate:       100.00%

✅ All tests passed!
```

### Failure

If a bug is found, you'll see:

```
╔════════════════════════════════════════════════════════════════╗
║                    VOPR Test Summary                            ║
╚════════════════════════════════════════════════════════════════╝

Results:
--------
Successes:          98523
Failures:           3
Success Rate:       99.997%

⚠️  FAILURES DETECTED! See ./vopr-results/.../failures.log for details.
```

The `failures.log` will contain full failure reports with:
- **Seed**: To reproduce the exact failure
- **Invariant violated**: Which correctness property failed
- **Classification**: Type of failure (DataCorruption, ReplicaDivergence, etc.)
- **Diagnosis**: Suggested root causes
- **Reproduction commands**: Exact command to re-run

### Failure Report Example

```
═══════════════════════════════════════════════════════════════
           SIMULATION FAILURE REPORT
═══════════════════════════════════════════════════════════════

Seed: 42
Invariant: model_verification
Message: read mismatch: key=5, expected=Some(100), actual=Some(200)
Time: 5000000000ns (5000ms)
Event: 150 / 200

───────────────────────────────────────────────────────────────
Classification
───────────────────────────────────────────────────────────────
Type: DataCorruption
Description: Data read does not match expected value (model mismatch)

───────────────────────────────────────────────────────────────
Diagnosis
───────────────────────────────────────────────────────────────
Possible Root Causes:
  1. Storage corruption not detected
  2. Write/read race condition
  3. Partial write accepted as complete
  4. Incorrect model state tracking

───────────────────────────────────────────────────────────────
Reproduction
───────────────────────────────────────────────────────────────
Seed: 42
Min events: 200

Commands:
  vopr --seed 42 -v
  vopr --seed 42 --max-events 200
```

## Reproducing Failures

To reproduce a failure:

```bash
# Use the seed from the failure report
./target/release/vopr --seed 42 -v

# For more verbose output
./target/release/vopr --seed 42 -v --enable-trace
```

## Performance Expectations

Rough estimates (varies by hardware):

| Iterations | Time      | Use Case              |
|------------|-----------|-----------------------|
| 100        | ~10s      | Quick smoke test      |
| 1,000      | ~1-2 min  | Pre-commit check      |
| 10,000     | ~10-20 min| Integration test      |
| 100,000    | ~8-12 hrs | Overnight test        |
| 500,000    | ~40 hrs   | Weekend test          |
| 1,000,000  | ~80 hrs   | Extended stress test  |

## What VOPR Tests

VOPR currently validates:

### TIER 1 & 2 (Implemented)
- ✅ **Hash Chain Integrity**: Offset monotonicity, linkage, genesis
- ✅ **Log Consistency**: Committed records match re-reads
- ✅ **Linearizability**: Single-key Wing-Gong linearizability
- ✅ **Replica Consistency**: Same log position = same hash
- ✅ **Replica Head Progress**: View/op never regress
- ✅ **Commit History**: No gaps, monotonic operation numbers
- ✅ **Model Verification**: Data reads match expected values
- ✅ **Checkpoint/Recovery**: State survives checkpoint cycles

### Fault Injection
- ✅ **Network**: Delays, drops, partitions, message reordering
- ✅ **Storage**: Write failures, read corruption, fsync failures, partial writes

### TIER 3 (Implemented)
- ✅ **Enhanced Workloads**: Read-Modify-Write, Scan operations
- ✅ **Trace Collection**: Full event history for debugging
- ✅ **Failure Diagnosis**: Automated root cause analysis

## Tips

1. **Start Small**: Run 1000 iterations first to verify setup
2. **Use tmux/screen**: For overnight tests, run in a persistent session
3. **Check Disk Space**: Logs can grow large with verbose mode
4. **Monitor Progress**: Use `tail -f` to watch the log file
5. **Save Failure Seeds**: Document any failures for future regression tests

## Troubleshooting

### "VOPR binary not found"

```bash
just build-release
```

### "Permission denied"

```bash
chmod +x ./scripts/vopr-overnight.sh
chmod +x ./scripts/vopr-quick.sh
```

### Test runs too slow

- Disable verbose mode (remove `-v`)
- Reduce max events: `--max-events 5000`
- Disable enhanced workloads: `--no-enhanced-workloads`

### Out of memory

- Disable trace collection: `--no-trace-on-failure`
- Reduce iterations per run, use checkpointing

## Next Steps After Finding a Bug

1. **Reproduce**: Use the seed from the failure report
2. **Isolate**: Run with `--enable-trace` for full event history
3. **Debug**: Use the failure diagnosis to understand root cause
4. **Fix**: Implement fix in the relevant crate
5. **Verify**: Re-run with same seed to confirm fix
6. **Regression Test**: Add seed to automated test suite

## Questions?

See `./target/release/vopr --help` for all options.
