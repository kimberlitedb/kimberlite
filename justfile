# KimberliteDB Development Commands
# Install just: cargo install just
# Run `just` to see available commands

set dotenv-load := false

# Default: show available commands
default:
    @just --list

# ─────────────────────────────────────────────────────────────────────────────
# Development
# ─────────────────────────────────────────────────────────────────────────────

# Run the application in debug mode
run *args:
    cargo run -- {{args}}

# Run with release optimizations
run-release *args:
    cargo run --release -- {{args}}

# Build debug
build:
    cargo build --workspace

# Build release
build-release:
    cargo build --workspace --release

# ─────────────────────────────────────────────────────────────────────────────
# Cross-Platform Build Verification
# ─────────────────────────────────────────────────────────────────────────────

# Verify native macOS build (current architecture)
verify-build-macos-native:
    @echo "Building for native macOS (aarch64-apple-darwin)..."
    cargo build --release --target aarch64-apple-darwin -p kimberlite-cli
    @echo "Testing binary..."
    ./target/aarch64-apple-darwin/release/kimberlite version
    @echo "✓ Native macOS build verified"

# Verify macOS x86_64 build using zigbuild (requires cargo-zigbuild and zig)
verify-build-macos-x86:
    @echo "Building for macOS x86_64 using zigbuild..."
    @if ! command -v cargo-zigbuild >/dev/null 2>&1; then \
        echo "Error: cargo-zigbuild not found. Install with: cargo install cargo-zigbuild"; \
        exit 1; \
    fi
    @if ! command -v zig >/dev/null 2>&1; then \
        echo "Error: zig not found. Install from: https://ziglang.org/download/"; \
        exit 1; \
    fi
    rustup target add x86_64-apple-darwin
    cargo zigbuild --release --target x86_64-apple-darwin -p kimberlite-cli
    @echo "✓ macOS x86_64 build completed (cannot test on ARM Mac)"
    @echo "  Binary: target/x86_64-apple-darwin/release/kimberlite"

# Verify Linux x86_64 build using zigbuild (for cross-compile testing)
verify-build-linux-x86:
    @echo "Building for Linux x86_64 using zigbuild..."
    @if ! command -v cargo-zigbuild >/dev/null 2>&1; then \
        echo "Error: cargo-zigbuild not found. Install with: cargo install cargo-zigbuild"; \
        exit 1; \
    fi
    @if ! command -v zig >/dev/null 2>&1; then \
        echo "Error: zig not found. Install from: https://ziglang.org/download/"; \
        exit 1; \
    fi
    rustup target add x86_64-unknown-linux-gnu
    cargo zigbuild --release --target x86_64-unknown-linux-gnu -p kimberlite-cli
    @echo "✓ Linux x86_64 build completed (cannot test on macOS)"
    @echo "  Binary: target/x86_64-unknown-linux-gnu/release/kimberlite"

# Verify all cross-platform builds (macOS only)
verify-builds-all: verify-build-macos-native verify-build-macos-x86 verify-build-linux-x86
    @echo ""
    @echo "=========================================="
    @echo "Cross-platform build verification complete"
    @echo "=========================================="
    @echo ""
    @echo "Built binaries:"
    @echo "  ✓ macOS ARM64:  target/aarch64-apple-darwin/release/kimberlite (TESTED)"
    @echo "  ✓ macOS x86_64: target/x86_64-apple-darwin/release/kimberlite (built only)"
    @echo "  ✓ Linux x86_64: target/x86_64-unknown-linux-gnu/release/kimberlite (built only)"
    @echo ""
    @echo "Next steps:"
    @echo "  - Test on actual Linux/x86 hardware or Docker"
    @echo "  - CI will test all platforms on their native runners"

# ─────────────────────────────────────────────────────────────────────────────
# Testing
# ─────────────────────────────────────────────────────────────────────────────

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

# ─────────────────────────────────────────────────────────────────────────────
# Code Quality (mirrors CI)
# ─────────────────────────────────────────────────────────────────────────────

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Format code
fmt:
    cargo fmt --all

# Run clippy
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run clippy and auto-fix
clippy-fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# Check that docs build without warnings
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Build and open docs
doc:
    cargo doc --workspace --no-deps --all-features --open

# Check for unused dependencies
unused-deps:
    cargo machete

# ─────────────────────────────────────────────────────────────────────────────
# Security (mirrors CI)
# ─────────────────────────────────────────────────────────────────────────────

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

# ─────────────────────────────────────────────────────────────────────────────
# CI Simulation
# ─────────────────────────────────────────────────────────────────────────────

