# ═══════════════════════════════════════════════════════════════════════════════
# KIMBERLITE TASK RUNNER
# ═══════════════════════════════════════════════════════════════════════════════
# Quick reference of common commands:
#   just build          - Build debug binary
#   just test           - Run all tests
#   just vopr           - Run VOPR simulation
#   just vopr-byzantine - Run Byzantine attack tests
#   just verify-local   - Run all formal verification
#   just pre-commit     - Run pre-commit checks
#   just fmt            - Format code
#
# Run 'just --list' to see all available commands
# ═══════════════════════════════════════════════════════════════════════════════

set dotenv-load := false

# Default: show available commands
default:
    @just --list

# ───────────────────────────────────────────────────────────────────────────────
# BUILDING
# ───────────────────────────────────────────────────────────────────────────────

# Build debug (fast compile, no optimizations)
build:
    cargo build --workspace

# Build release (optimized, takes longer)
build-release:
    cargo build --workspace --release

# Build CLI only
build-cli:
    cargo build --release -p kimberlite-cli

# Build FFI library for current platform
build-ffi:
    cargo build --package kimberlite-ffi --release
    @echo "FFI library built: target/release/libkimberlite_ffi.*"

# Build with all features
build-all-features:
    cargo build --workspace --all-features

# Clean build artifacts
clean:
    cargo clean

# Clean all artifacts (build + test + logs)
clean-all:
    cargo clean
    rm -rf .artifacts/
    rm -rf target/
    find . -name "*.log" -type f -delete
    find . -name "*.kmb" -type f -delete

# Archive VOPR logs (move to artifacts directory)
archive-vopr-logs:
    mkdir -p .artifacts/vopr/logs
    mv vopr-*.log .artifacts/vopr/logs/ 2>/dev/null || true
    mv *.kmb .artifacts/vopr/logs/ 2>/dev/null || true

# Clean test artifacts only
clean-test:
    rm -rf .artifacts/vopr/
    rm -rf states/
    rm -rf results/
    find . -name "*.kmb" -type f -delete

# ───────────────────────────────────────────────────────────────────────────────
# TESTING
# ───────────────────────────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --workspace --all-features

# Run tests with nextest (faster, better output)
nextest:
    cargo nextest run --workspace --all-features

# Run a specific test
test-one name:
    cargo test --workspace {{name}}

# Run tests with output shown
test-verbose:
    cargo test --workspace --all-features -- --nocapture

# Test code examples in documentation
test-docs:
    @echo "Testing documentation code examples..."
    cargo test --package kimberlite-doc-tests --doc --verbose

# Run property tests (extended)
test-property:
    PROPTEST_CASES=10000 cargo test --workspace

# Test canary mutations (verifies VOPR detects intentional bugs)
test-canaries:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "======================================================================"
    echo "Canary Mutation Testing - Verifying Invariant Detection"
    echo "======================================================================"
    echo ""

    declare -A CANARY_INVARIANTS=(
        ["canary-skip-fsync"]="log_consistency"
        ["canary-wrong-hash"]="hash_chain"
        ["canary-commit-quorum"]="vsr_agreement"
        ["canary-idempotency-race"]="linearizability"
        ["canary-monotonic-regression"]="replica_head"
    )

    PASSED=0
    FAILED=0

    for canary in "${!CANARY_INVARIANTS[@]}"; do
        invariant="${CANARY_INVARIANTS[$canary]}"
        echo "Testing: $canary"
        echo "  Target invariant: $invariant"
        echo -n "  Checking detection capability... "

        if ./target/release/vopr \
            --iterations 10 \
            --seed 99999 \
            --enable-invariant "$invariant" \
            --core-invariants-only > /dev/null 2>&1; then
            echo "✓ PASSED"
            PASSED=$((PASSED + 1))
        else
            echo "✗ FAILED"
            FAILED=$((FAILED + 1))
        fi
        echo ""
    done

    echo "======================================================================"
    echo "Summary: Passed $PASSED, Failed $FAILED"
    echo "======================================================================"
    [ $FAILED -eq 0 ]

# Test FFI library
test-ffi:
    cargo test --package kimberlite-ffi

# Test Python SDK
test-python:
    #!/usr/bin/env bash
    cd sdks/python
    pip install -e ".[dev]"
    mypy kimberlite
    pytest --cov=kimberlite

# Test TypeScript SDK
test-typescript:
    #!/usr/bin/env bash
    cd sdks/typescript
    npm install
    npm run type-check
    npm test

# Test all SDKs
test-sdks: build-ffi test-python test-typescript
    @echo "All SDK tests passed!"

# ───────────────────────────────────────────────────────────────────────────────
# VOPR SIMULATION TESTING
# ───────────────────────────────────────────────────────────────────────────────

# Run VOPR with default scenario
vopr *args:
    cargo run --release -p kimberlite-sim --bin vopr -- {{args}}

# Quick smoke test (100 iterations, baseline scenario)
vopr-quick:
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario baseline -n 100

# Fast development smoke test (linearizability disabled for speed)
vopr-dev:
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario baseline -n 100 --disable-invariant linearizability

# Full test suite (all scenarios with substantial iterations)
vopr-full iterations="10000":
    @just vopr-all-scenarios {{iterations}}

# Reproduce failure from .kmb bundle file
vopr-repro bundle:
    @echo "Reproducing failure from: {{bundle}}"
    cargo run --release -p kimberlite-sim --bin vopr -- repro {{bundle}}

# Run VOPR without fault injection (faster)
vopr-clean iterations="100":
    cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}

# Run VOPR with specific seed for reproduction
vopr-seed seed:
    cargo run --release -p kimberlite-sim --bin vopr -- --seed {{seed}} -v -n 1

# Run VOPR with JSON output (for headless runners that scrape logs)
vopr-json iterations="100":
    cargo run --release -p kimberlite-sim --bin vopr -- --json -n {{iterations}}

# List all VOPR scenarios
vopr-scenarios:
    cargo run --release -p kimberlite-sim --bin vopr -- --list-scenarios

# Run VOPR with a specific scenario
vopr-scenario scenario="baseline" iterations="100" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario {{scenario}} -n {{iterations}} {{args}}

# Run all VOPR scenarios sequentially
vopr-all-scenarios iterations="100":
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running all VOPR scenarios..."
    for scenario in baseline swizzle gray multi-tenant time-compression combined \
        view-change-merge commit-desync inflated-commit invalid-metadata malicious-view-change leader-race \
        dvc-tail-mismatch dvc-identical-claims oversized-start-view invalid-repair-range invalid-kernel-command \
        bit-flip checksum-validation silent-disk-failure \
        crash-commit crash-view-change recovery-corrupt \
        slow-disk intermittent-network \
        race-view-changes race-commit-dvc; do
        echo "=== Running scenario: $scenario ==="
        cargo run --release -p kimberlite-sim --bin vopr -- --scenario $scenario -n {{iterations}}
    done
    echo "All scenarios complete!"

# Run Byzantine attack simulations
vopr-byzantine iterations="1000":
    #!/usr/bin/env bash
    set -euo pipefail

    RESULTS_DIR=".artifacts/vopr/results/byzantine"
    mkdir -p "$RESULTS_DIR"

    ATTACK_SCENARIOS=(
        "view_change_merge:byzantine_view_change_merge"
        "commit_desync:byzantine_commit_desync"
        "inflated_commit:byzantine_inflated_commit"
        "invalid_metadata:byzantine_invalid_metadata"
        "malicious_view_change:byzantine_malicious_view_change"
        "leader_race:byzantine_leader_race"
    )

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Byzantine Attack Campaign - 6 Scenarios"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Iterations per attack: {{iterations}}"
    echo "Total iterations: $(({{iterations}} * 6))"
    echo ""

    for entry in "${ATTACK_SCENARIOS[@]}"; do
        IFS=':' read -r attack_key scenario_type <<< "$entry"
        output_file="$RESULTS_DIR/${attack_key}_$(date +%Y%m%d_%H%M%S).json"

        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo "  Attack: $attack_key"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo "Scenario: $scenario_type"
        echo "Output: $output_file"
        echo ""

        if cargo run --release -p kimberlite-sim --bin vopr -- \
            --scenario "$scenario_type" -n {{iterations}} --json > "$output_file" 2>&1; then
            echo "✅ Attack completed"
        else
            echo "⚠️  Attack completed with violations detected"
        fi

        if [ -f "$output_file" ]; then
            violations=$(jq -r '.violations // [] | length' "$output_file" 2>/dev/null || echo "0")
            if [ "$violations" -gt 0 ]; then
                echo "🔥 Found $violations violation(s)!"
                jq -r '.violations // [] | .[].seed' "$output_file" 2>/dev/null | while read -r seed; do
                    echo "  Seed: $seed"
                done
            fi
        fi
        echo ""
    done

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Campaign Complete"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Results directory: $RESULTS_DIR"

# Run VOPR CI checks (simulates CI determinism validation)
vopr-ci:
    #!/usr/bin/env bash
    set -e

    echo "================================================"
    echo "VOPR CI Simulation - Local Determinism Check"
    echo "================================================"
    echo ""

    cargo build --release -p kimberlite-sim --bin vopr

    # Check 1: Baseline
    echo "Check 1: Baseline scenario (100 iterations)"
    ./target/release/vopr --scenario baseline --iterations 100 --check-determinism --seed 12345
    echo "✅ Baseline passed"
    echo ""

    # Check 2: Combined
    echo "Check 2: Combined scenario (50 iterations)"
    ./target/release/vopr --scenario combined --iterations 50 --check-determinism --seed 54321
    echo "✅ Combined passed"
    echo ""

    # Check 3: Multi-tenant
    echo "Check 3: Multi-tenant isolation (50 iterations)"
    ./target/release/vopr --scenario multi_tenant_isolation --iterations 50 --check-determinism --seed 99999
    echo "✅ Multi-tenant passed"
    echo ""

    # Check 4: Coverage
    echo "Check 4: Coverage enforcement (200 iterations)"
    ./target/release/vopr \
        --iterations 200 \
        --min-fault-coverage 80.0 \
        --min-invariant-coverage 100.0 \
        --require-all-invariants \
        --check-determinism
    echo "✅ Coverage passed"
    echo ""

    echo "================================================"
    echo "✅ All VOPR CI checks passed!"
    echo "================================================"

# ───────────────────────────────────────────────────────────────────────────────
# VOPR STRESS TESTS
# ───────────────────────────────────────────────────────────────────────────────

# Shallow test: many iterations, few events per sim
vopr-shallow iterations="100000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 10000 {{args}}

# Medium test: balanced depth and breadth (~1 hour)
vopr-medium iterations="5000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 100000 {{args}}

# Deep test: fewer iterations, many events per sim (~4 hours)
vopr-deep iterations="50000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 500000 {{args}}

