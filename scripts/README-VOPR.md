# VOPR Overnight Testing Scripts

## Overview

Two scripts are available for overnight VOPR testing:

1. **`vopr-overnight.sh`** - Run a single scenario with many iterations
2. **`vopr-overnight-all.sh`** - Run all 27 scenarios sequentially

## Quick Start

### Single Scenario Testing

```bash
# Run combined scenario (recommended for overnight)
./scripts/vopr-overnight.sh

# Run specific scenario
VOPR_SCENARIO=baseline ./scripts/vopr-overnight.sh

# Custom iterations (default: 10M)
VOPR_ITERATIONS=50000000 ./scripts/vopr-overnight.sh
```

### Comprehensive Testing (All Scenarios)

```bash
# Run all 27 scenarios with 1M iterations each
./scripts/vopr-overnight-all.sh

# Custom iterations per scenario
VOPR_ITERATIONS=5000000 ./scripts/vopr-overnight-all.sh

# With custom output directory
VOPR_OUTPUT_DIR=/var/vopr/results ./scripts/vopr-overnight-all.sh
```

## Available Scenarios (27 Total)

### Core Scenarios (6)
- `baseline` - No faults, baseline performance
- `swizzle` - Intermittent network congestion
- `gray` - Partial node failures
- `multi-tenant` - Multi-tenant isolation with faults
- `time-compression` - 10x accelerated time
- `combined` - All fault types enabled (recommended)

### Byzantine Attack Scenarios (11)
- `view-change-merge` - View change log merge overwrites
- `commit-desync` - Commit number desynchronization
- `inflated-commit` - Inflated commit number in DoViewChange
- `invalid-metadata` - Invalid entry metadata
- `malicious-view-change` - Malicious view change selection
- `leader-race` - Leader selection race condition
- `dvc-tail-mismatch` - DoViewChange log_tail length mismatch
- `dvc-identical-claims` - DoViewChange with identical claims
- `oversized-start-view` - Oversized StartView log_tail (DoS)
- `invalid-repair-range` - Invalid repair range
- `invalid-kernel-command` - Invalid kernel command

### Corruption Detection Scenarios (3)
- `bit-flip` - Random bit flip in log entry
- `checksum-validation` - Checksum validation test
- `silent-disk-failure` - Silent disk corruption

### Crash & Recovery Scenarios (3)
- `crash-commit` - Crash during commit application
- `crash-view-change` - Crash during view change
- `recovery-corrupt` - Recovery with corrupt log

### Gray Failure Variants (2)
- `slow-disk` - Slow disk I/O
- `intermittent-network` - Intermittent network

### Race Condition Scenarios (2)
- `race-view-changes` - Concurrent view changes
- `race-commit-dvc` - Commit during DoViewChange

## Environment Variables

### Common Configuration

```bash
# Scenario to run (vopr-overnight.sh only)
VOPR_SCENARIO=combined           # Default: combined

# Number of iterations
VOPR_ITERATIONS=10000000         # Default: 10M (single), 1M (all)

# Starting seed
VOPR_SEED=12345                  # Default: $(date +%s)

# Output directory
VOPR_OUTPUT_DIR=./results        # Default: ./vopr-results/TIMESTAMP

# VOPR binary path
VOPR_BIN=./target/release/vopr   # Default: ./target/release/vopr

# Additional VOPR options
VOPR_OPTS="--verbose"            # Default: empty
```

### Invariant Control

```bash
# Core invariants only
VOPR_CORE_ONLY=1

# Specific invariants
VOPR_INVARIANTS=vsr_agreement,vsr_durability

# Group toggles (1=enabled, 0=disabled)
VOPR_ENABLE_VSR_INVARIANTS=1           # Default: 1
VOPR_ENABLE_PROJECTION_INVARIANTS=1    # Default: 1
VOPR_ENABLE_QUERY_INVARIANTS=1         # Default: 1
VOPR_ENABLE_SQL_ORACLES=0              # Default: 0 (very slow!)
```

## Performance Estimates

Based on M1 MacBook, release build:

### Single Scenario (vopr-overnight.sh)
- **Baseline**: ~167k sims/sec → 10M takes ~60s
- **Combined**: ~85k sims/sec → 10M takes ~118s
- **8-hour overnight**: 2.4 billion iterations possible

