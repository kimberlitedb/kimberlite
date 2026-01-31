# VOPR Overnight Testing - Quick Start

## TL;DR - Run Overnight Test

```bash
# Build and run overnight (100k iterations = ~8-12 hours)
just build-release && ./scripts/vopr-overnight.sh
```

That's it! Check results in the morning at `./vopr-results/*/summary.txt`

---

## Even Simpler (One Command)

```bash
# Build and run in background
just build-release && \
nohup ./scripts/vopr-overnight.sh > vopr-night.out 2>&1 &

# Check progress anytime
tail -f vopr-night.out
```

---

## Quick Manual Test (1 minute)

```bash
# Build
just build-release

# Run 1000 iterations (~1-2 minutes)
./target/release/vopr -n 1000 -v
```

---

## What You'll See

### If No Bugs Found (Success)
```
Results:
  Successes: 100000
  Failures: 0

✅ All tests passed!
```

### If Bugs Found (Failure)
```
Results:
  Successes: 98523
  Failures: 3

⚠️  FAILURES DETECTED! See ./vopr-results/.../failures.log

Failed seeds (for reproduction):
  vopr --seed 42 -v
    Error: model_verification: data mismatch
  vopr --seed 1337 -v
    Error: replica_consistency: divergence detected
```

---

## Reproduce a Failure

If VOPR finds a bug, reproduce it with:

```bash
# Use the exact seed from the failure report
./target/release/vopr --seed 42 -v

# For full trace (debugging)
./target/release/vopr --seed 42 -v --enable-trace
```

---

## Custom Configurations

```bash
# Run 500k iterations (~40 hours)
VOPR_ITERATIONS=500000 ./scripts/vopr-overnight.sh

# Specific seed
VOPR_SEED=12345 ./scripts/vopr-overnight.sh

# Disable faults (baseline test)
./target/release/vopr -n 100000 --no-faults -v
```

---

## Files & Locations

| File | Location | Description |
|------|----------|-------------|
| VOPR binary | `./target/release/vopr` | The test runner |
| Quick test script | `./scripts/vopr-quick.sh` | Fast tests |
| Overnight script | `./scripts/vopr-overnight.sh` | Long runs |
| Full guide | `./scripts/VOPR-TESTING.md` | Detailed docs |
| Results | `./vopr-results/YYYYMMDD-HHMMSS/` | Test output |

---

## Performance Expectations

| Iterations | Time | Use Case |
|------------|------|----------|
| 1,000 | 1-2 min | Quick check |
| 10,000 | 10-20 min | Integration test |
| 100,000 | 8-12 hrs | **Overnight test** |
| 500,000 | ~40 hrs | Weekend test |

---

## What VOPR Tests

VOPR validates **8 critical invariants** with fault injection:

- ✅ Data correctness (reads match writes)
- ✅ Linearizability (operation ordering)
- ✅ Replica consistency (same log = same hash)
- ✅ Hash chain integrity
- ✅ Commit history (no gaps)
- ✅ Checkpoint/recovery
- ✅ Storage determinism

With injected failures:
- 🔥 Network delays, drops, partitions
- 🔥 Storage corruption, partial writes, fsync failures

---

## Current Status

**As of this test, VOPR is finding linearizability issues:**

```bash
$ ./target/release/vopr --seed 42 -n 10 -v

Results:
  Successes: 7
  Failures: 3 (linearizability violations)
```

This could indicate:
1. ✅ **VOPR working correctly** - detecting real concurrency issues
2. 🔍 **Worth investigating** - might be a bug in the simulation or checker
3. 📊 **Expected behavior** - some seeds may hit edge cases

**Recommendation**: Run overnight to gather more data on failure rate and patterns.

---

## Help & Documentation

```bash
# VOPR help
./target/release/vopr --help

# Full testing guide
cat ./scripts/VOPR-TESTING.md

# Script source
cat ./scripts/vopr-overnight.sh
```

---

## Pro Tips

1. **Use tmux/screen** for overnight runs (survives disconnection)
2. **Start with 1000 iterations** to verify setup
3. **Save failure seeds** for regression testing
4. **Monitor disk space** (logs can grow with `-v`)

---

**Ready to find bugs? Run the overnight test and check back in the morning! 🚀**