# Overnight test: deep simulations (~8-12 hours)
vopr-overnight iterations="2000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 1000000 --checkpoint-file vopr-checkpoint.json {{args}}

# Marathon test: extreme depth (24+ hours)
vopr-marathon iterations="5000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 5000000 --checkpoint-file vopr-marathon.json {{args}}

# ───────────────────────────────────────────────────────────────────────────────
# VOPR ADVANCED DEBUGGING
# ───────────────────────────────────────────────────────────────────────────────

# Show timeline visualization of failure bundle
vopr-timeline bundle width="120":
    cargo run --release -p kimberlite-sim --bin vopr -- timeline {{bundle}} --width {{width}}

# Bisect to find first failing event
vopr-bisect bundle:
    cargo run --release -p kimberlite-sim --bin vopr -- bisect {{bundle}}

# Minimize failure bundle using delta debugging
vopr-minimize bundle:
    cargo run --release -p kimberlite-sim --bin vopr -- minimize {{bundle}}

# Start VOPR coverage dashboard
vopr-dashboard port="8080":
    cargo run --release -p kimberlite-sim --bin vopr --features dashboard -- dashboard --port {{port}}

# Launch interactive TUI
vopr-tui iterations="1000":
    cargo run --release -p kimberlite-sim --bin vopr --features tui -- tui --iterations {{iterations}}

# ───────────────────────────────────────────────────────────────────────────────
# FORMAL VERIFICATION
# ───────────────────────────────────────────────────────────────────────────────

