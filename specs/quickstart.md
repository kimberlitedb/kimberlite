# Formal Verification Quick Start

**Goal:** Get TLA+ model checking working in <5 minutes.

## Phase 1 Minimal Setup

For Phase 1, you only need **TLC** (TLA+ model checker). TLAPS, Ivy, and Alloy are optional.

### Step 1: Install TLA+ Toolbox

```bash
# macOS
brew install --cask tla-plus-toolbox

# Verify installation
tlc -help
```

**Expected output:**
```
TLC2 Version 2.18 of ...
Usage: java tlc2.TLC [-option] inputfile
```

### Step 2: Run Your First Verification

```bash
# Quick check (depth 10, ~30 seconds)
just verify-tla-quick

# Full check (depth 20, ~2 minutes)
just verify-tla
```

**Expected output:**
```
TLC2 Version 2.18
...
Model checking completed. No errors found.
6405234 states generated, 2873912 distinct states found.
```

### Step 3: Verify It Works

If you see "No errors found", congratulations! Your VSR consensus protocol has been formally verified for all executions up to depth 20.

## What Just Happened?

TLC explored **millions of possible executions** of your VSR protocol and verified that:
- ✅ Replicas never commit conflicting operations (Agreement)
- ✅ Committed log prefixes are identical (PrefixConsistency)
- ✅ View numbers only increase (ViewMonotonicity)
- ✅ One leader per view (LeaderUniqueness)
- ✅ Commit ≤ op always (CommitNotExceedOp)

## Optional: Advanced Tools

### TLAPS (Unbounded Proofs)

**Not required for Phase 1.** TLAPS provides unbounded verification (proves for ALL executions, not just depth 20).

**Easiest way: Use Docker**
```bash
# Pull TLA+ Docker image (includes TLAPS)
docker pull ghcr.io/tlaplus/tlaplus:latest

# Run TLAPS proof
just verify-tlaps-docker
```

**Alternative: Install from source**
See `setup.md` for detailed instructions.

### Ivy (Byzantine Consensus)

**Optional for Phase 1. Complex installation - skip for now.**

Ivy requires building from source. See `setup.md` if you want to try it later.

### Alloy (Structural Models)

**Optional for Phase 1.**

```bash
# Download GUI application
wget https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.dmg
open alloy-6.1.0.dmg
# Install and open .als files with Alloy Analyzer
```

## Troubleshooting

### "tlc: command not found"

The TLA+ Toolbox installation may not add `tlc` to your PATH.

**Fix:**
```bash
# Find TLA+ installation
find /Applications -name "tla2tools.jar" 2>/dev/null

# Create alias (add to ~/.zshrc or ~/.bashrc)
alias tlc='java -cp /Applications/TLA+\ Toolbox.app/Contents/Java/tla2tools.jar tlc2.TLC'
```

### "java.lang.OutOfMemoryError"

TLC needs more memory for large state spaces.

**Fix:**
```bash
# Increase heap size
export TLC_HEAP_SIZE=4g
just verify-tla
```

### Model checking takes too long

Reduce depth or number of replicas.

**Fix:**
```bash
# Edit specs/tla/VSR.cfg
# Change: MaxView = 2 (instead of 3)
# Change: MaxOp = 3 (instead of 4)
```

## Next Steps

1. ✅ **You have working formal verification!**
2. Read `docs/concepts/formal-verification.md` for overview of all 6 layers
3. Read `docs/internals/formal-verification/protocol-specifications.md` for Layer 1 technical details
4. Explore TLA+ specs in `specs/tla/`
5. Explore Coq crypto specs in `specs/coq/`

## CI Integration

Formal verification runs automatically in GitHub Actions:
- See `.github/workflows/formal-verification.yml`
- Runs TLC on every push
- Takes ~5 minutes

## Learning TLA+

- [learntla.com](https://learntla.com/) - Interactive tutorial
- [TLA+ Video Course](https://lamport.azurewebsites.net/video/videos.html) - Free course by Leslie Lamport
- [Practical TLA+](https://www.apress.com/gp/book/9781484238288) - Book by Hillel Wayne

## Questions?

See `specs/README.md` or `docs/concepts/formal-verification.md` for detailed documentation.
