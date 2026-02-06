# Formal Verification Tools Setup

**Phase 1 Status:** ✅ **COMPLETE** (Feb 5, 2026)
- 25 TLAPS theorems proven across 4 proof files
- 5 Ivy Byzantine invariants verified
- 3 regulatory framework mappings (HIPAA, GDPR, SOC 2)

This guide covers installing the verification tools needed for Phase 1.

## TLA+ Tools

### TLA+ Toolbox (includes TLC model checker)
```bash
# macOS
brew install --cask tla-plus-toolbox

# Or download from: https://github.com/tlaplus/tlaplus/releases
```

### TLAPS (TLA+ Proof System) - for mechanized proofs

**TLAPS is complex to install. For Phase 1, you can skip it and use TLC only.**

**Option 1: Pre-built binaries (recommended for macOS)**
```bash
# Download from GitHub releases
wget https://github.com/tlaplus/tlapm/releases/download/v1.5.0/tlaps-1.5.0-x86_64-darwin-inst.bin
chmod +x tlaps-1.5.0-x86_64-darwin-inst.bin
sudo ./tlaps-1.5.0-x86_64-darwin-inst.bin

# Or for ARM Mac (if available):
# Check https://github.com/tlaplus/tlapm/releases for latest ARM builds
```

**Option 2: Use Docker (easiest, recommended)**
```bash
# Run TLAPS via Docker wrapper
just verify-tlaps

# First run will pull Docker image automatically
```

The Docker wrapper (`scripts/tlaps_docker.sh`) automatically:
1. Pulls the official TLA+ Docker image if needed
2. Mounts the specs directory
3. Runs tlapm (TLAPS proof manager)

**Option 3: Build from source (advanced)**
```bash
# Requires OCaml, OPAM, and several dependencies
# See: https://github.com/tlaplus/tlapm#building-from-source
```

**For local development, use Docker.** TLAPS proofs may be incomplete - that's expected.

### Verify installation
```bash
# TLC should work after installing TLA+ Toolbox
tlc -help

# TLAPS (optional for Phase 1)
tlapm -help  # May not work if not installed
```

## Ivy (Byzantine consensus verification)

**Ivy is now available via Docker** - no local installation needed!

**Docker-based installation (recommended):**
```bash
# Run Ivy via Docker wrapper
just verify-ivy

# First run will build Docker image (~5-10 minutes)
# Subsequent runs are fast
```

The Docker wrapper (`scripts/ivy_check_docker.sh`) automatically:
1. Builds the Ivy Docker image if needed
2. Mounts the specs directory
3. Runs ivy_check on your specs

**Manual Docker usage:**
```bash
# Build the Ivy image manually
docker build -t kimberlite-ivy docker/ivy

# Run Ivy directly
docker run --rm -v $(pwd)/specs/ivy:/workspace kimberlite-ivy VSR_Byzantine.ivy
```

**Native installation (advanced, not recommended):**
Native Ivy installation is complex due to Z3 build dependencies. Use Docker instead.

## Alloy (structural modeling)

**Option 1: Download GUI application (recommended)**
```bash
# Download from GitHub releases
wget https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.dmg

# Or use direct link:
open https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.dmg

# Install the .dmg file
```

**Option 2: Command-line (requires Java)**
```bash
# Download JAR
wget https://github.com/AlloyTools/org.alloytools.alloy/releases/download/v6.1.0/alloy-6.1.0.jar

# Run Alloy
java -jar alloy-6.1.0.jar specs/alloy/HashChain.als
```

**Note:** Homebrew doesn't have an Alloy package.

## Running Verification

### TLA+ Model Checking (bounded)
```bash
# Check VSR spec with TLC
tlc -workers auto -depth 20 specs/tla/VSR.tla

# Fast check (depth 10)
tlc -workers auto -depth 10 specs/tla/VSR.tla
```

### TLAPS Mechanized Proofs (unbounded)
```bash
# Verify specific theorem
tlapm --check specs/tla/VSR.tla:Agreement

# Verify all theorems in file
tlapm specs/tla/VSR.tla
```

### Ivy Byzantine Model
```bash
ivy_check specs/ivy/VSR_Byzantine.ivy
```

### Alloy Structural Models
```bash
alloy specs/alloy/HashChain.als
alloy specs/alloy/Quorum.als
```

## Quick Start (Recommended Workflow)

**For daily development:**
```bash
just verify-tla-quick  # Fast TLA+ verification (~1 min)
```

**Before commits:**
```bash
just verify-local      # All tools (~5-10 min)
```

**Individual tools:**
```bash
just verify-tla        # TLA+ TLC model checking
just verify-tlaps      # TLAPS proofs (Docker)
just verify-alloy      # Alloy structural models
just verify-ivy        # Ivy Byzantine model (Docker)
```

## CI Integration

The verification tools are integrated into GitHub Actions (`.github/workflows/formal-verification.yml`) using Docker for TLAPS and Ivy.

## Tool Status

| Tool | Status | Verification Coverage | Installation | Time |
|------|--------|---------------------|--------------|------|
| TLC | ✅ Working | Bounded model checking | Homebrew | ~1-2 min |
| TLAPS | ✅ Complete | 25 theorems proven | Docker (auto-pull) | Varies |
| Alloy | ✅ Complete | 6+ structural assertions | JAR included | ~10-30 sec |
| Ivy | ✅ Complete | 5 Byzantine invariants | Docker (auto-build) | Varies |

**All 6 Phases Complete:** All verification tools are fully functional with Docker-based workflows. See `docs/internals/formal-verification/protocol-specifications.md` for Layer 1 technical details or `docs/concepts/formal-verification.md` for an overview of all 6 layers.