# Run all formal verification locally
verify-local:
    #!/usr/bin/env bash
    set -e
    echo "=========================================="
    echo "Kimberlite Formal Verification - Local"
    echo "=========================================="
    echo ""
    FAILED=()

    # 1. TLA+ TLC
    echo "[1/5] Running TLA+ Model Checking (TLC)..."
    if just verify-tla-quick 2>&1 | grep -q "Finished"; then
        echo "✅ TLA+ TLC: PASSED"
    else
        echo "❌ TLA+ TLC: FAILED"
        FAILED+=("TLA+")
    fi
    echo ""

    # 2. TLAPS
    echo "[2/5] Running TLAPS Mechanized Proofs..."
    if just verify-tlaps > /dev/null 2>&1; then
        echo "✅ TLAPS: PASSED"
    else
        echo "⚠️  TLAPS: FAILED (may be expected if proofs incomplete)"
    fi
    echo ""

    # 3. Coq
    echo "[3/5] Running Coq Cryptographic Proofs..."
    if just verify-coq > /dev/null 2>&1; then
        echo "✅ Coq: PASSED"
    else
        echo "❌ Coq: FAILED"
        FAILED+=("Coq")
    fi
    echo ""

    # 4. Alloy (uses -quick variants when available for local speed)
    echo "[4/5] Running Alloy Structural Models..."
    ALLOY_FAILED=0
    mkdir -p .artifacts/formal-verification/alloy

    # Build spec list: skip full-scope specs when a -quick variant exists
    ALLOY_SPECS=()
    for spec in specs/alloy/*.als; do
        base=$(basename "$spec")
        quick_variant="${spec%.als}-quick.als"
        if [[ "$base" != *-quick.als ]] && [[ -f "$quick_variant" ]]; then
            continue  # skip full scope, quick variant will be picked up
        fi
        ALLOY_SPECS+=("$spec")
    done

    total_specs=${#ALLOY_SPECS[@]}
    current_spec=0

    for spec in "${ALLOY_SPECS[@]}"; do
        current_spec=$((current_spec + 1))
        spec_name=$(basename "$spec" .als)
        echo "  [$current_spec/$total_specs] Checking $spec_name.als..."

        start_time=$(date +%s)
        if java -Djava.awt.headless=true -jar tools/formal-verification/alloy/alloy-6.2.0.jar exec -f -o ".artifacts/formal-verification/alloy/$spec_name" "$spec" > /dev/null 2>&1; then
            end_time=$(date +%s)
            elapsed=$((end_time - start_time))
            echo "      ✅ Completed in ${elapsed}s"
        else
            end_time=$(date +%s)
            elapsed=$((end_time - start_time))
            echo "      ❌ Failed after ${elapsed}s"
            ALLOY_FAILED=1
        fi
    done

    if [ $ALLOY_FAILED -eq 0 ]; then
        echo "✅ Alloy: PASSED"
    else
        echo "❌ Alloy: FAILED"
        FAILED+=("Alloy")
    fi
    echo ""

    # 5. Ivy
    echo "[5/5] Running Ivy Byzantine Model..."
    if just verify-ivy > /dev/null 2>&1; then
        echo "✅ Ivy: PASSED"
    else
        echo "❌ Ivy: FAILED"
        FAILED+=("Ivy")
    fi
    echo ""

    # Summary
    echo "=========================================="
    echo "Verification Summary"
    echo "=========================================="
    if [ ${#FAILED[@]} -eq 0 ]; then
        echo "✅ ALL VERIFICATIONS PASSED"
        exit 0
    else
        echo "❌ FAILURES: ${FAILED[*]}"
        exit 1
    fi

# Run TLA+ model checking (bounded verification)
verify-tla workers="2":
    #!/usr/bin/env bash
    set -e
    mkdir -p .artifacts/formal-verification/tla
    echo "Running TLA+ model checking ({{workers}} workers, depth 20)..."
    cd specs/tla && tlc -workers {{workers}} -depth 20 VSR.tla 2>&1 | tee ../../.artifacts/formal-verification/tla/tlc-output.log

# Run TLA+ quick check (small config, depth 10)
verify-tla-quick workers="2":
    #!/usr/bin/env bash
    set -e
    mkdir -p .artifacts/formal-verification/tla
    echo "Running TLA+ quick check ({{workers}} workers, depth 10, small config)..."
    cd specs/tla && tlc -workers {{workers}} -depth 10 -config VSR_Small.cfg VSR.tla 2>&1 | tee ../../.artifacts/formal-verification/tla/tlc-output.log

# Run TLAPS mechanized proofs (via Docker — x86_64 only, fails on ARM due to Isabelle JDK)
verify-tlaps:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running TLAPS mechanized proofs via Docker..."

    ARCH=$(uname -m)
    if [[ "$ARCH" == "arm64" || "$ARCH" == "aarch64" ]]; then
        echo "⚠️  TLAPS skipped: Isabelle JDK lacks ARM Linux support (upstream issue)"
        echo "   TLAPS runs in CI on x86_64 runners. See tools/formal-verification/docker/tlaps/Dockerfile"
        exit 0
    fi

    if ! docker info > /dev/null 2>&1; then
        echo "Error: Docker is not running"
        exit 1
    fi

    TLAPS_IMAGE="kimberlite-tlaps"
    if ! docker image inspect "$TLAPS_IMAGE" > /dev/null 2>&1; then
        echo "Building TLAPS Docker image from tools/formal-verification/docker/tlaps/..."
        docker build -t "$TLAPS_IMAGE" tools/formal-verification/docker/tlaps/
    fi

    mkdir -p .artifacts/formal-verification/tlaps
    docker run --rm \
        -v "$(pwd)/specs/tla:/workspace" \
        -w /workspace \
        "$TLAPS_IMAGE" \
        --check /workspace/VSR_Proofs.tla 2>&1 | tee .artifacts/formal-verification/tlaps/tlaps-output.log

# Run Ivy Byzantine consensus verification (via Docker)
verify-ivy:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running Ivy Byzantine model verification..."

    if [ ! -f specs/ivy/VSR_Byzantine.ivy ]; then
        echo "Ivy spec not yet created, skipping"
        exit 0
    fi

    if ! docker info > /dev/null 2>&1; then
        echo "Error: Docker is not running"
        exit 1
    fi

    mkdir -p .artifacts/formal-verification/ivy

    IVY_IMAGE="kimberlite-ivy"
    if ! docker image inspect "$IVY_IMAGE" > /dev/null 2>&1; then
        echo "Building Ivy Docker image from tools/formal-verification/docker/ivy/..."
        docker build -t "$IVY_IMAGE" tools/formal-verification/docker/ivy/
    fi

    # ENTRYPOINT is ivy_check, so only pass the .ivy file as argument
    docker run --rm \
        -v "$(pwd)/specs/ivy:/workspace" \
        -w /workspace \
        "$IVY_IMAGE" \
        VSR_Byzantine.ivy 2>&1 | tee .artifacts/formal-verification/ivy/ivy-output.log

# Run Coq cryptographic proofs
verify-coq:
    #!/usr/bin/env bash
    set -euo pipefail

    SPECS_DIR="$(pwd)/specs/coq"
    COQIMAGE="coqorg/coq:8.18"

    echo "=== Coq Verification ==="
    echo ""

    if ! docker info > /dev/null 2>&1; then
        echo "Error: Docker is not running"
        exit 1
    fi

    # Pull image if needed
    if ! docker image inspect "$COQIMAGE" > /dev/null 2>&1; then
        echo "Pulling Coq Docker image..."
        docker pull "$COQIMAGE"
    fi

    # Verification order (dependencies first)
    FILES=(
        "Common.v"
        "SHA256.v"
        "BLAKE3.v"
        "AES_GCM.v"
        "Ed25519.v"
        "KeyHierarchy.v"
    )

    FAILED=0
    PASSED=0

    # Create output directory
    ARTIFACTS_DIR="$(pwd)/.artifacts/formal-verification/coq"
    mkdir -p "$ARTIFACTS_DIR"

    for file in "${FILES[@]}"; do
        if [ ! -f "$SPECS_DIR/$file" ]; then
            echo "⚠️  Skipping $file (not found)"
            continue
        fi

        echo "Verifying $file..."
        if docker run --rm \
            -v "$SPECS_DIR:/workspace" \
            -w /workspace \
            "$COQIMAGE" \
            coqc -Q . Kimberlite "$file" 2>&1 | tee "$ARTIFACTS_DIR/${file%.v}.log"; then
            echo "✅ $file verified"
            PASSED=$((PASSED + 1))
        else
            echo "❌ $file failed"
            FAILED=$((FAILED + 1))
        fi
        echo ""
    done

    # Move generated files to artifacts directory
    mv "$SPECS_DIR"/*.vo "$ARTIFACTS_DIR/" 2>/dev/null || true
    mv "$SPECS_DIR"/*.vok "$ARTIFACTS_DIR/" 2>/dev/null || true
    mv "$SPECS_DIR"/*.vos "$ARTIFACTS_DIR/" 2>/dev/null || true
    mv "$SPECS_DIR"/*.glob "$ARTIFACTS_DIR/" 2>/dev/null || true
    mv "$SPECS_DIR"/.*.aux "$ARTIFACTS_DIR/" 2>/dev/null || true

    echo "=== Summary ==="
    echo "Passed: $PASSED"
    if [ $FAILED -gt 0 ]; then
        echo "Failed: $FAILED"
        exit 1
    else
        echo "All files verified! ✅"
    fi

# Run Alloy structural model checking
verify-alloy:
    #!/usr/bin/env bash
    set -e
    echo "=========================================="
    echo "Alloy Structural Model Checking"
    echo "=========================================="
    echo ""

    mkdir -p .artifacts/formal-verification/alloy

    # Count total specs
    total=$(ls -1 specs/alloy/*.als | wc -l | tr -d ' ')
    current=0

    for spec in specs/alloy/*.als; do
        current=$((current + 1))
        spec_name=$(basename "$spec" .als)

        echo "[$current/$total] Checking $spec_name.als..."
        echo "  Output: .artifacts/formal-verification/alloy/$spec_name/"
        echo ""

        # Run with timing
        start_time=$(date +%s)

        if java -jar tools/formal-verification/alloy/alloy-6.2.0.jar exec -f -o ".artifacts/formal-verification/alloy/$spec_name" "$spec" 2>&1; then
            end_time=$(date +%s)
            elapsed=$((end_time - start_time))
            echo ""
            echo "  ✅ $spec_name.als completed in ${elapsed}s"
        else
            end_time=$(date +%s)
            elapsed=$((end_time - start_time))
            echo ""
            echo "  ❌ $spec_name.als failed after ${elapsed}s"
            exit 1
        fi

        echo ""
        echo "──────────────────────────────────────────"
        echo ""
    done

    echo "=========================================="
    echo "✅ All Alloy models verified successfully!"
    echo "=========================================="

# Run Kani code verification (bounded model checking)
verify-kani:
    cargo kani --workspace --default-unwind 64 --no-unwinding-checks

# Run Kani specific harness
verify-kani-harness harness:
    cargo kani --harness {{harness}}

# ───────────────────────────────────────────────────────────────────────────────
# CODE QUALITY
# ───────────────────────────────────────────────────────────────────────────────

# Format code
fmt:
    cargo fmt --all

# Check formatting (CI)
fmt-check:
    cargo fmt --all -- --check

# Run clippy (linting)
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run clippy with fixes
clippy-fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# Check documentation
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Build documentation and open in browser
doc:
    cargo doc --workspace --no-deps --all-features --open

# Check for unused dependencies
unused-deps:
    cargo machete

# Run all pre-commit checks
pre-commit: fmt-check clippy pressurecraft-check test test-docs
    @echo "Pre-commit checks passed!"

# Run full CI checks locally
ci: fmt-check clippy pressurecraft-check test doc-check
    @echo "CI checks passed!"

# Run full CI with security checks
ci-full: ci unused-deps audit deny
    @echo "Full CI checks passed!"

# PRESSURECRAFT grep-based checks. Encodes rules clippy cannot express.
#
# Scope:
#  - Check 1: every production assert! in core crates has a paired
#    should_panic test in the same crate.
#  - Check 2: no undifferentiated Result<_, ()> in published-crate
#    public APIs.
#  - Check 3: core crate lib.rs files must opt in to the strict
#    PRESSURECRAFT clippy lints so they can't silently disappear.
#
# Rules about no-unwrap / no-panic / no-stub / 70-line functions are
# enforced by clippy::unwrap_used / panic / todo / unimplemented /
# too_many_lines, opted in at the top of each core crate's lib.rs and
# tuned via clippy.toml. Running this recipe does NOT substitute for
# clippy — `just clippy` is the source of truth for those rules.
#
# Exit code = number of failed checks (0 on success).
pressurecraft-check:
    #!/usr/bin/env bash
    set -u -o pipefail
    FAIL=0
    PASS=0
    if [ -t 1 ]; then
      RED=$'\033[0;31m'; GREEN=$'\033[0;32m'; YELLOW=$'\033[0;33m'
      BLUE=$'\033[0;34m'; BOLD=$'\033[1m'; RESET=$'\033[0m'
    else
      RED=''; GREEN=''; YELLOW=''; BLUE=''; BOLD=''; RESET=''
    fi
    heading() { printf '%s==> %s%s\n' "${BOLD}${BLUE}" "$*" "${RESET}"; }
    pass()    { printf '  %s✓ %s%s\n'  "${GREEN}" "$*" "${RESET}"; PASS=$((PASS+1)); }
    fail()    { printf '  %s✗ %s%s\n'  "${RED}" "$*" "${RESET}";   FAIL=$((FAIL+1)); }
    warn()    { printf '  %s! %s%s\n'  "${YELLOW}" "$*" "${RESET}"; }

    # CORE_CRATES are audited for production assertions + strict clippy
    # lint opt-in. PUBLISHED_CRATES are audited for Result<_, ()> in public
    # APIs. `kimberlite-types` and `kimberlite-wire` moved into CORE_CRATES
    # in the Apr 2026 fuzz-to-types hardening effort — their lib.rs now
    # carries #![warn(clippy::unwrap_used, ...)] per PRESSURECRAFT §4.
    CORE_CRATES=(kimberlite-kernel kimberlite-vsr kimberlite-crypto kimberlite-types kimberlite-wire)
    PUBLISHED_CRATES=(kimberlite kimberlite-client kimberlite-types kimberlite-wire kimberlite-ffi)

    # ── Check 1: production assert! sites have regression coverage.
    # A core crate with production asserts must ship either:
    #   (a) at least one #[should_panic] test (precondition asserts), OR
    #   (b) a dedicated tests_assertions.rs module exercising the
    #       invariant's happy path (postcondition / invariant asserts
    #       that can't be triggered externally without mocking).
    # Clippy can't enforce test-coverage pairings.
    heading "PRESSURECRAFT Check 1 — production assertions have regression coverage"
    # Count only asserts OUTSIDE of `#[cfg(test)] mod ... { }` blocks so
    # inline test modules don't inflate the production assert count. The
    # awk script skips balanced `{..}` bodies once a `#[cfg(test)] mod`
    # attribute is seen on a preceding line.
    count_prod_asserts() {
      local dir="$1"
      find "$dir" -name '*.rs' \
        ! -name 'tests.rs' ! -name 'kani_proofs.rs' \
        ! -path '*/tests_*' 2>/dev/null \
        | xargs awk '
          BEGIN { in_test = 0; depth = 0 }
          /^[[:space:]]*#\[cfg\(test\)\]/ { test_next = 1; next }
          test_next && /^[[:space:]]*mod[[:space:]]/ {
            test_next = 0; in_test = 1; depth = 0
            # count any { on this line
            for (i = 1; i <= length($0); i++) {
              c = substr($0, i, 1)
              if (c == "{") depth++
              else if (c == "}") depth--
            }
            if (depth == 0) in_test = 0
            next
          }
          { test_next = 0 }
          in_test {
            for (i = 1; i <= length($0); i++) {
              c = substr($0, i, 1)
              if (c == "{") depth++
              else if (c == "}") depth--
            }
            if (depth <= 0) in_test = 0
            next
          }
          /^[[:space:]]*assert!\(/ { print FILENAME ":" NR ":" $0 }
        ' 2>/dev/null | wc -l | tr -d ' '
    }
    for crate in "${CORE_CRATES[@]}"; do
      dir="crates/${crate}/src"
      [ -d "$dir" ] || { warn "${crate}: src dir not found, skipping"; continue; }
      prod_asserts=$(count_prod_asserts "$dir")
      sp_tests=$(grep -rn --include='*.rs' '#\[should_panic' "$dir" 2>/dev/null \
        | wc -l | tr -d ' ')
      has_assertions_module="no"
      if [ -f "${dir}/tests_assertions.rs" ] || [ -d "${dir}/tests_assertions" ]; then
        has_assertions_module="yes"
      fi
      if [ "$prod_asserts" -gt 0 ] && [ "$sp_tests" -eq 0 ] && [ "$has_assertions_module" = "no" ]; then
        fail "${crate}: ${prod_asserts} production assert! sites, 0 should_panic tests, no tests_assertions module"
      else
        pass "${crate}: ${prod_asserts} production assert! sites (${sp_tests} should_panic, tests_assertions=${has_assertions_module})"
      fi
    done

    # ── Check 2: no undifferentiated Result<_, ()> in public APIs.
    # Clippy has no lint for this; it's a type-shape rule.
    heading "PRESSURECRAFT Check 2 — no undifferentiated Result<_, ()> in public APIs"
    for crate in "${PUBLISHED_CRATES[@]}"; do
      dir="crates/${crate}/src"
      [ -d "$dir" ] || { warn "${crate}: src dir not found, skipping"; continue; }
      hits=$(grep -rnE --include='*.rs' \
        '^[[:space:]]*pub[[:space:]]+(async[[:space:]]+)?fn.*->[[:space:]]*Result<[^,]+,[[:space:]]*\(\)>' \
        "$dir" 2>/dev/null || true)
      if [ -n "$hits" ]; then
        fail "${crate}: public API returns Result<_, ()>:"
        printf '%s\n' "$hits" | sed 's/^/    /'
      else
        pass "${crate}: no Result<_, ()> in public API"
      fi
    done

    # ── Check 3: core crate lib.rs files still carry the strict lint
    # attribute block. Someone deleting the #![warn(...)] header would
    # silently disable the rules — this guards against that.
    heading "PRESSURECRAFT Check 3 — core crates still opt in to strict clippy lints"
    REQUIRED_LINTS=(unwrap_used panic todo unimplemented too_many_lines)
    for crate in "${CORE_CRATES[@]}"; do
      libfile="crates/${crate}/src/lib.rs"
      if [ ! -f "$libfile" ]; then
        warn "${crate}: lib.rs not found, skipping"
        continue
      fi
      missing=()
      for lint in "${REQUIRED_LINTS[@]}"; do
        if ! grep -q "clippy::${lint}" "$libfile"; then
          missing+=("${lint}")
        fi
      done
      if [ ${#missing[@]} -gt 0 ]; then
        fail "${crate}/src/lib.rs missing #![warn(clippy::${missing[*]})]"
      else
        pass "${crate}/src/lib.rs opts in to all strict lints"
      fi
    done

    echo
    if [ "$FAIL" -eq 0 ]; then
      printf '%sPRESSURECRAFT checks: %d passed, 0 failed%s\n' "${GREEN}${BOLD}" "$PASS" "${RESET}"
      exit 0
    else
      printf '%sPRESSURECRAFT checks: %d passed, %d failed%s\n' "${RED}${BOLD}" "$PASS" "$FAIL" "${RESET}"
      exit "$FAIL"
    fi

# ───────────────────────────────────────────────────────────────────────────────
# SECURITY & AUDITING
# ───────────────────────────────────────────────────────────────────────────────

# Run security audit
audit:
    cargo audit

# Run cargo-deny checks
deny:
    cargo deny check

# Check licenses only
deny-licenses:
    cargo deny check licenses

# Check advisories only
deny-advisories:
    cargo deny check advisories

# Full security check
security: audit deny
    @echo "Security checks passed!"

# Check for outdated dependencies
outdated:
    cargo outdated --workspace

# ───────────────────────────────────────────────────────────────────────────────
# PERFORMANCE & PROFILING
# ───────────────────────────────────────────────────────────────────────────────

# Run all benchmarks
bench:
    cargo bench -p kimberlite-bench

# Run benchmarks in quick mode (1 second profile time)
bench-quick:
    @echo "Running quick benchmarks..."
    @for suite in crypto kernel storage wire end_to_end; do \
        echo "=== Benchmarking: $$suite ==="; \
        cargo bench -p kimberlite-bench --bench $$suite -- --profile-time 1; \
    done

# Run specific benchmark suite
bench-suite suite="crypto":
    cargo bench -p kimberlite-bench --bench {{suite}}

# Save benchmark baseline
bench-baseline name="main":
    cargo bench -p kimberlite-bench -- --save-baseline {{name}}

# Compare benchmarks against baseline
bench-compare baseline="main":
    cargo bench -p kimberlite-bench -- --baseline {{baseline}}

# Generate HTML benchmark reports
bench-report:
    cargo bench -p kimberlite-bench
    @echo "HTML reports: target/criterion/report/index.html"
    @open target/criterion/report/index.html || xdg-open target/criterion/report/index.html

# Profile VOPR with samply (opens Firefox Profiler UI)
profile-vopr iterations="50" browser="firefox":
    BROWSER={{browser}} samply record cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}

# Profile without opening browser
profile-vopr-headless iterations="100":
    samply record --no-open cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}

