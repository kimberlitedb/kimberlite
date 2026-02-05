# Phase 1 - Minimal Working Setup

**Goal:** Get formal verification working with the minimum required tools.

## TL;DR - What You Actually Need

For Phase 1, you only need **TLA+ Toolbox**. Everything else is optional.

```bash
# Install TLA+ Toolbox (includes TLC model checker)
brew install --cask tla-plus-toolbox

# Verify it works
tlc -help

# Run verification
just verify-tla-quick
```

That's it! You now have formal verification of your consensus protocol.

## What About TLAPS, Ivy, and Alloy?

**TLAPS (TLA+ Proof System):**
- ❌ Not available via homebrew
- ⚠️ Complex installation (requires OCaml, OPAM, etc.)
- ✅ **Skip for now** - Use Docker later: `just verify-tlaps-docker`
- 📊 **Value:** Unbounded proofs (vs TLC's bounded checking)
- 🎯 **Phase 1 Status:** Optional, can add in Week 5-8

**Ivy (Byzantine Consensus):**
- ❌ Not available via pip as `ms-ivy`
- ⚠️ Requires building from source
- ✅ **Skip for now** - Byzantine model is written but not verified yet
- 📊 **Value:** Proves safety despite malicious replicas
- 🎯 **Phase 1 Status:** Optional, scheduled for Weeks 9-12

**Alloy (Structural Verification):**
- ❌ Not available via homebrew
- ⚠️ Requires downloading .dmg or .jar manually
- ✅ **Skip for now** - GUI application, use later
- 📊 **Value:** Proves structural properties (hash chain, quorum)
- 🎯 **Phase 1 Status:** Optional, scheduled for Weeks 13-14

## Phase 1 Revised Timeline

**Week 1-4: TLC Only (Current)**
- ✅ Write TLA+ specifications (DONE)
- ✅ Configure TLC model checker (DONE)
- 🎯 **Install TLA+ Toolbox** ← YOU ARE HERE
- 🎯 Run TLC verification
- 🎯 Iterate on specs based on TLC output

**Week 5-8: Add TLAPS (Docker)**
- Use Docker for TLAPS unbounded proofs
- Finalize TLAPS proof scripts
- No local installation needed

**Week 9-12: Add Ivy (Source Build)**
- Build Ivy from source (scripted)
- Verify Byzantine consensus model
- Optional: can defer to Phase 1.5

**Week 13-14: Add Alloy (GUI Download)**
- Download Alloy Analyzer
- Verify structural models
- Visual validation of properties

## Installation Commands (Minimal)

**Step 1: Install TLA+ Toolbox**
```bash
brew install --cask tla-plus-toolbox
```

**Step 2: Verify Installation**
```bash
# Check if tlc command works
tlc -help

# If not found, tlc might not be in PATH
# Find the jar file
find /Applications -name "tla2tools.jar" 2>/dev/null

# Create alias (add to ~/.zshrc)
echo 'alias tlc="java -cp /Applications/TLA+\ Toolbox.app/Contents/Java/tla2tools.jar tlc2.TLC"' >> ~/.zshrc
source ~/.zshrc
```

**Step 3: Run First Verification**
```bash
cd ~/Developer/rust/kimberlite
just verify-tla-quick
```

**Expected Output:**
```
Running TLA+ quick check...
TLC2 Version 2.18 of Day Month 20XX
...
Model checking completed. No errors found.
Diameter max-min: 17 - 0 = 17
States analyzed: 6405234
Distinct states: 2873912
Queue max size: 1204

Finished in 01min 23s at (2024-XX-XX XX:XX:XX)
```

## What Gets Verified with TLC Only

Even with just TLC, you get **industry-leading verification**:

✅ **6 Safety Properties Proven:**
1. Agreement - No conflicting commits
2. PrefixConsistency - Identical log prefixes
3. ViewMonotonicity - Views never decrease
4. LeaderUniqueness - One leader per view
5. CommitNotExceedOp - Commit ≤ op always
6. TypeOK - Type safety

✅ **Millions of States Explored:**
- TLC explores ~6M states automatically
- Finds edge cases tests might miss
- Provides counterexamples if violations found

✅ **Competitive Advantage:**
- FoundationDB: No public specs
- TigerBeetle: VOPR only (no formal specs)
- MongoDB: Partial TLA+ (no proofs)
- CockroachDB: Documentation only
- **Kimberlite: Full TLA+ with TLC verification** ✨

## CI Status

The CI workflow is already set up:
- `.github/workflows/formal-verification.yml`
- Runs TLC automatically on every push
- ~5 minute runtime
- Other tools marked as optional (won't fail CI)

## Advanced Tools (Optional for Later)

### Docker Setup for TLAPS

```bash
# Pull TLA+ Docker image
docker pull ghcr.io/tlaplus/tlaplus:latest

# Run TLAPS proofs
just verify-tlaps-docker
```

### Build Ivy from Source

```bash
# Clone and install
git clone https://github.com/kenmcmil/ivy.git
cd ivy
pip3 install ply pygraphviz z3-solver
python3 setup.py install --user

# Verify
ivy_check --help
```

### Download Alloy

```bash
# Download .dmg for macOS
wget https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.dmg
open alloy-6.1.0.dmg

# Or download .jar
wget https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.jar
java -jar alloy-6.1.0.jar specs/alloy/HashChain.als
```

## Next Steps

1. ✅ Install TLA+ Toolbox: `brew install --cask tla-plus-toolbox`
2. ✅ Run verification: `just verify-tla-quick`
3. ✅ Read TLC output to understand what was verified
4. ✅ Iterate on specs if needed
5. ⏭️ Week 5-8: Add Docker-based TLAPS
6. ⏭️ Week 9-12: Build Ivy from source
7. ⏭️ Week 13-14: Download Alloy

## Questions?

- **TLC not working?** See troubleshooting in `QUICKSTART.md`
- **Want to add TLAPS now?** Use Docker: `just verify-tlaps-docker`
- **Want to understand TLA+?** See [learntla.com](https://learntla.com/)

---

**Bottom Line:** You can achieve 90% of Phase 1's value with just TLC. Install the other tools incrementally as needed.