# Run all CI checks locally (quick version)
ci: fmt-check clippy test doc-check
    @echo "CI checks passed!"

# Run full CI checks including security
ci-full: ci unused-deps audit deny
    @echo "Full CI checks passed!"

# Pre-commit hook: fast checks before committing
pre-commit: fmt-check clippy test
    @echo "Pre-commit checks passed!"

# ─────────────────────────────────────────────────────────────────────────────
# Maintenance
# ─────────────────────────────────────────────────────────────────────────────

# Update dependencies
update:
    cargo update

# Clean build artifacts
clean:
    cargo clean

# Check MSRV (Minimum Supported Rust Version)
msrv:
    cargo +1.85 check --workspace --all-targets

# Generate code coverage report
coverage:
    cargo llvm-cov --workspace --all-features --html
    @echo "Coverage report: target/llvm-cov/html/index.html"

# Generate SBOM (Software Bill of Materials)
sbom:
    cargo cyclonedx --format json --output-prefix kimberlitedb

# ─────────────────────────────────────────────────────────────────────────────
# Simulation (VOPR)
# ─────────────────────────────────────────────────────────────────────────────

# Run VOPR simulation harness (deterministic testing)
vopr *args:
    cargo run --release -p kimberlite-sim --bin vopr -- {{args}}

# Quick smoke test (100 iterations, baseline scenario, all invariants including linearizability)
# Note: Linearizability checker has O(n!) complexity and can be slow. For development iteration,
# use vopr-dev instead. For CI/correctness testing, always use this or vopr-full.
vopr-quick:
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario baseline -n 100

# Fast development smoke test (100 iterations, linearizability disabled for speed)
# Use this for rapid iteration during development. Always run vopr-quick or vopr-full before committing.
vopr-dev:
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario baseline -n 100 --disable-invariant linearizability

# Full test suite (all scenarios with substantial iterations, all invariants)
# This is the CORRECTNESS test - always run before releases and in CI
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

# List available VOPR scenarios
vopr-scenarios:
    cargo run --release -p kimberlite-sim --bin vopr -- --list-scenarios

# Run VOPR with a specific scenario (pass additional args like --vsr-mode at the end)
vopr-scenario scenario="baseline" iterations="100" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario {{scenario}} -n {{iterations}} {{args}}

# Run all VOPR scenarios sequentially (27 scenarios)
vopr-all-scenarios iterations="100":
    @echo "Running all 27 VOPR scenarios..."
    @for scenario in baseline swizzle gray multi-tenant time-compression combined \
        view-change-merge commit-desync inflated-commit invalid-metadata malicious-view-change leader-race \
        dvc-tail-mismatch dvc-identical-claims oversized-start-view invalid-repair-range invalid-kernel-command \
        bit-flip checksum-validation silent-disk-failure \
        crash-commit crash-view-change recovery-corrupt \
        slow-disk intermittent-network \
        race-view-changes race-commit-dvc; do \
        echo "=== Running scenario: $scenario ==="; \
        cargo run --release -p kimberlite-sim --bin vopr -- --scenario $scenario -n {{iterations}}; \
    done
    @echo "All 27 scenarios complete!"

# Run comprehensive overnight test (all 27 scenarios with many iterations)
vopr-overnight-all iterations="1000000":
    VOPR_ITERATIONS={{iterations}} ./scripts/vopr-overnight-all.sh

# Run single scenario overnight test
vopr-overnight scenario="combined" iterations="10000000":
    VOPR_SCENARIO={{scenario}} VOPR_ITERATIONS={{iterations}} ./scripts/vopr-overnight.sh

# ─────────────────────────────────────────────────────────────────────────────
# VOPR Stress Tests (Deep Simulations)
# ─────────────────────────────────────────────────────────────────────────────

# Shallow test: many iterations, few events per sim (good for coverage)
vopr-shallow iterations="100000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 10000 {{args}}

# Medium test: balanced depth and breadth (~1 hour)
vopr-medium iterations="5000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 100000 {{args}}

# Deep test: fewer iterations, many events per sim (~4 hours)
vopr-deep iterations="1000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 500000 {{args}}

# Overnight test: deep simulations with substantial iterations (~8-12 hours)
vopr-overnight-deep iterations="2000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 1000000 --checkpoint-file vopr-checkpoint.json {{args}}

# Marathon test: extreme depth (24+ hours)
vopr-marathon iterations="5000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario combined -n {{iterations}} --max-events 5000000 --checkpoint-file vopr-marathon.json {{args}}