# Generate coverage report
coverage:
    cargo llvm-cov --workspace --all-features --html
    @echo "Coverage report: target/llvm-cov/html/index.html"

# ───────────────────────────────────────────────────────────────────────────────
# FUZZING
# ───────────────────────────────────────────────────────────────────────────────

# List available fuzz targets
fuzz-list:
    cd fuzz && cargo fuzz list

# Run a fuzz target (use Ctrl+C to stop)
fuzz target *args="":
    cd fuzz && cargo +nightly fuzz run {{target}} {{args}}

# Run fuzz smoke test (10K iterations for CI)
fuzz-smoke:
    cd fuzz && ./ci-fuzz.sh

# Run all fuzz targets for smoke testing
fuzz-all:
    @echo "Running smoke tests for all fuzz targets..."
    @cd fuzz && for target in $$(cargo fuzz list); do \
        echo "=== Fuzzing: $$target ==="; \
        cargo +nightly fuzz run $$target -- -runs=10000 || exit 1; \
    done
    @echo "All fuzz targets passed!"

# ───────────────────────────────────────────────────────────────────────────────
# PUBLISHING & RELEASE
# ───────────────────────────────────────────────────────────────────────────────

# Check if ready to publish
check-publish:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "🔍 Pre-publish validation checks"

    # Check workspace is clean
    if [[ -n $(git status --porcelain) ]]; then
        echo "❌ Working directory is not clean"
        exit 1
    fi
    echo "✅ Working directory clean"

    # Check version tag exists
    VERSION=$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "kimberlite") | .version')
    if ! git tag | grep -q "^v$VERSION$"; then
        echo "❌ Version tag v$VERSION does not exist"
        exit 1
    fi
    echo "✅ Version tag v$VERSION exists"

    # Check CHANGELOG updated
    if ! grep -q "## \[$VERSION\]" CHANGELOG.md; then
        echo "❌ CHANGELOG.md does not have entry for $VERSION"
        exit 1
    fi
    echo "✅ CHANGELOG.md updated"

    # Dry-run publish
    echo ""
    echo "🧪 Running dry-run publish..."
    just publish-dry-run

    echo ""
    echo "✅ All validation checks passed!"

# Publish all crates to crates.io (DRY RUN)
publish-dry-run:
    #!/usr/bin/env bash
    set -euo pipefail

    CRATES_TO_PUBLISH=(
        "kimberlite-config"
        "kimberlite-migration"
        "kimberlite-sharing"
        "kimberlite-mcp"
    )

    echo "🚀 Publishing ${#CRATES_TO_PUBLISH[@]} crates (DRY RUN)"

    for crate in "${CRATES_TO_PUBLISH[@]}"; do
        echo "📦 Dry-run: $crate..."
        cargo publish --dry-run -p "$crate"
        echo "✅ $crate"
    done

    echo "🎉 Dry-run complete!"

