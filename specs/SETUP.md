# Formal Verification Tools Setup

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

**Option 2: Use Docker (easiest, recommended for CI)**
```bash
# Pull pre-configured image with TLA+ tools + TLAPS
docker pull ghcr.io/tlaplus/tlaplus:latest

# Run TLAPS in container
docker run -v $(pwd)/specs:/specs ghcr.io/tlaplus/tlaplus \
  tlapm --check /specs/tla/VSR.tla:AgreementTheorem
```

**Option 3: Build from source (advanced)**
```bash
# Requires OCaml, OPAM, and several dependencies
# See: https://github.com/tlaplus/tlapm#building-from-source
```

**For Phase 1, TLC is sufficient.** TLAPS verification will be added to CI via Docker in later weeks.

### Verify installation
```bash
# TLC should work after installing TLA+ Toolbox
tlc -help

# TLAPS (optional for Phase 1)
tlapm -help  # May not work if not installed
```

## Ivy (Byzantine consensus verification)

**Ivy is complex to install. For Phase 1, you can skip it.**

**Option 1: Build from source (recommended)**
```bash
# Clone repository
git clone https://github.com/kenmcmil/ivy.git
cd ivy

# Install Python dependencies
pip3 install ply pygraphviz z3-solver

# Build and install
python3 setup.py install --user

# Verify
ivy_check --help
```

**Option 2: Use Docker (easiest)**
```bash
# Ivy doesn't have official Docker images yet
# For Phase 1, skip Ivy verification
```

**For Phase 1, Ivy is optional.** The Byzantine consensus model can be verified later.

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

## CI Integration

The verification tools will be integrated into GitHub Actions (see `.github/workflows/formal-verification.yml`).

Local verification before commit:
```bash
just verify-tla      # Run TLA+ verification
just verify-ivy      # Run Ivy verification
just verify-alloy    # Run Alloy verification
just verify-all      # Run all formal verification
```