### All Scenarios (vopr-overnight-all.sh)
- **27 scenarios × 1M iterations** = 27M total iterations
- **Combined average**: ~85k sims/sec
- **Estimated time**: ~5-6 hours for full suite

## Examples

### Quick Smoke Test (All Scenarios)

```bash
# Run all scenarios with 10k iterations each (~5 minutes total)
VOPR_ITERATIONS=10000 ./scripts/vopr-overnight-all.sh
```

### Overnight Full Test (Single Scenario)

```bash
# Run combined scenario overnight (~8 hours)
VOPR_SCENARIO=combined \
VOPR_ITERATIONS=2400000000 \
./scripts/vopr-overnight.sh
```

### Comprehensive Suite (Weekend Run)

```bash
# Run all scenarios with 10M iterations each (~24 hours)
VOPR_ITERATIONS=10000000 ./scripts/vopr-overnight-all.sh
```

### Byzantine Attack Testing

```bash
# Test only Byzantine scenarios
for scenario in view-change-merge commit-desync inflated-commit \
                invalid-metadata malicious-view-change leader-race; do
    VOPR_SCENARIO=$scenario \
    VOPR_ITERATIONS=5000000 \
    ./scripts/vopr-overnight.sh
done
```

### Core Invariants Only (Fast)

```bash
# Run with only 6 core invariants (much faster)
VOPR_CORE_ONLY=1 \
VOPR_ITERATIONS=50000000 \
./scripts/vopr-overnight-all.sh
```

## Output Structure

### Single Scenario (`vopr-overnight.sh`)

```
vopr-results/20260202-143022/
├── config.txt              # Test configuration
├── vopr.log               # Full output log
├── failures.log           # Failure details (if any)
├── checkpoint.json        # Resume checkpoint
└── summary.txt            # Final summary
```

### All Scenarios (`vopr-overnight-all.sh`)

```
vopr-results/comprehensive-20260202-143022/
├── config.txt                      # Master configuration
├── SUMMARY.txt                     # Overall summary
├── baseline/
│   ├── vopr.log
│   └── checkpoint.json
├── swizzle/
│   ├── vopr.log
│   └── checkpoint.json
├── ...
└── race-commit-dvc/
    ├── vopr.log
    └── checkpoint.json
```

## Resuming Interrupted Tests

Both scripts support checkpoint-based resuming:

```bash
# The checkpoint is automatically saved
# If interrupted, re-run with same VOPR_OUTPUT_DIR
VOPR_OUTPUT_DIR=./vopr-results/20260202-143022 \
./scripts/vopr-overnight.sh
```

## Analyzing Results

### Check for Failures

```bash
# Single scenario
grep -i "fail" vopr-results/*/summary.txt

# All scenarios
grep -i "fail" vopr-results/comprehensive-*/SUMMARY.txt
```

### Failed Scenario Details

```bash
# View failed scenario log
less vopr-results/comprehensive-*/view-change-merge/vopr.log

# Extract failure reports
grep -A 50 "SIMULATION FAILURE REPORT" vopr-results/comprehensive-*/*/vopr.log
```

### Performance Metrics

```bash
# Single scenario
grep "sims/sec" vopr-results/*/vopr.log

# All scenarios summary
cat vopr-results/comprehensive-*/SUMMARY.txt
```

## Tips

1. **Build in release mode first**: `just build-release`
2. **Monitor progress**: `tail -f vopr-results/*/vopr.log`
3. **Use tmux/screen** for long-running tests
4. **Disable SQL oracles** unless specifically testing them (10-100x slower)
5. **Start with fewer iterations** to validate setup
6. **Check disk space** - logs can grow large with verbose mode

## Troubleshooting

### Binary not found

```bash
# Build the release binary
just build-release

# Or specify custom path
VOPR_BIN=/path/to/vopr ./scripts/vopr-overnight.sh
```

### Out of memory

```bash
# Reduce iterations or use core invariants only
VOPR_CORE_ONLY=1 ./scripts/vopr-overnight.sh
```

### Disk space issues

```bash
# Disable verbose output
VOPR_OPTS="" ./scripts/vopr-overnight.sh
```

## See Also

- `just vopr` - Run VOPR with default settings
- `just vopr-scenarios` - List all available scenarios
- `./target/release/vopr --help` - Full VOPR help
- `./target/release/vopr --list-scenarios` - Detailed scenario descriptions
- `./target/release/vopr --list-invariants` - Available invariants