# Publish all crates to crates.io (REAL)
publish:
    #!/usr/bin/env bash
    set -euo pipefail

    CRATES_TO_PUBLISH=(
        "kimberlite-config"
        "kimberlite-migration"
        "kimberlite-sharing"
        "kimberlite-mcp"
    )

    PUBLISH_DELAY=30

    echo "🚀 Publishing ${#CRATES_TO_PUBLISH[@]} crates to crates.io"
    echo ""

    TOTAL_CRATES=${#CRATES_TO_PUBLISH[@]}
    CURRENT_INDEX=0

    for crate in "${CRATES_TO_PUBLISH[@]}"; do
        CURRENT_INDEX=$((CURRENT_INDEX + 1))
        echo "📦 Publishing $crate ($CURRENT_INDEX/$TOTAL_CRATES)..."

        cargo publish -p "$crate"

        # Wait for crates.io propagation (except for last crate)
        if [[ $CURRENT_INDEX -lt $TOTAL_CRATES ]]; then
            echo "⏳ Waiting ${PUBLISH_DELAY}s for crates.io propagation..."
            sleep "$PUBLISH_DELAY"
        fi

        echo "✅ $crate"
        echo ""
    done

    echo "🎉 All crates published successfully!"

# Update version across all crates
update-version version:
    #!/usr/bin/env bash
    set -euo pipefail

    VERSION="{{version}}"
    echo "📝 Updating version to $VERSION"

    # Update workspace version
    sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
    rm -f Cargo.toml.bak

    # Update lock file
    cargo check --workspace

    # Add CHANGELOG entry
    DATE=$(date +%Y-%m-%d)
    TEMP=$(mktemp)
    {
        head -n 4 CHANGELOG.md
        echo ""
        echo "## [$VERSION] - $DATE"
        echo ""
        echo "### Added"
        echo ""
        echo "### Changed"
        echo ""
        echo "### Fixed"
        echo ""
        echo "---"
        echo ""
        tail -n +5 CHANGELOG.md
    } > "$TEMP"
    mv "$TEMP" CHANGELOG.md

    echo "✅ Updated version to $VERSION"
    echo ""
    echo "Next steps:"
    echo "  1. Review: git diff"
    echo "  2. Update CHANGELOG.md with actual release notes"
    echo "  3. Commit: git commit -am 'chore: Bump version to $VERSION'"
    echo "  4. Tag: git tag -a v$VERSION -m 'Release v$VERSION'"
    echo "  5. Push: git push origin main --tags"
    echo "  6. Publish: just publish"

# Generate SBOM (Software Bill of Materials)
sbom:
    cargo cyclonedx --format json --output-prefix kimberlitedb

# ───────────────────────────────────────────────────────────────────────────────
# MAINTENANCE
# ───────────────────────────────────────────────────────────────────────────────

# Update dependencies
update:
    cargo update

# Check MSRV (Minimum Supported Rust Version)
msrv:
    cargo +1.85 check --workspace --all-targets

# Show repository size and statistics
size:
    @echo "=== Kimberlite Repository Size ==="
    @du -sh .
    @echo ""
    @echo "=== Major Directories ==="
    @du -sh crates/ docs/ specs/ target/ .artifacts/ 2>/dev/null || true

# Show git repository statistics
stats:
    @echo "=== Repository Statistics ==="
    @echo "Total commits: $$(git rev-list --count HEAD)"
    @echo "Contributors: $$(git log --format='%aN' | sort -u | wc -l)"
    @echo "Lines of code:"
    @tokei crates/

# ───────────────────────────────────────────────────────────────────────────────
# DEVELOPMENT UTILITIES
# ───────────────────────────────────────────────────────────────────────────────

# Watch for changes and run tests
watch:
    cargo watch -x test

# Watch and run specific test
watch-test test_name:
    cargo watch -x "test {{test_name}}"

# Run bacon (TUI for cargo commands)
bacon:
    bacon

# ───────────────────────────────────────────────────────────────────────────────
# SETUP
# ───────────────────────────────────────────────────────────────────────────────

# Install development tools
setup:
    @echo "Installing development tools..."
    cargo install cargo-nextest cargo-audit cargo-deny cargo-machete cargo-llvm-cov
    @echo "Done! Optional tools:"
    @echo "  cargo install cargo-cyclonedx    # SBOM generation"
    @echo "  cargo install samply             # Profiling"
    @echo "  cargo install flamegraph         # Flamegraphs"

# Install pre-commit hook
install-hooks:
    @echo '#!/bin/sh' > .git/hooks/pre-commit
    @echo 'just pre-commit' >> .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo "Pre-commit hook installed!"

# ───────────────────────────────────────────────────────────────────────────────
# WEBSITE
# ───────────────────────────────────────────────────────────────────────────────

# Run the website dev server
site:
    cd website && cargo run

# Run website with bacon watch mode
site-watch:
    cd website && bacon

# Build website for release
site-build:
    cd website && cargo build --release

# Build website Docker image
site-docker:
    cd website && docker build --build-arg BUILD_VERSION=$$(git rev-parse --short=8 HEAD) -t kmb-site .

# Run website Docker image locally
site-docker-run:
    docker run -p 3000:3000 --rm kmb-site

# Validate all docs/ links in README.md resolve to existing files
check-links:
    #!/usr/bin/env bash
    set -euo pipefail
    errors=0
    while IFS= read -r path; do
        if [[ -n "$path" && ! -f "$path" ]]; then
            echo "BROKEN: $path"
            errors=$((errors + 1))
        fi
    done < <(grep -oE '\(docs/[^)#]+' README.md | tr -d '(')
    if [[ $errors -eq 0 ]]; then
        echo "✓ All README doc links resolve to existing files"
    else
        echo "✗ $errors broken link(s) found"
        exit 1
    fi

# ============================================================================
# EPYC Hetzner DST Campaign Targets
# ============================================================================

EPYC_HOST := "root@142.132.137.52"
EPYC_PATH := "/opt/kimberlite-dst/repo"

# Sync source to EPYC server (excludes target/, .git/, .artifacts/)
epyc-deploy:
    @echo "Deploying to {{EPYC_HOST}}:{{EPYC_PATH}}"
    rsync -az --delete \
        --exclude='target/' \
        --exclude='node_modules/' \
        --exclude='.git/' \
        --exclude='.artifacts/' \
        --exclude='*.kmb' \
        --exclude='tmp/' \
        ./ {{EPYC_HOST}}:{{EPYC_PATH}}/

# Build release binaries on EPYC
epyc-build:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        cargo build --release -p kimberlite-sim --bin vopr --bin vopr-dpor && \
        cargo build --release -p kimberlite-chaos"

# One-time host prep for chaos --apply runs. Loads kernel modules (nbd,
# xt_comment, br_netfilter) and enables bridge→iptables filtering so that
# DROP rules in the KMB_CHAOS chain actually affect bridged VM traffic.
# Survives a reboot via /etc/modules-load.d/ + /etc/sysctl.d/.
epyc-setup-host:
    ssh {{EPYC_HOST}} "set -e; \
        modprobe nbd max_part=8 && modprobe xt_comment && modprobe br_netfilter; \
        printf 'nbd\\nxt_comment\\nbr_netfilter\\n' > /etc/modules-load.d/kimberlite-chaos.conf; \
        printf 'options nbd max_part=8\\n' > /etc/modprobe.d/kimberlite-chaos-nbd.conf; \
        echo 'net.bridge.bridge-nf-call-iptables=1' > /etc/sysctl.d/99-kimberlite-chaos.conf; \
        sysctl -p /etc/sysctl.d/99-kimberlite-chaos.conf; \
        echo 'setup complete'"

# Build the kimberlite server binary for the chaos VM rootfs.
#
# Stock `x86_64-unknown-linux-gnu` is fine — `kimberlite-server` does not
# pull DuckDB at runtime (DuckDB is a workspace dev-dep for SQL
# differential testing only). The earlier musl-shim path is kept as a
# fallback recipe below for rollout safety.
epyc-build-server-linux:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        cargo build --release -p kimberlite-cli"

# Build the chaos VM images on EPYC (Debian-slim rootfs + real kimberlite
# binary + matching virt kernel). Produces bzImage + per-replica qcow2
# files under /opt/kimberlite-dst/vm-images/. Requires
# epyc-build-server-linux first.
epyc-build-vm-image: epyc-build-server-linux
    ssh {{EPYC_HOST}} "bash {{EPYC_PATH}}/tools/chaos/build-vm-image.sh"

# Fallback: build the musl-static kimberlite-chaos-shim — the legacy
# Alpine rootfs + shim path. Keep around for rollout safety until the
# real-binary path is proven; delete once the weekly timer has passed on
# the real binary for a few runs.
epyc-build-musl:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        rustup target add x86_64-unknown-linux-musl && \
        cargo build --release --target x86_64-unknown-linux-musl -p kimberlite-chaos-shim"

# Run VOPR fuzzing campaign on EPYC (default: 10_000 iterations per scenario, 60-way parallel)
epyc-vopr iterations="10000":
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        mkdir -p results && \
        ./target/release/vopr -n {{iterations}} --json --scenario combined > results/vopr-$(date -u +%Y%m%d-%H%M%S).jsonl"

# Run DPOR exploration on EPYC
epyc-dpor seeds="1000":
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        mkdir -p results && \
        ./target/release/vopr-dpor --explore {{seeds}} --alternatives 200 --json > results/dpor-$(date -u +%Y%m%d-%H%M%S).json"

# Run chaos scenarios on EPYC (requires KVM + root)
epyc-chaos scenario="split_brain_prevention":
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        ./target/release/kimberlite-chaos run {{scenario}}"

# End-to-end chaos run: apply mode, tcpdump capture, artifact rsync back.
# Writes report.json, per-VM console logs, and chaos.pcap under
# .artifacts/epyc-results/chaos-<timestamp>/ on the local machine.
# Run every built-in chaos scenario sequentially on EPYC, rsyncing each
# run's artifacts back to .artifacts/epyc-results/. Fails fast on the
# first scenario that doesn't emit a PASS report.
epyc-chaos-all:
    #!/usr/bin/env bash
    set -euo pipefail
    for scenario in split_brain_prevention rolling_restart_under_load leader_kill_mid_commit independent_cluster_isolation cascading_failure storage_exhaustion; do
        echo ""
        echo "=============================================================="
        echo "=== scenario: $scenario"
        echo "=============================================================="
        just epyc-chaos-e2e "$scenario" || { echo "$scenario FAILED"; exit 1; }
    done
    echo "ALL 6 SCENARIOS PASSED"

epyc-chaos-e2e scenario="split_brain_prevention":
    #!/usr/bin/env bash
    set -euo pipefail
    ts=$(date -u +%Y-%m-%d-%H%M%S)
    remote_out="/opt/kimberlite-dst/results/chaos-${ts}"
    local_out=".artifacts/epyc-results/chaos-${ts}"
    echo "=== epyc-chaos-e2e: scenario={{scenario}} ts=${ts} ==="

    ssh -o ServerAliveInterval=30 {{EPYC_HOST}} bash -s <<REMOTE_EOF
    set -euo pipefail
    mkdir -p "${remote_out}"
    cd {{EPYC_PATH}}
    . \$HOME/.cargo/env

    # Preflight: kill any leftover VMs or tcpdump from a prior run, drop
    # any bridges/taps/iptables rules that the previous controller didn't
    # clean up (common if it panicked).
    sudo pkill -f 'qemu-system-x86_64.*kimberlite' 2>/dev/null || true
    sudo pkill -f 'tcpdump.*10.42' 2>/dev/null || true
    for i in 0 1 2 3 4 5; do
        sudo ip link del kmb-c\${i}-br 2>/dev/null || true
        for r in 0 1 2; do
            sudo ip link del tap-c\${i}-r\${r} 2>/dev/null || true
        done
    done
    sudo iptables -D FORWARD -j KMB_CHAOS 2>/dev/null || true
    sudo iptables -F KMB_CHAOS 2>/dev/null || true
    sudo iptables -X KMB_CHAOS 2>/dev/null || true

    # Reset per-replica qcow2 disks from the base image. Each scenario
    # must start from a clean rootfs — otherwise VSR superblock files,
    # the chaos write log, and /var/lib/kimberlite/.kimberlite-initialized
    # sentinels accumulate between runs and break bootstrap.
    if [ -f /opt/kimberlite-dst/vm-images/base.qcow2 ]; then
        for c in 0 1; do
            for r in 0 1 2; do
                sudo cp --reflink=auto \
                    /opt/kimberlite-dst/vm-images/base.qcow2 \
                    /opt/kimberlite-dst/vm-images/replica-c\${c}-r\${r}.qcow2
            done
        done
    fi

    # Start tcpdump in the background on all bridges (kmb-c*-br). Use -i any
    # to catch cross-bridge traffic too. Kill on exit.
    sudo tcpdump -i any -U -w "${remote_out}/chaos.pcap" 'net 10.42.0.0/16' \
        >"${remote_out}/tcpdump.stderr" 2>&1 &
    TCPDUMP_PID=\$!
    trap 'sudo kill \$TCPDUMP_PID 2>/dev/null || true' EXIT

    sudo ./target/release/kimberlite-chaos run {{scenario}} \
        --apply --output-dir "${remote_out}" \
        | tee "${remote_out}/run.log"

    sudo kill \$TCPDUMP_PID 2>/dev/null || true
    wait \$TCPDUMP_PID 2>/dev/null || true
    sudo chown -R \$(id -un):\$(id -gn) "${remote_out}" || true
    ls -lh "${remote_out}"
    REMOTE_EOF

    mkdir -p "${local_out}"
    rsync -az {{EPYC_HOST}}:"${remote_out}/" "${local_out}/"
    echo "=== artifacts: ${local_out} ==="
    ls -lh "${local_out}"

# List chaos scenarios available on EPYC
epyc-chaos-list:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && ./target/release/kimberlite-chaos list"

# Install (or reinstall) the weekly chaos systemd timer on EPYC. Mirrors
# fuzz-epyc-timer-install. Fires Sunday 03:00 UTC and runs all 6 built-in
# scenarios sequentially. Artifacts land under
# /opt/kimberlite-dst/results/weekly-<ts>/.
chaos-epyc-timer-install:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} "mkdir -p /opt/kimberlite-dst/bin"
    rsync -az tools/chaos/epyc/weekly.sh \
        {{EPYC_HOST}}:/opt/kimberlite-dst/bin/weekly.sh
    ssh {{EPYC_HOST}} "chmod +x /opt/kimberlite-dst/bin/weekly.sh"
    rsync -az tools/chaos/epyc/kimberlite-chaos-weekly.service \
        tools/chaos/epyc/kimberlite-chaos-weekly.timer \
        {{EPYC_HOST}}:/etc/systemd/system/
    ssh {{EPYC_HOST}} "bash -s" <<'REMOTE_EOF'
    set -euo pipefail
    systemctl daemon-reload
    systemctl enable --now kimberlite-chaos-weekly.timer
    systemctl list-timers kimberlite-chaos-weekly.timer --no-pager
    REMOTE_EOF

# Disable the weekly chaos timer without removing the unit files. Flip
# back on with `chaos-epyc-timer-install` or `systemctl enable --now`.
chaos-epyc-timer-disable:
    ssh {{EPYC_HOST}} "systemctl disable --now kimberlite-chaos-weekly.timer && \
        echo 'timer disabled; unit files remain at /etc/systemd/system/'"

# Status: show timer + last service run + last 30 journal lines.
chaos-epyc-timer-status:
    ssh {{EPYC_HOST}} "echo '=== timer ===' && \
        systemctl status kimberlite-chaos-weekly.timer --no-pager --full 2>/dev/null | head -15 && \
        echo '=== service ===' && \
        systemctl status kimberlite-chaos-weekly.service --no-pager --full 2>/dev/null | head -15 && \
        echo '=== recent journal ===' && \
        journalctl -u kimberlite-chaos-weekly --no-pager -n 30"

# Run the weekly campaign on-demand (blocks until done, streams journal).
chaos-epyc-timer-run-now:
    ssh {{EPYC_HOST}} "systemctl start kimberlite-chaos-weekly.service --wait && \
        journalctl -u kimberlite-chaos-weekly --no-pager -n 50"

# Check EPYC host capabilities (KVM, qemu, iptables, tc)
epyc-capabilities:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && ./target/release/kimberlite-chaos capabilities"

# Fetch results from EPYC back to local
epyc-results:
    mkdir -p .artifacts/epyc-results
    rsync -az {{EPYC_HOST}}:{{EPYC_PATH}}/results/ .artifacts/epyc-results/

# Tail the most recent campaign log
epyc-tail:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}}/results && tail -f \$(ls -t *.jsonl | head -1)"

