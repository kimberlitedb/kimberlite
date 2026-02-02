# VOPR Overnight Testing - Quick Start

## TL;DR

```bash
# Test all 27 scenarios overnight (recommended)
just vopr-overnight-all

# Test single scenario with many iterations
just vopr-overnight combined 50000000

# Quick smoke test (all scenarios, 10k iterations each, ~5 min)
VOPR_ITERATIONS=10000 ./scripts/vopr-overnight-all.sh
```

## What Changed

### Added 15 New Scenarios

**Byzantine Attacks (Phase 3A):**
- `dvc-tail-mismatch` - DoViewChange log_tail length mismatch
- `dvc-identical-claims` - DoViewChange with identical claims
- `oversized-start-view` - Oversized StartView (DoS attack)
- `invalid-repair-range` - Invalid repair range
- `invalid-kernel-command` - Invalid kernel command

**Corruption Detection:**
- `bit-flip` - Random bit flip in log entry
- `checksum-validation` - Checksum validation test
- `silent-disk-failure` - Silent disk corruption

**Crash & Recovery:**
- `crash-commit` - Crash during commit
- `crash-view-change` - Crash during view change
- `recovery-corrupt` - Recovery with corrupt log

**Gray Failure Variants:**
- `slow-disk` - Slow disk I/O
- `intermittent-network` - Intermittent network

**Race Conditions:**
- `race-view-changes` - Concurrent view changes
- `race-commit-dvc` - Commit during DoViewChange

### New Scripts

1. **`vopr-overnight-all.sh`** - Run all 27 scenarios sequentially
2. **`README-VOPR.md`** - Complete documentation
3. **`QUICKSTART.md`** - This file

### New Just Commands

```bash
# Run all 27 scenarios with custom iterations
just vopr-overnight-all 5000000

# Run single scenario overnight
just vopr-overnight combined 50000000

# List all scenarios
just vopr-scenarios

# Test all scenarios (short run)
just vopr-all-scenarios 100
```

## Common Usage Patterns

### Overnight Test (8 hours)

```bash
# All scenarios, 1M iterations each (~6 hours)
just vopr-overnight-all 1000000

# Single combined scenario, max iterations (~8 hours)
just vopr-overnight combined 2400000000
```

### CI/Pre-commit

```bash
# Quick validation (all scenarios, 1k each, ~1 min)
just vopr-all-scenarios 1000
```

### Byzantine Attack Testing

```bash
# Test specific scenario
just vopr-overnight view-change-merge 10000000
```

### Weekend Run

```bash
# Comprehensive: all scenarios, 10M each (~24 hours)
just vopr-overnight-all 10000000
```

## Monitoring Progress

```bash
# Watch latest scenario
tail -f vopr-results/comprehensive-*/$(ls -t vopr-results/comprehensive-* | head -1)/**/vopr.log

# Check summary
cat vopr-results/comprehensive-*/SUMMARY.txt
```

## Next Steps

1. **Test the setup:**
   ```bash
   just vopr-overnight-all 10000
   ```

2. **Review results:**
   ```bash
   cat vopr-results/comprehensive-*/SUMMARY.txt
   ```

3. **Run overnight:**
   ```bash
   # In tmux/screen:
   just vopr-overnight-all 5000000
   ```

See `README-VOPR.md` for complete documentation.
