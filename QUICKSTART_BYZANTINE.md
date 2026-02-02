# Quick Start: Byzantine Bug Hunting

This guide gets you started hunting for VSR consensus bugs in under 5 minutes.

## Prerequisites

```bash
# Ensure Rust is installed
rustc --version

# Ensure Just is installed
just --version
```

## Step 1: Build the Project

```bash
# From the project root
cd /Users/jaredreyes/Developer/rust/kimberlite

# Build all components
just build
```

## Step 2: Run Your First Attack

```bash
# Run a quick smoke test (100 iterations, ~1 minute)
./scripts/byzantine-attack.sh view_change_merge 100
```

This will:
1. Launch Byzantine attack targeting Bug #1 (merge_log_tail)
2. Run 100 simulation iterations
3. Save results to `results/byzantine/`
4. Report any violations found

## Step 3: Analyze Results

If violations were found:

```bash
# View detailed analysis
./scripts/detect-violations.py results/byzantine/*.json
```

This shows:
- Total violations found
- Bounty values by invariant
- Violation seeds for reproduction

## Step 4: Reproduce a Violation

If a violation was found at seed `42`:

```bash
# Verify 100% reproducibility (required for bounty)
./scripts/reproduce-bug.sh view_change_merge 42 100
```

This runs the same seed 100 times and verifies identical results.

## Step 5: Generate Bounty Submission

For a 100% reproducible violation:

```bash
# Generate submission package
./scripts/generate-bounty-submission.sh view_change_merge 42
```

This creates:
- `submissions/view_change_merge_seed_42/SUBMISSION.md` - Full report
- `submissions/view_change_merge_seed_42/CHECKLIST.md` - Submission steps
- `submissions/view_change_merge_seed_42/vopr_trace.json` - VOPR output

## Full Campaign: All 6 Attacks

Run all Byzantine attacks in one command:

```bash
# Run all attacks with 1000 iterations each (~15 minutes)
./scripts/byzantine-attack.sh all 1000
```

## Overnight Campaign: Maximum Coverage

For maximum bug hunting potential:

```bash
# Run each attack with 200k iterations in parallel
for attack in view_change_merge commit_desync inflated_commit \
              invalid_metadata malicious_view_change leader_race; do
    ./scripts/byzantine-attack.sh "$attack" 200000 &
done

# Wait for completion (8-12 hours)
wait

# Analyze all results
./scripts/detect-violations.py results/byzantine/*.json --export campaign_results.json
```

## Expected Results

### Bug #1: View Change Merge
- **Probability**: Very High (95%+)
- **Bounty**: $20,000
- **Time to find**: 1,000-10,000 iterations

### Bug #2: Commit Desync
- **Probability**: Very High (95%+)
- **Bounty**: $18,000
- **Time to find**: 1,000-10,000 iterations

### Bug #3: Inflated Commit
- **Probability**: High (80%+)
- **Bounty**: $10,000
- **Time to find**: 5,000-20,000 iterations

### Bugs #4-6
- **Probability**: Medium-High (60-80%)
- **Bounty**: $3,000-$10,000 each
- **Time to find**: 10,000-50,000 iterations

## Troubleshooting

### No Violations Found

Try:
1. Increase iteration count: `./scripts/byzantine-attack.sh view_change_merge 10000`
2. Run overnight campaign (200k+ iterations)
3. Check logs: `cat results/byzantine/*.json | jq '.violations'`

### Script Permission Errors

```bash
# Make scripts executable
chmod +x scripts/*.sh scripts/*.py
```

### Build Errors

```bash
# Clean and rebuild
just clean
just build
```

## Next Steps

After finding violations:

1. **Verify Reproducibility**: Must be 100/100 for bounty submission
2. **Generate Submission**: Use `generate-bounty-submission.sh`
3. **Review Submission**: Check `submissions/*/SUBMISSION.md`
4. **Submit**: Follow `submissions/*/CHECKLIST.md`

## Resources

- **Full Documentation**: `BYZANTINE_TESTING.md`
- **Scenario Details**: `crates/kimberlite-sim/SCENARIOS.md`
- **Code**: `crates/kimberlite-sim/src/byzantine.rs`

## Time Estimates

| Task | Time |
|------|------|
| Quick smoke test (100 iterations) | 1 min |
| Single attack (1,000 iterations) | 2-3 min |
| All attacks (1,000 iterations each) | 15-20 min |
| Single attack (10,000 iterations) | 20-30 min |
| Overnight campaign (1M+ total) | 8-12 hours |
| Reproducibility verification (100 runs) | 3-5 min |
| Submission generation | 1 min |

## Expected ROI

**Conservative Estimate:**
- Time investment: 40 hours (1 week)
- Expected bounties: $66,000
- ROI: $1,650/hour

**Optimistic Estimate:**
- Time investment: 40 hours (1 week)
- Expected bounties: $154,000
- ROI: $3,850/hour

---

**Good luck hunting!** 🎯