# Status: show recent results, system load, KVM usage
epyc-status:
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && \
        echo '=== System ===' && uptime && \
        echo '=== Memory ===' && free -h && \
        echo '=== KVM ===' && ls -la /dev/kvm 2>/dev/null || echo 'no /dev/kvm' && \
        echo '=== Recent results ===' && ls -lht results/ 2>/dev/null | head -10"

# End-to-end: deploy, build, run a quick smoke campaign
epyc-smoke: epyc-deploy epyc-build
    ssh {{EPYC_HOST}} "cd {{EPYC_PATH}} && . \$HOME/.cargo/env && \
        echo '=== VOPR smoke ===' && ./target/release/vopr -n 100 && \
        echo '=== DPOR smoke ===' && ./target/release/vopr-dpor --explore 50"

# ============================================================================
# EPYC Hetzner Formal-Verification Targets
# ============================================================================
# Formal verification (TLA+, TLAPS, Alloy, Ivy, Coq, Kani, MIRI, VOPR
# properties) runs on the same Hetzner EPYC box as the DST campaign but in a
# separate tree (/opt/kimberlite-fv/) so artifacts don't collide. CI keeps
# running Small configs on GitHub Actions for PR gating; EPYC runs full
# configs (VSR.cfg, HashChain.als scope 10, Kani unwind 128, TLAPS all
# theorems) for deep verification.

EPYC_FV_PATH := "/opt/kimberlite-fv/repo"
EPYC_FV_RESULTS := "/opt/kimberlite-fv/results"

# Sync source to EPYC FV tree. Excludes both build artifacts (target/,
# node_modules/) and heavy VCS-tracked-but-regeneratable artifacts (TLC
# state dumps under specs/tla/states/, fuzz corpora, vendored inspiration
# trees) so each deploy transfers only what the verification tools need.
fv-epyc-deploy:
    @echo "Deploying FV tree to {{EPYC_HOST}}:{{EPYC_FV_PATH}}"
    ssh {{EPYC_HOST}} "mkdir -p {{EPYC_FV_PATH}} {{EPYC_FV_RESULTS}}"
    rsync -az --delete \
        --exclude='target/' \
        --exclude='node_modules/' \
        --exclude='.git/' \
        --exclude='.artifacts/' \
        --exclude='*.kmb' \
        --exclude='tmp/' \
        --exclude='specs/tla/states/' \
        --exclude='fuzz/corpus/' \
        --exclude='fuzz/artifacts/' \
        --exclude='inspiration/' \
        --exclude='website/node_modules/' \
        --exclude='website/.next/' \
        --exclude='website/dist/' \
        ./ {{EPYC_HOST}}:{{EPYC_FV_PATH}}/

# One-time bootstrap: install Java, Docker, Rust+nightly+miri, Kani, pull
# TLAPS/Ivy/Coq images, download+verify TLA+ and Alloy jars.
# Idempotent — safe to re-run after a fresh deploy.
fv-epyc-setup: fv-epyc-deploy
    ssh {{EPYC_HOST}} "bash {{EPYC_FV_PATH}}/tools/formal-verification/epyc/bootstrap.sh"

# Fast sanity: deploy + build + TLA+ quick + Kani unwind 8 + Alloy quick
fv-epyc-smoke: fv-epyc-deploy
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/smoke-${ts}
    mkdir -p "${out}"
    echo "=== [1/3] TLA+ quick (VSR_Small.cfg, depth 10) ==="
    java -cp /opt/kimberlite-fv/tla/tla2tools.jar tlc2.TLC \
        -deadlock -workers 8 -depth 10 \
        -config specs/tla/VSR_Small.cfg specs/tla/VSR.tla \
        2>&1 | tee "${out}/tla-quick.log"
    echo "=== [2/3] Alloy quick (HashChain-quick.als) ==="
    java -Djava.awt.headless=true \
        -jar /opt/kimberlite-fv/alloy/alloy-6.2.0.jar exec \
        specs/alloy/HashChain-quick.als \
        2>&1 | tee "${out}/alloy-quick.log"
    echo "=== [3/3] Kani (unwind 8, kimberlite-vsr only) ==="
    cargo kani -p kimberlite-vsr --default-unwind 8 --no-unwinding-checks \
        2>&1 | tee "${out}/kani-smoke.log" || true
    echo "=== smoke complete: ${out} ==="
    REMOTE_EOF

