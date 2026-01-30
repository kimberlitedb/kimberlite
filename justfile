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
    cargo run --release -p kmb-sim --bin vopr -- {{args}}

# Run VOPR without fault injection (faster)
vopr-clean iterations="100":
    cargo run --release -p kmb-sim --bin vopr -- --no-faults -n {{iterations}}

# Run VOPR with specific seed for reproduction
vopr-seed seed:
    cargo run --release -p kmb-sim --bin vopr -- --seed {{seed}} -v -n 1

# Run VOPR with JSON output (for AWS deployment)
vopr-json iterations="100":
    cargo run --release -p kmb-sim --bin vopr -- --json -n {{iterations}}

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
# Profiling
# ─────────────────────────────────────────────────────────────────────────────

# Profile VOPR with samply (opens Firefox Profiler UI)
profile-vopr iterations="50" browser="firefox":
    BROWSER={{browser}} samply record cargo run --release -p kmb-sim --bin vopr -- --no-faults -n {{iterations}}

# Profile tests with samply
profile-tests crate="kmb-storage" browser="firefox":
    BROWSER={{browser}} samply record cargo test --release -p {{crate}}

# Profile without opening browser (saves .json.gz for manual upload to profiler.firefox.com)
profile-vopr-headless iterations="100":
    samply record --no-open cargo run --release -p kmb-sim --bin vopr -- --no-faults -n {{iterations}}

# Generate flamegraph for VOPR (macOS: may require sudo or SIP disabled)
flamegraph-vopr iterations="50":
    cargo flamegraph --root -o flamegraph.svg -- run --release -p kmb-sim --bin vopr -- --no-faults -n {{iterations}}
    @echo "Flamegraph generated: flamegraph.svg"

# Linux perf profiling (Linux only)
perf-vopr iterations="50":
    perf record -g cargo run --release -p kmb-sim --bin vopr -- --no-faults -n {{iterations}}
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