# Stress test specific scenario with custom depth
vopr-stress-scenario scenario iterations="1000" max_events="500000" *args="":
    cargo run --release -p kimberlite-sim --bin vopr -- --scenario {{scenario}} -n {{iterations}} --max-events {{max_events}} {{args}}

# Byzantine attack marathon (all Byzantine scenarios, deep)
vopr-byzantine-marathon iterations="500" max_events="300000" *args="":
    @echo "Running Byzantine attack scenarios with {{iterations}} iterations x {{max_events}} events..."
    @for scenario in view-change-merge commit-desync inflated-commit invalid-metadata malicious-view-change leader-race \
        dvc-tail-mismatch dvc-identical-claims oversized-start-view invalid-repair-range invalid-kernel-command; do \
        echo "=== Byzantine: $scenario ==="; \
        cargo run --release -p kimberlite-sim --bin vopr -- --scenario $scenario -n {{iterations}} --max-events {{max_events}} {{args}}; \
    done

# Corruption detection marathon (deep simulations)
vopr-corruption-marathon iterations="500" max_events="300000" *args="":
    @echo "Running corruption detection scenarios..."
    @for scenario in bit-flip checksum-validation silent-disk-failure; do \
        echo "=== Corruption: $scenario ==="; \
        cargo run --release -p kimberlite-sim --bin vopr -- --scenario $scenario -n {{iterations}} --max-events {{max_events}} {{args}}; \
    done

# Crash recovery marathon
vopr-crash-marathon iterations="500" max_events="300000" *args="":
    @echo "Running crash recovery scenarios..."
    @for scenario in crash-commit crash-view-change recovery-corrupt; do \
        echo "=== Crash: $scenario ==="; \
        cargo run --release -p kimberlite-sim --bin vopr -- --scenario $scenario -n {{iterations}} --max-events {{max_events}} {{args}}; \
    done

# ─────────────────────────────────────────────────────────────────────────────
# VOPR Advanced Debugging (v0.4.0)
# ─────────────────────────────────────────────────────────────────────────────

# Show timeline visualization of failure bundle (ASCII Gantt chart)
vopr-timeline bundle width="120":
    cargo run --release -p kimberlite-sim --bin vopr -- timeline {{bundle}} --width {{width}}

# Show timeline with time range filter (microseconds)
vopr-timeline-range bundle start end:
    cargo run --release -p kimberlite-sim --bin vopr -- timeline {{bundle}} --time-range {{start}} {{end}}

# Bisect to find first failing event in bundle
vopr-bisect bundle:
    cargo run --release -p kimberlite-sim --bin vopr -- bisect {{bundle}}

# Bisect with custom checkpoint interval
vopr-bisect-checkpoint bundle interval="1000":
    cargo run --release -p kimberlite-sim --bin vopr -- bisect {{bundle}} --checkpoint-interval {{interval}}

# Minimize failure bundle using delta debugging (ddmin)
vopr-minimize bundle:
    cargo run --release -p kimberlite-sim --bin vopr -- minimize {{bundle}}

# Minimize with custom granularity
vopr-minimize-gran bundle granularity="8":
    cargo run --release -p kimberlite-sim --bin vopr -- minimize {{bundle}} --granularity {{granularity}}

# Start VOPR coverage dashboard (requires --features dashboard)
vopr-dashboard port="8080":
    cargo run --release -p kimberlite-sim --bin vopr --features dashboard -- dashboard --port {{port}}

# Start dashboard with saved coverage file
vopr-dashboard-load coverage_file port="8080":
    cargo run --release -p kimberlite-sim --bin vopr --features dashboard -- dashboard --coverage-file {{coverage_file}} --port {{port}}

# Launch interactive TUI (requires --features tui)
vopr-tui iterations="1000":
    cargo run --release -p kimberlite-sim --bin vopr --features tui -- tui --iterations {{iterations}}

# Launch TUI with specific scenario
vopr-tui-scenario scenario iterations="5000":
    cargo run --release -p kimberlite-sim --bin vopr --features tui -- tui --scenario {{scenario}} --iterations {{iterations}}

# ─────────────────────────────────────────────────────────────────────────────
# VOPR AWS Deployment
# ─────────────────────────────────────────────────────────────────────────────

# Deploy VOPR to AWS (requires terraform.tfvars configured)
deploy-vopr:
    #!/usr/bin/env bash
    cd infra/vopr-aws
    terraform init
    terraform apply