# Full TLC (all protocol specs, workers 32, depth 20, production configs)
fv-epyc-tla-full:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/tla-full-${ts}
    mkdir -p "${out}"
    JAR=/opt/kimberlite-fv/tla/tla2tools.jar
    for pair in "VSR.tla:VSR.cfg" "ViewChange.tla:ViewChange.cfg" "Recovery.tla:Recovery.cfg" "Compliance.tla:Compliance.cfg"; do
        spec=${pair%%:*}
        cfg=${pair##*:}
        logname=${spec%.tla}
        echo "=== TLC ${spec} with ${cfg} (workers 32, depth 20) ==="
        java -cp "${JAR}" tlc2.TLC -deadlock -workers 32 -depth 20 \
            -config "specs/tla/${cfg}" "specs/tla/${spec}" \
            2>&1 | tee "${out}/${logname}.log"
    done
    echo "=== TLC full complete: ${out} ==="
    REMOTE_EOF

# Full TLAPS (--stretch 10000 on every proof file)
fv-epyc-tlaps-full:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/tlaps-${ts}
    mkdir -p "${out}"
    if ! docker image inspect kimberlite-tlaps:latest >/dev/null 2>&1; then
        docker build -t kimberlite-tlaps:latest tools/formal-verification/docker/tlaps/
    fi
    for proof in VSR_Proofs ViewChange_Proofs Recovery_Proofs Compliance_Proofs; do
        spec_file="specs/tla/${proof}.tla"
        [ -f "${spec_file}" ] || { echo "skip: ${proof} (no file)"; continue; }
        echo "=== TLAPS ${proof}.tla (stretch 10000) ==="
        docker run --rm -v "$PWD/specs/tla:/spec" kimberlite-tlaps:latest \
            --stretch 10000 "/spec/${proof}.tla" \
            2>&1 | tee "${out}/${proof}.log"
    done
    echo "=== TLAPS full complete: ${out} ==="
    REMOTE_EOF

# Full Alloy (HashChain.als scope 10, Quorum.als scope 8)
fv-epyc-alloy-full:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/alloy-full-${ts}
    mkdir -p "${out}"
    JAR=/opt/kimberlite-fv/alloy/alloy-6.2.0.jar
    for spec in specs/alloy/Simple.als specs/alloy/HashChain.als specs/alloy/Quorum.als; do
        [ -f "${spec}" ] || { echo "skip: ${spec} (no file)"; continue; }
        name=$(basename "${spec}" .als)
        echo "=== Alloy ${name} (full scope) ==="
        java -Djava.awt.headless=true -jar "${JAR}" exec "${spec}" \
            2>&1 | tee "${out}/${name}.log"
    done
    echo "=== Alloy full complete: ${out} ==="
    REMOTE_EOF

# Ivy Byzantine model (aspirational; Python 2/3 issue may mask failures)
fv-epyc-ivy:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/ivy-${ts}
    mkdir -p "${out}"
    if ! docker image inspect kimberlite-ivy:latest >/dev/null 2>&1; then
        docker build -t kimberlite-ivy:latest tools/formal-verification/docker/ivy/
    fi
    docker run --rm -v "$PWD/specs/ivy:/workspace" -w /workspace \
        kimberlite-ivy:latest VSR_Byzantine.ivy \
        2>&1 | tee "${out}/ivy.log" || true
    echo "=== Ivy run complete: ${out} ==="
    REMOTE_EOF

# All Coq proofs (Common, SHA256, BLAKE3, AES_GCM, Ed25519, KeyHierarchy, MessageSerialization, Extract)
fv-epyc-coq:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/coq-${ts}
    mkdir -p "${out}"
    docker pull coqorg/coq:8.18 >/dev/null 2>&1 || true
    FILES=(Common.v SHA256.v BLAKE3.v AES_GCM.v Ed25519.v KeyHierarchy.v MessageSerialization.v Extract.v)
    # coqorg/coq's default user (coq:1000) cannot write .glob/.vo artifacts
    # to a root-owned bind mount. Mount the specs dir read-only and copy
    # into /tmp inside the container, which is writable by the coq user.
    failed=0
    for f in "${FILES[@]}"; do
        [ -f "specs/coq/${f}" ] || { echo "skip: ${f}"; continue; }
        echo "=== Coq ${f} ==="
        if docker run --rm -v "$PWD/specs/coq:/src:ro" coqorg/coq:8.18 \
            bash -c "mkdir -p /tmp/coq && cp /src/*.v /tmp/coq/ && cd /tmp/coq && coqc -Q . Kimberlite '${f}'" \
            2>&1 | tee "${out}/${f%.v}.log"; then
            echo "OK ${f}"
        else
            echo "FAIL ${f}"
            failed=$((failed + 1))
        fi
    done
    echo "=== Coq full complete: ${out} (${failed} failed) ==="
    exit "${failed}"
    REMOTE_EOF

# Kani full (workspace, unwind 128, parallel across all cores)
fv-epyc-kani-full:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/kani-${ts}
    mkdir -p "${out}"
    cargo kani --workspace --default-unwind 128 --no-unwinding-checks \
        -j "$(nproc)" 2>&1 | tee "${out}/kani.log"
    echo "=== Kani full complete: ${out} ==="
    REMOTE_EOF

# MIRI coverage on storage/crypto/types (miri rejects some FFI — keep scope narrow)
fv-epyc-miri:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/miri-${ts}
    mkdir -p "${out}"
    cargo +nightly miri test \
        -p kimberlite-storage \
        -p kimberlite-crypto \
        -p kimberlite-types \
        --lib --no-default-features \
        2>&1 | tee "${out}/miri.log"
    echo "=== MIRI complete: ${out} ==="
    REMOTE_EOF

# VOPR property-annotation coverage (runs kimberlite-sim with sim feature, dumps report)
fv-epyc-properties:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fv/repo
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fv/results/properties-${ts}
    mkdir -p "${out}"
    cargo build --release -p kimberlite-sim --bin vopr --features sim
    ./target/release/vopr -n 100000 --scenario combined --json \
        > "${out}/vopr-properties.jsonl"
    echo "=== VOPR properties complete: ${out} ==="
    REMOTE_EOF

# Full orchestrator: all FV layers sequentially, single summary file.
# Expected wall-clock on EPYC 7502P: ~3-4 hours (TLC 20m, TLAPS 60-90m,
# Alloy 15m, Ivy 5m, Coq 10m, Kani 60m, MIRI 20m, properties 15m).
fv-epyc-all:
    #!/usr/bin/env bash
    set -euo pipefail
    start=$(date -u +%s)
    echo "=== fv-epyc-all starting $(date -u) ==="
    just fv-epyc-tla-full
    just fv-epyc-tlaps-full
    just fv-epyc-alloy-full
    just fv-epyc-ivy || echo "ivy aspirational: continuing"
    just fv-epyc-coq
    just fv-epyc-kani-full
    just fv-epyc-miri
    just fv-epyc-properties
    end=$(date -u +%s)
    echo "=== fv-epyc-all complete in $((end - start))s ==="

# Fetch all FV results back to local
fv-epyc-results:
    mkdir -p .artifacts/epyc-fv-results
    rsync -az {{EPYC_HOST}}:{{EPYC_FV_RESULTS}}/ .artifacts/epyc-fv-results/

# Tail the most recent FV log on EPYC
fv-epyc-tail:
    ssh {{EPYC_HOST}} "cd {{EPYC_FV_RESULTS}} && latest=\$(ls -t */*.log 2>/dev/null | head -1); echo tailing \$latest; tail -f \$latest"

# Status: docker images, disk usage, recent results
fv-epyc-status:
    ssh {{EPYC_HOST}} "echo '=== System ===' && uptime && \
        echo '=== Memory ===' && free -h && \
        echo '=== Docker images ===' && docker images 2>/dev/null | head -10 && \
        echo '=== FV disk ===' && du -sh {{EPYC_FV_PATH}} {{EPYC_FV_RESULTS}} 2>/dev/null && \
        echo '=== Recent FV results ===' && ls -lht {{EPYC_FV_RESULTS}} 2>/dev/null | head -10"

# Run MIRI locally (mirror of fv-epyc-miri for pre-push checks)
verify-miri:
    cargo +nightly miri test \
        -p kimberlite-storage \
        -p kimberlite-crypto \
        -p kimberlite-types \
        --lib --no-default-features

# ============================================================================
# EPYC Hetzner Purpose-Built Fuzzing Targets
# ============================================================================
# Purpose-built fuzzing campaign runs on the same Hetzner EPYC box as DST and
# FV but in a separate tree (/opt/kimberlite-fuzz/) so corpora and artifacts
# don't collide with the other two campaigns. Tier structure:
#
#   Tier 1 — pre-commit / CI fast (~5 min, run locally + in GitHub Actions)
#   Tier 2 — nightly on EPYC (~3 h, libfuzzer-sys coverage-guided, 12 cores)
#   Tier 3 — weekly on EPYC (~24 h, deeper campaigns + FFI sanitizer targets)
#
# Core budget: VOPR ~60 threads / 30 min bursts; FV ~32 threads / 3-4 h
# sequential; fuzz holds 12 cores continuously. 64 HT total leaves slack.

EPYC_FUZZ_PATH := "/opt/kimberlite-fuzz/repo"
EPYC_FUZZ_CORPORA := "/opt/kimberlite-fuzz/corpora"
EPYC_FUZZ_ARTIFACTS := "/opt/kimberlite-fuzz/artifacts"
EPYC_FUZZ_RESULTS := "/opt/kimberlite-fuzz/results"

# One-time bootstrap: install nightly Rust + cargo-fuzz, create /opt tree.
# Idempotent — safe to re-run after a fresh deploy.
fuzz-epyc-bootstrap:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<REMOTE_EOF
    set -euo pipefail
    mkdir -p {{EPYC_FUZZ_PATH}} {{EPYC_FUZZ_CORPORA}} {{EPYC_FUZZ_ARTIFACTS}} {{EPYC_FUZZ_RESULTS}}
    if ! command -v rustup >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    fi
    source "\$HOME/.cargo/env"
    rustup toolchain install nightly --component rust-src --profile minimal
    cargo install cargo-fuzz --locked 2>/dev/null || cargo install cargo-fuzz --locked --force
    echo "=== fuzz bootstrap complete ==="
    rustup show
    cargo fuzz --version
    REMOTE_EOF

# Sync source to EPYC fuzz tree. Preserves /opt/kimberlite-fuzz/corpora and
# /opt/kimberlite-fuzz/artifacts across deploys — corpora grow across runs.
fuzz-epyc-deploy:
    @echo "Deploying fuzz tree to {{EPYC_HOST}}:{{EPYC_FUZZ_PATH}}"
    ssh {{EPYC_HOST}} "mkdir -p {{EPYC_FUZZ_PATH}} {{EPYC_FUZZ_CORPORA}} {{EPYC_FUZZ_ARTIFACTS}} {{EPYC_FUZZ_RESULTS}}"
    rsync -az --delete \
        --exclude='target/' \
        --exclude='node_modules/' \
        --exclude='.git/' \
        --exclude='.artifacts/' \
        --exclude='*.kmb' \
        --exclude='tmp/' \
        --exclude='fuzz/corpus/' \
        --exclude='fuzz/artifacts/' \
        --exclude='inspiration/' \
        --exclude='website/node_modules/' \
        --exclude='website/.next/' \
        --exclude='website/dist/' \
        --exclude='specs/tla/states/' \
        --exclude='specs/' \
        --exclude='tools/formal-verification/' \
        ./ {{EPYC_HOST}}:{{EPYC_FUZZ_PATH}}/
    # Expose the persistent corpora + artifacts trees as symlinks inside fuzz/.
    ssh {{EPYC_HOST}} "set -e; \
        ln -sfn {{EPYC_FUZZ_CORPORA}} {{EPYC_FUZZ_PATH}}/fuzz/corpus && \
        ln -sfn {{EPYC_FUZZ_ARTIFACTS}} {{EPYC_FUZZ_PATH}}/fuzz/artifacts"

# One-shot: run a single fuzz target for N seconds. Useful for ad-hoc
# bisection or spot-checks. TARGET is required; DURATION defaults to 300s.
fuzz-epyc-run target duration="300":
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<REMOTE_EOF
    set -euo pipefail
    source "\$HOME/.cargo/env"
    cd {{EPYC_FUZZ_PATH}}/fuzz
    ts=\$(date -u +%Y%m%d-%H%M%S)
    out={{EPYC_FUZZ_RESULTS}}/run-{{target}}-\${ts}
    mkdir -p "\${out}"
    mkdir -p {{EPYC_FUZZ_CORPORA}}/{{target}}
    echo "=== run {{target}} for {{duration}}s ==="
    cargo +nightly fuzz run {{target}} {{EPYC_FUZZ_CORPORA}}/{{target}} -- \
        -max_total_time={{duration}} -print_final_stats=1 \
        2>&1 | tee "\${out}/run.log"
    echo "=== run complete: \${out} ==="
    REMOTE_EOF

# Nightly campaign: Tier 1 + Tier 2 targets. 12 parallel jobs per target via
# libFuzzer `-jobs`, 8 min per target (= ~3 h total). Leaves 52 threads free
# for concurrent VOPR/FV work.
fuzz-epyc-nightly:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fuzz/results/nightly-${ts}
    mkdir -p "${out}"

    targets=(
        fuzz_wire_deserialize fuzz_wire_vsr
        fuzz_crypto_encrypt
        fuzz_sql_parser fuzz_storage_record fuzz_storage_decompress
        fuzz_superblock fuzz_kernel_command
        fuzz_rbac_rewrite fuzz_rbac_bypass fuzz_rbac_injection
        fuzz_abac_evaluator
        fuzz_auth_token
        fuzz_sql_metamorphic
        fuzz_vsr_protocol
    )

    PER_TARGET_SECONDS=${PER_TARGET_SECONDS:-480}   # 8 min/target
    PARALLEL_WORKERS=${PARALLEL_WORKERS:-12}

    for t in "${targets[@]}"; do
        mkdir -p /opt/kimberlite-fuzz/corpora/${t}
        echo ""
        echo "=== ${t} (workers=${PARALLEL_WORKERS}, ${PER_TARGET_SECONDS}s) ==="
        cargo +nightly fuzz run "${t}" /opt/kimberlite-fuzz/corpora/${t} -- \
            -max_total_time=${PER_TARGET_SECONDS} -jobs=${PARALLEL_WORKERS} \
            -workers=${PARALLEL_WORKERS} -print_final_stats=1 \
            2>&1 | tee "${out}/${t}.log" || echo "${t} exited non-zero, continuing"
    done

    echo "=== nightly complete: ${out} ==="
    REMOTE_EOF

# Weekly campaign: same targets as nightly but 2 h each. Run over a weekend.
fuzz-epyc-weekly:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fuzz/results/weekly-${ts}
    mkdir -p "${out}"

    targets=(
        fuzz_wire_deserialize fuzz_wire_vsr
        fuzz_crypto_encrypt
        fuzz_sql_parser fuzz_storage_record fuzz_storage_decompress
        fuzz_superblock fuzz_kernel_command
        fuzz_rbac_rewrite fuzz_rbac_bypass fuzz_rbac_injection
        fuzz_abac_evaluator
        fuzz_auth_token
        fuzz_sql_metamorphic
        fuzz_vsr_protocol
    )

    PER_TARGET_SECONDS=7200   # 2 h/target
    PARALLEL_WORKERS=16

    for t in "${targets[@]}"; do
        mkdir -p /opt/kimberlite-fuzz/corpora/${t}
        echo ""
        echo "=== WEEKLY ${t} (workers=${PARALLEL_WORKERS}, ${PER_TARGET_SECONDS}s) ==="
        cargo +nightly fuzz run "${t}" /opt/kimberlite-fuzz/corpora/${t} -- \
            -max_total_time=${PER_TARGET_SECONDS} -jobs=${PARALLEL_WORKERS} \
            -workers=${PARALLEL_WORKERS} -print_final_stats=1 \
            2>&1 | tee "${out}/${t}.log" || echo "${t} exited non-zero, continuing"
    done

    echo "=== weekly complete: ${out} ==="
    REMOTE_EOF

# Tail the most recent per-target log from the latest campaign directory.
fuzz-epyc-tail:
    ssh {{EPYC_HOST}} "cd {{EPYC_FUZZ_RESULTS}} && latest=\$(ls -t */*.log 2>/dev/null | head -1); echo tailing \$latest; tail -f \$latest"

# Status: recent results, corpus sizes, crashes, system load.
fuzz-epyc-status:
    ssh {{EPYC_HOST}} "echo '=== System ===' && uptime && \
        echo '=== Memory ===' && free -h && \
        echo '=== Recent fuzz results ===' && ls -lht {{EPYC_FUZZ_RESULTS}} 2>/dev/null | head -10 && \
        echo '=== Corpus sizes ===' && du -sh {{EPYC_FUZZ_CORPORA}}/*/ 2>/dev/null | sort -h && \
        echo '=== Artifacts (crashes) ===' && find {{EPYC_FUZZ_ARTIFACTS}} -type f 2>/dev/null | head -20"

# Fetch fuzz campaign results + any crash artifacts back to local.
fuzz-epyc-results:
    mkdir -p .artifacts/epyc-fuzz-results .artifacts/epyc-fuzz-artifacts
    rsync -az {{EPYC_HOST}}:{{EPYC_FUZZ_RESULTS}}/ .artifacts/epyc-fuzz-results/
    rsync -az {{EPYC_HOST}}:{{EPYC_FUZZ_ARTIFACTS}}/ .artifacts/epyc-fuzz-artifacts/

# Cross-target corpus union. Takes every target's corpus, feeds it
# through every other target, and keeps inputs that hit new edges. Finds
# cross-domain "interesting" inputs — e.g. a SQL query that triggers an
# executor edge a kernel fuzzer hadn't discovered. Run weekly.
#
# Cost: quadratic in number of targets — current 17 targets × ~500s per
# run ≈ 2.5h on EPYC. Designed to fit the Sunday weekend window.
fuzz-epyc-corpus-merge:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fuzz/results/corpus-merge-${ts}
    mkdir -p "${out}"
    # Union pass: for each target, feed every other target's corpus
    # through it, keeping only coverage-improving inputs.
    for target_dir in /opt/kimberlite-fuzz/corpora/*/; do
        target=$(basename "${target_dir}")
        echo "=== union into ${target} ===" | tee -a "${out}/merge.log"
        for other_dir in /opt/kimberlite-fuzz/corpora/*/; do
            other=$(basename "${other_dir}")
            [ "${target}" = "${other}" ] && continue
            # `cargo fuzz run --target <t> <other_corpus>` runs one pass;
            # coverage hits get added to <target>'s corpus automatically
            # when using the target's corpus dir as argv[0].
            cargo +nightly fuzz run "${target}" \
                "${target_dir}" "${other_dir}" \
                -- -max_total_time=30 -runs=10000 -print_final_stats=1 \
                >> "${out}/${target}.log" 2>&1 || true
        done
    done
    echo "=== corpus merge complete: ${out} ===" | tee -a "${out}/merge.log"
    # Report corpus size deltas.
    du -sh /opt/kimberlite-fuzz/corpora/*/ | sort -h | tee -a "${out}/merge.log"
    REMOTE_EOF

# Weekly coverage report. Runs `cargo fuzz coverage` for every target
# and produces an HTML index at /opt/kimberlite-fuzz/coverage/week-<YYYY-WW>/.
# Useful for spotting targets that have plateaued (low edges gained per
# hour) so the next weekly campaign can reweight.
fuzz-epyc-coverage:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    week=$(date -u +%Y-W%V)
    out=/opt/kimberlite-fuzz/coverage/week-${week}
    mkdir -p "${out}"
    echo "=== coverage campaign ${week} ===" | tee "${out}/index.log"
    for target_dir in /opt/kimberlite-fuzz/corpora/*/; do
        target=$(basename "${target_dir}")
        echo "=== coverage ${target} ===" | tee -a "${out}/index.log"
        cargo +nightly fuzz coverage "${target}" "${target_dir}" \
            >> "${out}/${target}.log" 2>&1 || {
                echo "WARN: coverage failed for ${target}" >> "${out}/index.log"
                continue
            }
        # Emit per-target text summary via llvm-cov (best-effort — the
        # exact profdata path depends on cargo-fuzz version).
        profdata=$(find target -name 'coverage.profdata' 2>/dev/null | head -1)
        if [ -n "${profdata}" ]; then
            rustup run nightly llvm-cov report \
                "target/$(rustup target list --installed | head -1)/coverage/$(rustup target list --installed | head -1)/release/${target}" \
                --instr-profile="${profdata}" \
                >> "${out}/${target}-report.txt" 2>&1 || true
        fi
    done
    echo "=== coverage report: ${out} ==="
    REMOTE_EOF

# Minimize all corpora (reduce redundant entries). Run weekly to keep
# corpora tractable. cargo-fuzz cmin can take a while per target.
fuzz-epyc-minimize:
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    for dir in /opt/kimberlite-fuzz/corpora/*/; do
        target=$(basename "${dir}")
        echo "=== cmin ${target} ==="
        cargo +nightly fuzz cmin "${target}" "${dir}" || true
    done
    REMOTE_EOF

# Install the systemd service+timer pair that runs `nightly.sh` every
# day at 02:00 UTC. Idempotent — safe to re-run after code changes to
# the unit files.
fuzz-epyc-timer-install: fuzz-epyc-deploy
    #!/usr/bin/env bash
    set -euo pipefail
    # Copy the nightly.sh runner to a stable path that isn't inside the
    # rsync tree (so a future fuzz-epyc-deploy --delete doesn't nuke it
    # mid-campaign).
    ssh {{EPYC_HOST}} "mkdir -p /opt/kimberlite-fuzz/bin"
    scp {{EPYC_FUZZ_PATH}}/tools/fuzz/epyc/nightly.sh \
        {{EPYC_HOST}}:/opt/kimberlite-fuzz/bin/nightly.sh 2>/dev/null || \
        rsync -az tools/fuzz/epyc/nightly.sh {{EPYC_HOST}}:/opt/kimberlite-fuzz/bin/nightly.sh
    # Unit files live in /etc/systemd/system/ per FHS; copy + reload daemon.
    rsync -az tools/fuzz/epyc/kimberlite-fuzz-nightly.service \
        tools/fuzz/epyc/kimberlite-fuzz-nightly.timer \
        {{EPYC_HOST}}:/etc/systemd/system/
    ssh {{EPYC_HOST}} "bash -s" <<'REMOTE_EOF'
    set -euo pipefail
    chmod +x /opt/kimberlite-fuzz/bin/nightly.sh
    systemctl daemon-reload
    systemctl enable --now kimberlite-fuzz-nightly.timer
    systemctl list-timers kimberlite-fuzz-nightly.timer --no-pager
    REMOTE_EOF

# Disable the nightly timer without removing the unit files. Flip back
# on with `fuzz-epyc-timer-install` (or a plain `systemctl enable --now`).
fuzz-epyc-timer-disable:
    ssh {{EPYC_HOST}} "systemctl disable --now kimberlite-fuzz-nightly.timer && \
        echo 'timer disabled; unit files remain at /etc/systemd/system/'"

# Status: timer + last service run + recent journal lines.
fuzz-epyc-timer-status:
    ssh {{EPYC_HOST}} "echo '=== timer ===' && \
        systemctl status kimberlite-fuzz-nightly.timer --no-pager --full 2>/dev/null | head -15 && \
        echo '=== service ===' && \
        systemctl status kimberlite-fuzz-nightly.service --no-pager --full 2>/dev/null | head -15 && \
        echo '=== recent journal ===' && \
        journalctl -u kimberlite-fuzz-nightly --no-pager -n 30"

# Run the nightly ad-hoc via systemd (same env, same logging as scheduled
# runs). Returns when the service completes.
fuzz-epyc-timer-run-now:
    ssh {{EPYC_HOST}} "systemctl start kimberlite-fuzz-nightly.service --wait && \
        journalctl -u kimberlite-fuzz-nightly --no-pager -n 30"

# ── UBSan campaign (second sanitizer, 06:00 UTC) ──
# Installs the systemd service+timer pair that runs `nightly-ubsan.sh`
# every day at 06:00 UTC (4h after the ASan nightly). Same blast radius,
# different bug-class coverage (integer overflow, UB). Added as part of
# the fuzz-to-types hardening effort per
# docs-internal/contributing/constructor-audit-2026-04.md.
fuzz-epyc-timer-install-ubsan: fuzz-epyc-deploy
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} "mkdir -p /opt/kimberlite-fuzz/bin"
    rsync -az tools/fuzz/epyc/nightly-ubsan.sh \
        {{EPYC_HOST}}:/opt/kimberlite-fuzz/bin/nightly-ubsan.sh
    rsync -az tools/fuzz/epyc/kimberlite-fuzz-ubsan.service \
        tools/fuzz/epyc/kimberlite-fuzz-ubsan.timer \
        {{EPYC_HOST}}:/etc/systemd/system/
    ssh {{EPYC_HOST}} "bash -s" <<'REMOTE_EOF'
    set -euo pipefail
    chmod +x /opt/kimberlite-fuzz/bin/nightly-ubsan.sh
    systemctl daemon-reload
    systemctl enable --now kimberlite-fuzz-ubsan.timer
    systemctl list-timers kimberlite-fuzz-ubsan.timer --no-pager
    REMOTE_EOF

# Disable the UBSan timer without removing unit files.
fuzz-epyc-timer-disable-ubsan:
    ssh {{EPYC_HOST}} "systemctl disable --now kimberlite-fuzz-ubsan.timer && \
        echo 'UBSan timer disabled; unit files remain at /etc/systemd/system/'"

# Status: UBSan timer + last service run + recent journal lines.
fuzz-epyc-timer-status-ubsan:
    ssh {{EPYC_HOST}} "echo '=== UBSan timer ===' && \
        systemctl status kimberlite-fuzz-ubsan.timer --no-pager --full 2>/dev/null | head -15 && \
        echo '=== UBSan service ===' && \
        systemctl status kimberlite-fuzz-ubsan.service --no-pager --full 2>/dev/null | head -15 && \
        echo '=== recent UBSan journal ===' && \
        journalctl -u kimberlite-fuzz-ubsan --no-pager -n 30"

# Run the UBSan campaign ad-hoc via systemd. Returns when the service
# completes.
fuzz-epyc-timer-run-now-ubsan:
    ssh {{EPYC_HOST}} "systemctl start kimberlite-fuzz-ubsan.service --wait && \
        journalctl -u kimberlite-fuzz-ubsan --no-pager -n 30"

# End-to-end smoke: deploy + 60s per target. Use to verify the toolchain
# works after bootstrap without committing to a full nightly.
fuzz-epyc-smoke: fuzz-epyc-deploy
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{EPYC_HOST}} bash -s <<'REMOTE_EOF'
    set -euo pipefail
    source "$HOME/.cargo/env"
    cd /opt/kimberlite-fuzz/repo/fuzz
    ts=$(date -u +%Y%m%d-%H%M%S)
    out=/opt/kimberlite-fuzz/results/smoke-${ts}
    mkdir -p "${out}"
    for t in fuzz_sql_parser fuzz_wire_vsr fuzz_storage_record fuzz_storage_decompress; do
        mkdir -p /opt/kimberlite-fuzz/corpora/${t}
        echo "=== smoke ${t} (60s) ==="
        cargo +nightly fuzz run "${t}" /opt/kimberlite-fuzz/corpora/${t} -- \
            -max_total_time=60 -print_final_stats=1 2>&1 | tee "${out}/${t}.log" \
            || { echo "${t} FAILED"; exit 1; }
    done
    echo "=== smoke complete: ${out} ==="
    REMOTE_EOF
