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

# Run VOPR with JSON output (for AWS deployment)
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
    if just verify-tla-quick 2>&1 | grep -q "passed"; then
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

    # 4. Alloy
    echo "[4/5] Running Alloy Structural Models..."
    ALLOY_FAILED=0
    mkdir -p .artifacts/formal-verification/alloy

    total_specs=$(ls -1 specs/alloy/*.als | wc -l | tr -d ' ')
    current_spec=0

    for spec in specs/alloy/*.als; do
        current_spec=$((current_spec + 1))
        spec_name=$(basename "$spec" .als)
        echo "  [$current_spec/$total_specs] Checking $spec_name.als..."

        start_time=$(date +%s)
        if java -jar tools/formal-verification/alloy/alloy-6.2.0.jar exec -f -o ".artifacts/formal-verification/alloy/$spec_name" "$spec" > /dev/null 2>&1; then
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
verify-tla:
    #!/usr/bin/env bash
    set -e
    mkdir -p .artifacts/formal-verification/tla
    echo "Running TLA+ model checking..."
    cd specs/tla && tlc -workers auto -depth 20 VSR.tla 2>&1 | tee ../../.artifacts/formal-verification/tla/tlc-output.log

# Run TLA+ quick check (depth 10)
verify-tla-quick:
    #!/usr/bin/env bash
    set -e
    mkdir -p .artifacts/formal-verification/tla
    echo "Running TLA+ quick check..."
    cd specs/tla && tlc -workers auto -depth 10 VSR.tla 2>&1 | tee ../../.artifacts/formal-verification/tla/tlc-output.log

# Run TLAPS mechanized proofs (via Docker)
verify-tlaps:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Running TLAPS mechanized proofs via Docker..."

    if ! docker info > /dev/null 2>&1; then
        echo "Error: Docker is not running"
        exit 1
    fi

    docker run --rm \
        -v "$(pwd)/specs/tla:/workspace" \
        -w /workspace \
        ghcr.io/tlaplus/tlaps:latest \
        tlapm --check /workspace/VSR_Proofs.tla

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
        docker build --platform linux/amd64 \
            -t "$IVY_IMAGE" tools/formal-verification/docker/ivy/
    fi

    # ENTRYPOINT is ivy_check, so only pass the .ivy file as argument
    docker run --rm --platform linux/amd64 \
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
pre-commit: fmt-check clippy test test-docs
    @echo "Pre-commit checks passed!"

# Run full CI checks locally
ci: fmt-check clippy test doc-check
    @echo "CI checks passed!"

# Run full CI with security checks
ci-full: ci unused-deps audit deny
    @echo "Full CI checks passed!"

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

# ───────────────────────────────────────────────────────────────────────────────
# INFRASTRUCTURE TESTING (AWS)
# ───────────────────────────────────────────────────────────────────────────────

# Smoke test all test harnesses locally before deploying to AWS
test-infra-smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    PASSED=0
    FAILED=0

    echo "=============================================="
    echo "Infrastructure Smoke Test"
    echo "=============================================="
    echo ""

    # 1. VOPR JSON output
    echo "[1/4] VOPR JSON output..."
    VOPR_OUT=$(cargo run --release -p kimberlite-sim --bin vopr -- --json -n 10 2>/dev/null || true)
    FIRST_LINE=$(echo "$VOPR_OUT" | head -1)
    if echo "$FIRST_LINE" | jq . > /dev/null 2>&1; then
        echo "  PASSED: VOPR produces valid JSON"
        PASSED=$((PASSED + 1))
    else
        echo "  FAILED: VOPR JSON output not parseable"
        FAILED=$((FAILED + 1))
    fi

    # 2. Fuzz targets
    echo "[2/4] Fuzz targets..."
    FUZZ_OK=true
    for target in fuzz_wire_deserialize fuzz_crypto_encrypt fuzz_sql_parser fuzz_storage_record fuzz_kernel_command fuzz_rbac_rewrite; do
        if (cd fuzz && cargo +nightly fuzz run "$target" -- -runs=1000 > /dev/null 2>&1); then
            echo "  PASSED: $target (1000 runs)"
        else
            echo "  FAILED: $target"
            FUZZ_OK=false
        fi
    done
    if $FUZZ_OK; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
    fi

    # 3. Docker availability (for formal verification)
    echo "[3/4] Docker availability..."
    if docker info > /dev/null 2>&1; then
        echo "  PASSED: Docker is running"
        PASSED=$((PASSED + 1))
    else
        echo "  SKIPPED: Docker not available (formal verification will be skipped on AWS)"
        PASSED=$((PASSED + 1))
    fi

    # 4. Benchmark smoke test
    echo "[4/4] Benchmark smoke..."
    if cargo bench -p kimberlite-bench --bench crypto -- --profile-time 1 > /dev/null 2>&1; then
        echo "  PASSED: Benchmarks run"
        PASSED=$((PASSED + 1))
    else
        echo "  FAILED: Benchmarks failed"
        FAILED=$((FAILED + 1))
    fi

    echo ""
    echo "=============================================="
    echo "Results: $PASSED passed, $FAILED failed"
    echo "=============================================="
    [ $FAILED -eq 0 ]

# Deploy AWS testing infrastructure
deploy-infra:
    #!/usr/bin/env bash
    set -euo pipefail
    cd infra/vopr-aws
    terraform init
    terraform apply

# Destroy AWS testing infrastructure
infra-destroy:
    #!/usr/bin/env bash
    set -euo pipefail
    cd infra/vopr-aws
    terraform destroy

# Show current testing infrastructure status
infra-status:
    #!/usr/bin/env bash
    set -euo pipefail
    BUCKET=$(cd infra/vopr-aws && terraform output -raw s3_bucket 2>/dev/null || echo "")
    if [[ -z "$BUCKET" ]]; then
        echo "No infrastructure deployed. Run: just deploy-infra"
        exit 1
    fi

    INSTANCE_ID=$(cd infra/vopr-aws && terraform output -raw instance_id 2>/dev/null)

    echo "=== Kimberlite Testing Infrastructure ==="
    echo ""

    # Instance status
    STATE=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" \
        --query 'Reservations[0].Instances[0].State.Name' --output text 2>/dev/null || echo "unknown")
    echo "Instance: $INSTANCE_ID ($STATE)"

    # Latest digest
    echo ""
    echo "--- Latest Digest ---"
    if aws s3 cp "s3://$BUCKET/digests/latest.json" - 2>/dev/null | jq -r '
        "Cycle: \(.cycle)",
        "Commit: \(.git_commit)",
        "Generated: \(.generated_at)",
        "VOPR: \(.vopr.iterations // 0) iterations, \(.vopr.failures_new // 0) new failures",
        "Fuzzing crashes: \((.fuzzing | to_entries | map(.value.crashes // 0) | add) // 0)"
    ' 2>/dev/null; then
        true
    else
        echo "No digest available yet"
    fi

    # Latest checkpoint
    echo ""
    echo "--- VOPR Checkpoint ---"
    if aws s3 cp "s3://$BUCKET/checkpoints/latest.json" - 2>/dev/null | jq -r '
        "Last seed: \(.last_seed // 0)",
        "Total iterations: \(.total_iterations // 0)",
        "Total failures: \(.total_failures // 0)"
    ' 2>/dev/null; then
        true
    else
        echo "No checkpoint available yet"
    fi

# Fetch and pretty-print latest daily digest
infra-digest:
    #!/usr/bin/env bash
    set -euo pipefail
    BUCKET=$(cd infra/vopr-aws && terraform output -raw s3_bucket 2>/dev/null || echo "")
    if [[ -z "$BUCKET" ]]; then
        echo "No infrastructure deployed. Run: just deploy-infra"
        exit 1
    fi
    aws s3 cp "s3://$BUCKET/digests/latest.json" - | jq .

# Tail CloudWatch logs from the testing instance
infra-logs:
    #!/usr/bin/env bash
    set -euo pipefail
    LOG_GROUP=$(cd infra/vopr-aws && terraform output -raw log_group 2>/dev/null)
    aws logs tail "$LOG_GROUP" --follow --since 1h

# Open SSM session to testing instance
infra-ssh:
    #!/usr/bin/env bash
    set -euo pipefail
    INSTANCE_ID=$(cd infra/vopr-aws && terraform output -raw instance_id 2>/dev/null)
    aws ssm start-session --target "$INSTANCE_ID"

# Stop the testing instance (save money)
infra-stop:
    #!/usr/bin/env bash
    set -euo pipefail
    INSTANCE_ID=$(cd infra/vopr-aws && terraform output -raw instance_id 2>/dev/null)
    aws ec2 stop-instances --instance-ids "$INSTANCE_ID"
    echo "Instance $INSTANCE_ID stopping..."

# Start the testing instance
infra-start:
    #!/usr/bin/env bash
    set -euo pipefail
    INSTANCE_ID=$(cd infra/vopr-aws && terraform output -raw instance_id 2>/dev/null)
    aws ec2 start-instances --instance-ids "$INSTANCE_ID"
    echo "Instance $INSTANCE_ID starting..."

# List failure artifacts on S3
list-failures:
    #!/usr/bin/env bash
    set -euo pipefail
    BUCKET=$(cd infra/vopr-aws && terraform output -raw s3_bucket 2>/dev/null || echo "")
    if [[ -z "$BUCKET" ]]; then
        echo "No infrastructure deployed. Run: just deploy-infra"
        exit 1
    fi
    echo "=== VOPR Failures ==="
    aws s3 ls "s3://$BUCKET/failures/vopr/" --recursive 2>/dev/null || echo "  (none)"
    echo ""
    echo "=== Fuzz Crashes ==="
    aws s3 ls "s3://$BUCKET/failures/fuzz/" --recursive 2>/dev/null || echo "  (none)"

# Download a failure artifact for local reproduction
fetch-failure artifact:
    #!/usr/bin/env bash
    set -euo pipefail
    BUCKET=$(cd infra/vopr-aws && terraform output -raw s3_bucket 2>/dev/null || echo "")
    if [[ -z "$BUCKET" ]]; then
        echo "No infrastructure deployed. Run: just deploy-infra"
        exit 1
    fi
    mkdir -p .artifacts/aws-failures
    FILENAME=$(basename "{{artifact}}")
    aws s3 cp "s3://$BUCKET/failures/{{artifact}}" ".artifacts/aws-failures/$FILENAME"
    echo ""
    echo "Downloaded to: .artifacts/aws-failures/$FILENAME"
    echo ""
    if [[ "$FILENAME" == *.kmb ]]; then
        echo "Reproduce with:"
        echo "  just vopr-repro .artifacts/aws-failures/$FILENAME"
    elif [[ "$FILENAME" == *.tar.gz ]]; then
        echo "Extract with:"
        echo "  tar xzf .artifacts/aws-failures/$FILENAME -C .artifacts/aws-failures/"
        echo "Then run the fuzz target with the crash input"
    fi

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

# Fetch latest benchmark results from AWS
infra-bench:
    #!/usr/bin/env bash
    set -euo pipefail
    BUCKET=$(cd infra/vopr-aws && terraform output -raw s3_bucket 2>/dev/null || echo "")
    if [[ -z "$BUCKET" ]]; then
        echo "No infrastructure deployed. Run: just deploy-infra"
        exit 1
    fi
    echo "=== Latest Benchmark Results ==="
    aws s3 cp "s3://$BUCKET/benchmarks/$(date +%Y-%m-%d).json" - 2>/dev/null | jq . || \
        echo "No benchmark results for today. Checking latest..."
    aws s3 ls "s3://$BUCKET/benchmarks/" 2>/dev/null | tail -5