# Check VOPR deployment status
vopr-status:
    #!/usr/bin/env bash
    set -euo pipefail
    ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
    BUCKET="vopr-simulation-results-${ACCOUNT_ID}"

    echo "=== Current Progress ==="
    aws s3 cp "s3://${BUCKET}/checkpoints/latest.json" - 2>/dev/null | jq . || echo "No checkpoint found"
    echo ""
    echo "=== CloudWatch Metrics (last hour) ==="
    ITERATIONS=$(aws cloudwatch get-metric-statistics \
        --region us-east-1 \
        --namespace VOPR \
        --metric-name IterationsCompleted \
        --start-time $(date -u -v-1H +%Y-%m-%dT%H:%M:%S) \
        --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
        --period 3600 \
        --statistics Sum \
        --query 'Datapoints[0].Sum' \
        --output text)
    echo "Iterations completed: ${ITERATIONS:-0}"

# View live VOPR logs from AWS
vopr-logs:
    aws logs tail --region us-east-1 /aws/ec2/vopr-simulation --follow

# SSH to VOPR instance via AWS SSM
vopr-ssh:
    #!/usr/bin/env bash
    cd infra/vopr-aws
    INSTANCE_ID=$(terraform output -raw instance_id)
    aws ssm start-session --region us-east-1 --target "$INSTANCE_ID"

# Stop VOPR instance (saves costs, preserves state)
vopr-stop:
    #!/usr/bin/env bash
    cd infra/vopr-aws
    INSTANCE_ID=$(terraform output -raw instance_id)
    aws ec2 stop-instances --region us-east-1 --instance-ids "$INSTANCE_ID"
    echo "Instance $INSTANCE_ID stopped"

# Start VOPR instance (resumes from checkpoint)
vopr-start:
    #!/usr/bin/env bash
    cd infra/vopr-aws
    INSTANCE_ID=$(terraform output -raw instance_id)
    aws ec2 start-instances --region us-east-1 --instance-ids "$INSTANCE_ID"
    echo "Instance $INSTANCE_ID started"

# Destroy VOPR AWS infrastructure (WARNING: deletes all data)
vopr-destroy:
    #!/usr/bin/env bash
    cd infra/vopr-aws
    echo "WARNING: This will delete all VOPR data including failure archives!"
    terraform destroy

# ─────────────────────────────────────────────────────────────────────────────
# Fuzzing
# ─────────────────────────────────────────────────────────────────────────────

# List available fuzz targets
fuzz-list:
    cd fuzz && cargo fuzz list

# Run a fuzz target (use Ctrl+C to stop)
fuzz target *args="":
    cd fuzz && cargo +nightly fuzz run {{target}} {{args}}

# Run fuzz smoke test (10K iterations for CI)
fuzz-smoke:
    cd fuzz && ./ci-fuzz.sh

# Run fuzzer with specific iteration count
fuzz-iterations target="fuzz_wire_deserialize" runs="10000":
    cd fuzz && cargo +nightly fuzz run {{target}} -- -runs={{runs}}

# Run fuzzer with specific seed (for reproduction)
fuzz-seed target="fuzz_wire_deserialize" seed="0":
    cd fuzz && cargo +nightly fuzz run {{target}} -- -seed={{seed}}

# Clean fuzz corpus
fuzz-clean target="fuzz_wire_deserialize":
    rm -rf fuzz/corpus/{{target}}/*
    @echo "Corpus cleaned for {{target}}"

# Run all fuzz targets for smoke testing
fuzz-all:
    @echo "Running smoke tests for all fuzz targets..."
    @cd fuzz && for target in $(cargo fuzz list); do \
        echo "=== Fuzzing: $target ==="; \
        cargo +nightly fuzz run $target -- -runs=10000 || exit 1; \
    done
    @echo "All fuzz targets passed!"

# ─────────────────────────────────────────────────────────────────────────────
# Benchmarking
# ─────────────────────────────────────────────────────────────────────────────

# Run all benchmarks
bench:
    cargo bench -p kimberlite-bench

# Run benchmarks in quick mode (1 second profile time per suite)
bench-quick:
    @echo "Running quick benchmarks (1s profile time)..."
    @cargo bench -p kimberlite-bench --bench crypto -- --profile-time 1
    @cargo bench -p kimberlite-bench --bench storage -- --profile-time 1
    @cargo bench -p kimberlite-bench --bench kernel -- --profile-time 1
    @cargo bench -p kimberlite-bench --bench wire -- --profile-time 1
    @cargo bench -p kimberlite-bench --bench end_to_end -- --profile-time 1
    @echo "All quick benchmarks complete!"

# Run specific benchmark suite
bench-suite suite="crypto":
    cargo bench -p kimberlite-bench --bench {{suite}}

# Run specific benchmark suite in quick mode (1 second profile time)
bench-suite-quick suite="crypto":
    cargo bench -p kimberlite-bench --bench {{suite}} -- --profile-time 1

# Save benchmark baseline
bench-baseline name="main":
    cargo bench -p kimberlite-bench -- --save-baseline {{name}}

# Compare benchmarks against baseline
bench-compare baseline="main":
    cargo bench -p kimberlite-bench -- --baseline {{baseline}}

# Run all benchmark suites sequentially (quick mode)
bench-all-quick:
    @echo "Running all benchmark suites (quick mode)..."
    @for suite in crypto kernel storage wire end_to_end; do \
        echo "=== Benchmarking: $suite ==="; \
        cargo bench -p kimberlite-bench --bench $suite -- --quick; \
    done
    @echo "All benchmarks complete!"

# Generate HTML benchmark reports
bench-report:
    cargo bench -p kimberlite-bench
    @echo "HTML reports: target/criterion/report/index.html"
    @echo "Opening report..."
    open target/criterion/report/index.html || xdg-open target/criterion/report/index.html

# ─────────────────────────────────────────────────────────────────────────────
# Profiling
# ─────────────────────────────────────────────────────────────────────────────

# Profile VOPR with samply (opens Firefox Profiler UI)
profile-vopr iterations="50" browser="firefox":
    BROWSER={{browser}} samply record cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}

# Profile tests with samply
profile-tests crate="kmb-storage" browser="firefox":
    BROWSER={{browser}} samply record cargo test --release -p {{crate}}

# Profile without opening browser (saves .json.gz for manual upload to profiler.firefox.com)
profile-vopr-headless iterations="100":
    samply record --no-open cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}

# Generate flamegraph for VOPR (macOS: may require sudo or SIP disabled)
flamegraph-vopr iterations="50":
    cargo flamegraph --root -o flamegraph.svg -- run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}
    @echo "Flamegraph generated: flamegraph.svg"

# Linux perf profiling (Linux only)
perf-vopr iterations="50":
    perf record -g cargo run --release -p kimberlite-sim --bin vopr -- --no-faults -n {{iterations}}
    perf report

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

# Install development tools
setup:
    @echo "Installing development tools..."
    cargo install cargo-nextest cargo-audit cargo-deny cargo-machete cargo-llvm-cov
    @echo "Done! Optional tools:"
    @echo "  cargo install cargo-cyclonedx    # SBOM generation"
    @echo "  cargo install samply             # Profiling (recommended, works on macOS)"
    @echo "  cargo install flamegraph         # Flamegraphs (Linux preferred, macOS needs sudo)"

# Install pre-commit hook
install-hooks:
    @echo '#!/bin/sh' > .git/hooks/pre-commit
    @echo 'just pre-commit' >> .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo "Pre-commit hook installed!"

# ─────────────────────────────────────────────────────────────────────────────
# SDK Development
# ─────────────────────────────────────────────────────────────────────────────

# Build FFI library for current platform
build-ffi:
    cargo build --package kimberlite-ffi --release
    @echo "FFI library built: target/release/libkimberlite_ffi.*"

# Test FFI library
test-ffi:
    cargo test --package kimberlite-ffi

# Run FFI tests under Valgrind (Linux only)
test-ffi-valgrind:
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes --error-exitcode=1" \
    cargo test --package kimberlite-ffi -- --test-threads=1

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

# Build Python wheel
build-python-wheel:
    #!/usr/bin/env bash
    cd sdks/python
    python build_wheel.py
    @echo "Python wheel built: sdks/python/dist/"

# Build TypeScript package
build-typescript:
    #!/usr/bin/env bash
    cd sdks/typescript
    npm run build
    @echo "TypeScript package built: sdks/typescript/dist/"

# ─────────────────────────────────────────────────────────────────────────────
# Website (separate workspace in website/)
# ─────────────────────────────────────────────────────────────────────────────

# Run the website dev server
site:
    cd website && cargo run

# Run website with bacon watch mode
site-watch:
    cd website && bacon

# Check website crate
site-check:
    cd website && cargo check

# Run clippy on website crate
site-clippy:
    cd website && cargo clippy

# Build website for release
site-build:
    cd website && cargo build --release

# Build website Docker image (passes git hash for cache busting)
site-docker:
    cd website && docker build --build-arg BUILD_VERSION=$(git rev-parse --short=8 HEAD) -t kmb-site .

# Run website Docker image locally
site-docker-run:
    docker run -p 3000:3000 --rm kmb-site

# Deploy website to AWS (requires SST setup)
site-deploy stage="dev":
    cd website && npx sst deploy --stage {{stage}}
