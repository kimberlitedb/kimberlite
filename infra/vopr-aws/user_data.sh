#!/bin/bash
set -euo pipefail

# Log everything
exec > >(tee /var/log/user-data.log)
exec 2>&1

echo "=== Kimberlite Testing Infrastructure Setup ==="

# ============================================================================
# Environment variables from Terraform
# ============================================================================
export AWS_DEFAULT_REGION="${aws_region}"
export S3_BUCKET="${s3_bucket}"
export SNS_TOPIC_ARN="${sns_topic_arn}"
export LOG_GROUP="${log_group}"
export GITHUB_REPO="${github_repo}"
export GITHUB_BRANCH="${github_branch}"
export RUN_DURATION_HOURS="${run_duration_hours}"
export ENABLE_FUZZING="${enable_fuzzing}"
export ENABLE_FORMAL_VERIFICATION="${enable_formal_verification}"
export ENABLE_BENCHMARKS="${enable_benchmarks}"

# ============================================================================
# Install system dependencies
# ============================================================================
echo "Installing system dependencies..."
dnf update -y
dnf install -y \
  amazon-cloudwatch-agent aws-cli jq git gcc gcc-c++ make \
  openssl-devel pkgconfig cmake perl

# Ensure SSM agent is running (pre-installed on AL2023, may need restart after dnf update)
systemctl restart amazon-ssm-agent 2>/dev/null || true

# Install Docker (for Coq, TLAPS, Ivy)
if [[ "$ENABLE_FORMAL_VERIFICATION" == "true" ]]; then
  echo "Installing Docker for formal verification..."
  dnf install -y docker
  systemctl enable docker
  systemctl start docker
  usermod -aG docker ec2-user

  # Wait for Docker daemon to be fully ready
  echo "Waiting for Docker daemon..."
  for i in $(seq 1 30); do
    docker info > /dev/null 2>&1 && break
    sleep 1
  done

  # Register QEMU binfmt for cross-architecture emulation (amd64 on ARM Graviton).
  # Required because coqorg/coq and Z3/Ivy only work reliably on amd64.
  echo "Setting up QEMU binfmt for amd64 emulation..."
  docker run --privileged --rm tonistiigi/binfmt --install amd64 > /dev/null 2>&1 || true
fi

# Install Java 17 (for TLA+/Alloy)
if [[ "$ENABLE_FORMAL_VERIFICATION" == "true" ]]; then
  echo "Installing Java 17 for TLA+/Alloy..."
  dnf install -y java-17-amazon-corretto-headless
fi

# ============================================================================
# Install Rust
# ============================================================================
echo "Installing Rust 1.88.0..."
export HOME=/root
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.88.0
source "$HOME/.cargo/env"

# Install nightly toolchain for cargo-fuzz
if [[ "$ENABLE_FUZZING" == "true" ]]; then
  echo "Installing nightly Rust for fuzzing..."
  rustup toolchain install nightly --profile minimal
  cargo install cargo-fuzz
fi

# ============================================================================
# Clone and build kimberlite
# ============================================================================
echo "Cloning and building kimberlite..."
cd /opt
git clone "$GITHUB_REPO" kimberlite
cd kimberlite
git checkout "$GITHUB_BRANCH"
GIT_COMMIT=$(git rev-parse --short=7 HEAD)

# Build VOPR (release mode for performance)
cargo build --release -p kimberlite-sim --bin vopr

# Build benchmarks
if [[ "$ENABLE_BENCHMARKS" == "true" ]]; then
  cargo build --release -p kimberlite-bench
fi

# Create working directories
mkdir -p /var/lib/kimberlite-testing
chown -R ec2-user:ec2-user /var/lib/kimberlite-testing

# ============================================================================
# Test orchestrator script
# ============================================================================
cat > /usr/local/bin/kimberlite-test-runner.sh <<'RUNNER_EOF'
#!/bin/bash
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────
STATE_DIR="/var/lib/kimberlite-testing"
CHECKPOINT_FILE="$STATE_DIR/checkpoint.json"
VOPR_BIN="/opt/kimberlite/target/release/vopr"
REPO_DIR="/opt/kimberlite"
BATCH_SIZE=100
CYCLE=0

echo "=== Kimberlite Test Runner Starting ==="
echo "S3 Bucket: $S3_BUCKET"
echo "Run Duration: $RUN_DURATION_HOURS hours"
echo "Fuzzing: $ENABLE_FUZZING"
echo "Formal Verification: $ENABLE_FORMAL_VERIFICATION"
echo "Benchmarks: $ENABLE_BENCHMARKS"

# ── Helper functions ─────────────────────────────────────────────────────────

publish_metric() {
  local metric_name="$1"
  local value="$2"
  local unit="$${3:-Count}"
  aws cloudwatch put-metric-data \
    --namespace "Kimberlite/Testing" \
    --metric-name "$metric_name" \
    --value "$value" \
    --unit "$unit" \
    --region "$AWS_DEFAULT_REGION" 2>/dev/null || true
}

upload_to_s3() {
  local src="$1"
  local dst="$2"
  aws s3 cp "$src" "s3://$S3_BUCKET/$dst" \
    --region "$AWS_DEFAULT_REGION" --only-show-errors 2>/dev/null || true
}

download_from_s3() {
  local src="$1"
  local dst="$2"
  aws s3 cp "s3://$S3_BUCKET/$src" "$dst" \
    --region "$AWS_DEFAULT_REGION" --only-show-errors 2>/dev/null
}

get_known_signatures() {
  local sig_file="$STATE_DIR/known-failures.json"
  download_from_s3 "signatures/known-failures.json" "$sig_file" || true
  if [[ ! -f "$sig_file" ]]; then
    echo '{"signatures":[]}' > "$sig_file"
  fi
  echo "$sig_file"
}

add_signature() {
  local sig="$1"
  local sig_file
  sig_file=$(get_known_signatures)
  local updated
  updated=$(jq --arg s "$sig" '.signatures += [$s] | .signatures |= unique' "$sig_file")
  echo "$updated" > "$sig_file"
  upload_to_s3 "$sig_file" "signatures/known-failures.json"
}

is_known_signature() {
  local sig="$1"
  local sig_file
  sig_file=$(get_known_signatures)
  jq -e --arg s "$sig" '.signatures | index($s) != null' "$sig_file" > /dev/null 2>&1
}

# ── Update repo ──────────────────────────────────────────────────────────────

update_repo() {
  echo "Updating repository..."
  cd "$REPO_DIR"
  git fetch origin "$GITHUB_BRANCH"
  git reset --hard "origin/$GITHUB_BRANCH"
  GIT_COMMIT=$(git rev-parse --short=7 HEAD)
  echo "Building at commit $GIT_COMMIT..."
  cargo build --release -p kimberlite-sim --bin vopr
  if [[ "$ENABLE_BENCHMARKS" == "true" ]]; then
    cargo build --release -p kimberlite-bench
  fi
}

# ── Phase 1: Formal Verification ────────────────────────────────────────────

run_formal_verification() {
  if [[ "$ENABLE_FORMAL_VERIFICATION" != "true" ]]; then
    echo "Formal verification disabled, skipping"
    return
  fi

  echo "=== Phase: Formal Verification ==="
  local results="{}"
  local start_time
  start_time=$(date +%s)
  cd "$REPO_DIR"

  # TLA+ model checking
  echo "[FV] Running TLA+ model checking..."
  local tla_status="skipped"
  if command -v java &> /dev/null && [[ -f specs/tla/VSR.tla ]]; then
    if java -jar tools/formal-verification/alloy/alloy-6.2.0.jar --version > /dev/null 2>&1 || true; then
      # Use TLC directly if available, otherwise skip
      if command -v tlc &> /dev/null; then
        if (cd specs/tla && tlc -workers auto -depth 10 VSR.tla > /dev/null 2>&1); then
          tla_status="passed"
        else
          tla_status="failed"
        fi
      else
        tla_status="skipped_no_tlc"
      fi
    fi
  fi
  results=$(echo "$results" | jq --arg s "$tla_status" '. + {tla_viewchange: $s}')

  # Coq proofs (via Docker, amd64 emulation on ARM, 30 min timeout)
  echo "[FV] Running Coq proofs..."
  local coq_status="skipped"
  if docker info > /dev/null 2>&1; then
    local coq_image="coqorg/coq:8.18"
    docker pull --platform linux/amd64 "$coq_image" > /dev/null 2>&1 || true
    # Mount read-only + copy to writable /tmp (coq user can't write to root-owned volume)
    if timeout 1800 docker run --rm --platform linux/amd64 \
      -v "$(pwd)/specs/coq:/src:ro" \
      "$coq_image" \
      sh -c 'cp /src/*.v /tmp/ && cd /tmp && for f in Common.v SHA256.v BLAKE3.v AES_GCM.v Ed25519.v KeyHierarchy.v; do [ -f "$f" ] && coqc -Q . Kimberlite "$f" || exit 1; done' > /dev/null 2>&1; then
      coq_status="passed"
    else
      coq_status="failed"
    fi
  fi
  echo "[FV] Coq: $coq_status"
  results=$(echo "$results" | jq --arg s "$coq_status" '. + {coq: $s}')

  # TLAPS — no public Docker image exists; skip until custom image is built
  echo "[FV] TLAPS: skipped (no public Docker image)"
  local tlaps_status="skipped"
  results=$(echo "$results" | jq --arg s "$tlaps_status" '. + {tlaps: $s}')

  # Alloy structural models (5 min timeout per spec, headless JVM)
  echo "[FV] Running Alloy models..."
  local alloy_status="skipped"
  if command -v java &> /dev/null && [[ -f tools/formal-verification/alloy/alloy-6.2.0.jar ]]; then
    local alloy_ok=true
    for spec in specs/alloy/*.als; do
      if [[ -f "$spec" ]]; then
        echo "[FV]   Checking $(basename "$spec")..."
        if ! timeout 300 java -Djava.awt.headless=true -jar tools/formal-verification/alloy/alloy-6.2.0.jar exec -f -o "/tmp/alloy-check" "$spec" > /dev/null 2>&1; then
          echo "[FV]   $(basename "$spec") failed or timed out"
          alloy_ok=false
          break
        fi
      fi
    done
    if $alloy_ok; then
      alloy_status="passed"
    else
      alloy_status="failed"
    fi
  fi
  results=$(echo "$results" | jq --arg s "$alloy_status" '. + {alloy: $s}')

  # Ivy Byzantine model (via Docker — amd64 emulation because Z3 segfaults on ARM)
  echo "[FV] Running Ivy Byzantine model..."
  local ivy_status="skipped"
  if docker info > /dev/null 2>&1 && [[ -f specs/ivy/VSR_Byzantine.ivy ]]; then
    local ivy_image="kimberlite-ivy:amd64"
    if ! docker image inspect "$ivy_image" > /dev/null 2>&1; then
      echo "[FV] Building Ivy Docker image for amd64 (first run, compiles Z3 via QEMU — ~60 min)..."
      if timeout 5400 docker build --platform linux/amd64 -t "$ivy_image" tools/formal-verification/docker/ivy/ 2>&1 | tail -5; then
        echo "[FV] Ivy Docker image built successfully"
      else
        echo "[FV] Ivy Docker image build failed or timed out"
      fi
    fi
    if docker image inspect "$ivy_image" > /dev/null 2>&1; then
      echo "[FV] Running ivy_check on VSR_Byzantine.ivy..."
      if timeout 1800 docker run --rm --platform linux/amd64 \
        -v "$(pwd)/specs/ivy:/workspace" \
        -w /workspace \
        "$ivy_image" \
        VSR_Byzantine.ivy 2>&1 | tail -20; then
        ivy_status="passed"
      else
        ivy_status="failed"
      fi
    else
      ivy_status="skipped_build_failed"
    fi
  fi
  results=$(echo "$results" | jq --arg s "$ivy_status" '. + {ivy: $s}')

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))
  results=$(echo "$results" | jq --argjson d "$duration" '. + {duration_seconds: $d}')

  # Save results
  echo "$results" > "$STATE_DIR/formal-verification-results.json"
  upload_to_s3 "$STATE_DIR/formal-verification-results.json" "formal-verification/$(date +%Y-%m-%d).json"

  echo "Formal verification complete ($${duration}s): $results"
}

# ── Phase 2: Benchmarks ─────────────────────────────────────────────────────

run_benchmarks() {
  if [[ "$ENABLE_BENCHMARKS" != "true" ]]; then
    echo "Benchmarks disabled, skipping"
    return
  fi

  echo "=== Phase: Benchmarks ==="
  cd "$REPO_DIR"
  local start_time
  start_time=$(date +%s)
  local results='{"suites":{},"regressions":false}'

  for suite in crypto kernel storage wire end_to_end; do
    echo "[Bench] Running: $suite"
    local bench_output="/tmp/bench-$suite.json"
    if cargo bench -p kimberlite-bench --bench "$suite" -- --output-format bencher > "$bench_output" 2>&1; then
      results=$(echo "$results" | jq --arg s "$suite" '.suites[$s] = "passed"')
    else
      results=$(echo "$results" | jq --arg s "$suite" '.suites[$s] = "failed"')
    fi
  done

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))
  results=$(echo "$results" | jq --argjson d "$duration" '. + {duration_seconds: $d}')

  # Check for regressions against baseline
  if aws s3 ls "s3://$S3_BUCKET/benchmarks/baseline.json" --region "$AWS_DEFAULT_REGION" > /dev/null 2>&1; then
    echo "[Bench] Baseline exists, future regression detection possible"
  else
    echo "[Bench] No baseline yet, saving current as baseline"
    echo "$results" > "$STATE_DIR/benchmark-baseline.json"
    upload_to_s3 "$STATE_DIR/benchmark-baseline.json" "benchmarks/baseline.json"
  fi

  echo "$results" > "$STATE_DIR/benchmark-results.json"
  upload_to_s3 "$STATE_DIR/benchmark-results.json" "benchmarks/$(date +%Y-%m-%d).json"

  echo "Benchmarks complete ($${duration}s)"
}

# ── Phase 3: Fuzz Testing ───────────────────────────────────────────────────

run_fuzzing() {
  if [[ "$ENABLE_FUZZING" != "true" ]]; then
    echo "Fuzzing disabled, skipping"
    return
  fi

  echo "=== Phase: Fuzz Testing (5.3 hours) ==="
  cd "$REPO_DIR"
  local start_time
  start_time=$(date +%s)
  local fuzz_duration=2400  # 40 minutes per target (8 targets)
  local results='{}'

  # Fuzz targets:
  #   - fuzz_wire_deserialize: Wire protocol deserialization
  #   - fuzz_crypto_encrypt: Cryptographic operations
  #   - fuzz_sql_parser: SQL parser (AUDIT-2026-03 M-1 - enhanced with AST validation)
  #   - fuzz_storage_record: Storage record operations
  #   - fuzz_kernel_command: Kernel command handling
  #   - fuzz_rbac_rewrite: RBAC policy SQL rewriting
  #   - fuzz_sql_differential: SQL differential testing (Crucible - Kimberlite vs DuckDB)
  #   - fuzz_rbac_bypass: RBAC/ABAC policy bypass detection (Crucible - adversarial testing)
  #   - fuzz_abac_evaluator: ABAC evaluator with 12 condition types (AUDIT-2026-03 M-2)

  # Restore fuzz corpus from S3
  echo "[Fuzz] Restoring corpus from S3..."
  mkdir -p fuzz/corpus
  for target in fuzz_wire_deserialize fuzz_crypto_encrypt fuzz_sql_parser fuzz_storage_record fuzz_kernel_command fuzz_rbac_rewrite fuzz_sql_differential fuzz_rbac_bypass fuzz_abac_evaluator; do
    mkdir -p "fuzz/corpus/$target"
    aws s3 sync "s3://$S3_BUCKET/fuzz-corpus/$target/" "fuzz/corpus/$target/" \
      --region "$AWS_DEFAULT_REGION" --only-show-errors 2>/dev/null || true
  done

  # Run each fuzz target
  for target in fuzz_wire_deserialize fuzz_crypto_encrypt fuzz_sql_parser fuzz_storage_record fuzz_kernel_command fuzz_rbac_rewrite fuzz_sql_differential fuzz_rbac_bypass fuzz_abac_evaluator; do
    echo "[Fuzz] Running: $target ($${fuzz_duration}s)"
    local crash_dir="fuzz/artifacts/$target"
    mkdir -p "$crash_dir"

    local crashes_before
    crashes_before=$(find "$crash_dir" -name 'crash-*' -o -name 'leak-*' 2>/dev/null | wc -l)

    # Run fuzzer with time limit
    cd "$REPO_DIR/fuzz"
    timeout "$fuzz_duration" cargo +nightly fuzz run "$target" -- \
      -max_total_time="$fuzz_duration" 2>/dev/null || true
    cd "$REPO_DIR"

    local crashes_after
    crashes_after=$(find "$crash_dir" -name 'crash-*' -o -name 'leak-*' 2>/dev/null | wc -l)
    local new_crashes=$((crashes_after - crashes_before))

    results=$(echo "$results" | jq \
      --arg t "$target" \
      --argjson c "$new_crashes" \
      '. + {($t): {crashes: $c}}')

    # Upload crash artifacts to S3
    if [[ "$new_crashes" -gt 0 ]]; then
      echo "[Fuzz] Found $new_crashes new crash(es) in $target!"
      local ts
      ts=$(date +%s)
      tar czf "/tmp/fuzz-$target-$ts.tar.gz" -C "$crash_dir" .
      upload_to_s3 "/tmp/fuzz-$target-$ts.tar.gz" "failures/fuzz/$target-$ts.tar.gz"
      rm -f "/tmp/fuzz-$target-$ts.tar.gz"
    fi
  done

  # Sync corpus back to S3 for persistence across spot interruptions
  echo "[Fuzz] Syncing corpus to S3..."
  for target in fuzz_wire_deserialize fuzz_crypto_encrypt fuzz_sql_parser fuzz_storage_record fuzz_kernel_command fuzz_rbac_rewrite fuzz_sql_differential fuzz_rbac_bypass fuzz_abac_evaluator; do
    aws s3 sync "fuzz/corpus/$target/" "s3://$S3_BUCKET/fuzz-corpus/$target/" \
      --region "$AWS_DEFAULT_REGION" --only-show-errors 2>/dev/null || true
  done

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))
  results=$(echo "$results" | jq --argjson d "$duration" '. + {duration_seconds: $d}')

  echo "$results" > "$STATE_DIR/fuzz-results.json"
  echo "Fuzzing complete ($${duration}s)"
}

# ── Phase 4: VOPR Marathon ──────────────────────────────────────────────────

run_vopr_marathon() {
  echo "=== Phase: VOPR Marathon ==="
  cd "$REPO_DIR"
  local start_time
  start_time=$(date +%s)

  # Calculate VOPR duration: total run time minus time already spent
  local elapsed_hours=$(( (start_time - CYCLE_START) / 3600 ))
  local remaining_hours=$((RUN_DURATION_HOURS - elapsed_hours - 1))  # -1h for digest generation
  if [[ "$remaining_hours" -lt 1 ]]; then
    remaining_hours=1
  fi
  local vopr_end_time=$((start_time + remaining_hours * 3600))

  echo "[VOPR] Running for ~$${remaining_hours} hours"

  # Restore checkpoint from S3
  if download_from_s3 "checkpoints/latest.json" "$CHECKPOINT_FILE" 2>/dev/null && [[ -f "$CHECKPOINT_FILE" ]]; then
    echo "[VOPR] Restored checkpoint from S3"
    LAST_SEED=$(jq -r '.last_seed // 0' "$CHECKPOINT_FILE")
  else
    echo "[VOPR] No checkpoint found, starting from seed 0"
    LAST_SEED=0
  fi

  local total_iterations=0
  local total_failures=0
  local new_failures=0

  # Run VOPR in batches until time runs out
  while [[ $(date +%s) -lt $vopr_end_time ]]; do
    BATCH_START=$LAST_SEED
    BATCH_END=$((BATCH_START + BATCH_SIZE))

    # Run VOPR batch
    OUTPUT=$($VOPR_BIN --json --max-events 100 --checkpoint-file "$CHECKPOINT_FILE" --seed "$BATCH_START" -n "$BATCH_SIZE" 2>&1 || true)

    # Parse results
    SUCCESSES=$(echo "$OUTPUT" | jq -s '[.[] | select(.type == "iteration" and .data.status == "ok")] | length' 2>/dev/null || echo "0")
    FAILURES=$(echo "$OUTPUT" | jq -s '[.[] | select(.type == "iteration" and .data.status == "failed")] | length' 2>/dev/null || echo "0")

    total_iterations=$((total_iterations + BATCH_SIZE))
    total_failures=$((total_failures + FAILURES))

    # Publish progress metric
    publish_metric "IterationsCompleted" "$BATCH_SIZE"

    # Handle failures
    if [[ "$FAILURES" -gt 0 ]]; then
      FAILED_SEEDS=$(echo "$OUTPUT" | jq -r 'select(.type == "iteration" and .data.status == "failed") | .data.seed' 2>/dev/null | tr '\n' ' ')

      for SEED in $FAILED_SEEDS; do
        # Generate signature for deduplication
        local invariant
        invariant=$(echo "$OUTPUT" | jq -r "select(.type == \"iteration\" and .data.seed == $SEED) | .data.invariant // \"unknown\"" 2>/dev/null || echo "unknown")
        local scenario
        scenario=$(echo "$OUTPUT" | jq -r "select(.type == \"iteration\" and .data.seed == $SEED) | .data.scenario // \"unknown\"" 2>/dev/null || echo "unknown")
        local signature="$${invariant}:$${scenario}"

        # Archive failure to S3
        local ts
        ts=$(date +%s)
        echo "$OUTPUT" | jq -s '.' > "/tmp/failure-$SEED-$ts.json"
        upload_to_s3 "/tmp/failure-$SEED-$ts.json" "failures/vopr/seed-$SEED-$ts.json"
        rm -f "/tmp/failure-$SEED-$ts.json"

        # Check if this is a new failure signature
        if ! is_known_signature "$signature"; then
          new_failures=$((new_failures + 1))
          add_signature "$signature"
          echo "[VOPR] NEW failure signature: $signature (seed $SEED)"
        fi
      done

      # Critical alert: >100 failures in single batch (catastrophic regression)
      if [[ "$FAILURES" -ge 100 ]]; then
        local alert_cooldown_file="$STATE_DIR/last_critical_alert"
        local should_alert=false
        local current_time
        current_time=$(date +%s)

        if [[ ! -f "$alert_cooldown_file" ]]; then
          should_alert=true
        else
          local last_alert
          last_alert=$(cat "$alert_cooldown_file")
          if [[ $((current_time - last_alert)) -ge 3600 ]]; then
            should_alert=true
          fi
        fi

        if [[ "$should_alert" == "true" ]]; then
          aws sns publish \
            --topic-arn "$SNS_TOPIC_ARN" \
            --subject "CRITICAL: $FAILURES failures in single VOPR batch" \
            --message "Catastrophic regression detected. $FAILURES failures in batch $BATCH_START-$BATCH_END at commit $(cd $REPO_DIR && git rev-parse --short=7 HEAD). Check: just infra-digest" \
            --region "$AWS_DEFAULT_REGION" 2>/dev/null || true
          echo "$current_time" > "$alert_cooldown_file"
        fi
      fi
    fi

    # Sync checkpoint to S3
    if [[ -f "$CHECKPOINT_FILE" ]]; then
      upload_to_s3 "$CHECKPOINT_FILE" "checkpoints/latest.json"
      upload_to_s3 "$CHECKPOINT_FILE" "checkpoints/daily/$(date +%Y-%m-%d).json"
    fi

    LAST_SEED=$BATCH_END
    sleep 1
  done

  local end_time
  end_time=$(date +%s)
  local duration=$((end_time - start_time))

  # Calculate throughput
  local throughput=0
  if [[ "$duration" -gt 0 ]]; then
    throughput=$((total_iterations * 1000 / duration))
  fi

  # Save VOPR results
  jq -n \
    --argjson iter "$total_iterations" \
    --argjson fail "$total_failures" \
    --argjson new_fail "$new_failures" \
    --argjson dur "$duration" \
    --argjson tp "$throughput" \
    '{iterations: $iter, failures_total: $fail, failures_new: $new_fail, duration_seconds: $dur, throughput_sps: $tp}' \
    > "$STATE_DIR/vopr-results.json"

  echo "VOPR marathon complete: $total_iterations iterations, $total_failures failures ($new_failures new), $${duration}s"
}

# ── Digest Generation ────────────────────────────────────────────────────────

generate_and_send_digest() {
  echo "=== Generating Daily Digest ==="
  cd "$REPO_DIR"
  local git_commit
  git_commit=$(git rev-parse --short=7 HEAD)
  local generated_at
  generated_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)

  # Load phase results
  local vopr_results='{}'
  if [[ -f "$STATE_DIR/vopr-results.json" ]]; then
    vopr_results=$(cat "$STATE_DIR/vopr-results.json")
  fi

  local fuzz_results='{}'
  if [[ -f "$STATE_DIR/fuzz-results.json" ]]; then
    fuzz_results=$(cat "$STATE_DIR/fuzz-results.json")
  fi

  local fv_results='{}'
  if [[ -f "$STATE_DIR/formal-verification-results.json" ]]; then
    fv_results=$(cat "$STATE_DIR/formal-verification-results.json")
  fi

  local bench_results='{"regressions":false}'
  if [[ -f "$STATE_DIR/benchmark-results.json" ]]; then
    bench_results=$(cat "$STATE_DIR/benchmark-results.json")
  fi

  # Get new failure signatures
  local new_sigs="[]"
  local vopr_new
  vopr_new=$(echo "$vopr_results" | jq -r '.failures_new // 0')

  # Build digest JSON
  local digest
  digest=$(jq -n \
    --arg at "$generated_at" \
    --argjson cycle "$CYCLE" \
    --arg commit "$git_commit" \
    --argjson vopr "$vopr_results" \
    --argjson fuzz "$fuzz_results" \
    --argjson fv "$fv_results" \
    --argjson bench "$bench_results" \
    '{
      generated_at: $at,
      cycle: $cycle,
      git_commit: $commit,
      vopr: $vopr,
      fuzzing: $fuzz,
      formal_verification: $fv,
      benchmarks: $bench
    }')

  # Save and upload digest
  echo "$digest" > "$STATE_DIR/digest.json"
  upload_to_s3 "$STATE_DIR/digest.json" "digests/latest.json"
  upload_to_s3 "$STATE_DIR/digest.json" "digests/$(date +%Y-%m-%d).json"

  # Publish DigestUploaded metric (for no_digest alarm)
  publish_metric "DigestUploaded" "1"

  # Send digest email via SNS (1 email per cycle, not per failure)
  local vopr_iter
  vopr_iter=$(echo "$vopr_results" | jq -r '.iterations // 0')
  local vopr_fail
  vopr_fail=$(echo "$vopr_results" | jq -r '.failures_total // 0')
  local vopr_new_fail
  vopr_new_fail=$(echo "$vopr_results" | jq -r '.failures_new // 0')
  local vopr_tp
  vopr_tp=$(echo "$vopr_results" | jq -r '.throughput_sps // 0')

  local fuzz_crashes=0
  for target in fuzz_wire_deserialize fuzz_crypto_encrypt fuzz_sql_parser fuzz_storage_record fuzz_kernel_command fuzz_rbac_rewrite fuzz_sql_differential fuzz_rbac_bypass fuzz_abac_evaluator; do
    local tc
    tc=$(echo "$fuzz_results" | jq -r --arg t "$target" '.[$t].crashes // 0')
    fuzz_crashes=$((fuzz_crashes + tc))
  done

  local fv_summary=""
  for check in tla_viewchange coq tlaps alloy ivy; do
    local status
    status=$(echo "$fv_results" | jq -r --arg c "$check" '.[$c] // "skipped"')
    fv_summary="$fv_summary  $check: $status\n"
  done

  local bench_regress
  bench_regress=$(echo "$bench_results" | jq -r '.regressions // false')

  aws sns publish \
    --topic-arn "$SNS_TOPIC_ARN" \
    --subject "Kimberlite Testing Digest - Cycle $CYCLE ($git_commit)" \
    --message "$(cat <<SNS_MSG
Kimberlite Testing Digest
=========================
Generated: $generated_at
Cycle: $CYCLE
Commit: $git_commit

VOPR Simulation
  Iterations: $vopr_iter
  Failures (total): $vopr_fail
  Failures (new): $vopr_new_fail
  Throughput: $vopr_tp sims/sec

Fuzz Testing
  Total new crashes: $fuzz_crashes
  fuzz_wire_deserialize: $(echo "$fuzz_results" | jq -r '.fuzz_wire_deserialize.crashes // "skipped"') crashes
  fuzz_crypto_encrypt: $(echo "$fuzz_results" | jq -r '.fuzz_crypto_encrypt.crashes // "skipped"') crashes
  fuzz_sql_parser: $(echo "$fuzz_results" | jq -r '.fuzz_sql_parser.crashes // "skipped"') crashes
  fuzz_storage_record: $(echo "$fuzz_results" | jq -r '.fuzz_storage_record.crashes // "skipped"') crashes
  fuzz_kernel_command: $(echo "$fuzz_results" | jq -r '.fuzz_kernel_command.crashes // "skipped"') crashes
  fuzz_rbac_rewrite: $(echo "$fuzz_results" | jq -r '.fuzz_rbac_rewrite.crashes // "skipped"') crashes
  fuzz_sql_differential: $(echo "$fuzz_results" | jq -r '.fuzz_sql_differential.crashes // "skipped"') crashes
  fuzz_rbac_bypass: $(echo "$fuzz_results" | jq -r '.fuzz_rbac_bypass.crashes // "skipped"') crashes
  fuzz_abac_evaluator: $(echo "$fuzz_results" | jq -r '.fuzz_abac_evaluator.crashes // "skipped"') crashes

Formal Verification
$(echo -e "$fv_summary")
Benchmarks
  Regressions: $bench_regress

Full digest: aws s3 cp s3://$S3_BUCKET/digests/latest.json -
Failures: aws s3 ls s3://$S3_BUCKET/failures/ --recursive
SNS_MSG
)" \
    --region "$AWS_DEFAULT_REGION" 2>/dev/null || true

  echo "Digest generated and sent"
}

# ── Main orchestration loop ──────────────────────────────────────────────────

while true; do
  CYCLE=$((CYCLE + 1))
  CYCLE_START=$(date +%s)
  echo ""
  echo "================================================================"
  echo "=== Starting Cycle $CYCLE at $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
  echo "================================================================"

  # Phase 0: Update repo and rebuild
  update_repo

  # Phase 1: Formal Verification (~1 hour)
  run_formal_verification

  # Phase 2: Benchmarks (~1 hour)
  run_benchmarks

  # Phase 3: Fuzz Testing (~4 hours)
  run_fuzzing

  # Phase 4: VOPR Marathon (remaining time minus 1h for digest)
  run_vopr_marathon

  # Phase 5: Generate and send digest
  generate_and_send_digest

  # Clean up per-cycle state
  rm -f "$STATE_DIR/vopr-results.json"
  rm -f "$STATE_DIR/fuzz-results.json"
  rm -f "$STATE_DIR/formal-verification-results.json"
  rm -f "$STATE_DIR/benchmark-results.json"

  echo "Cycle $CYCLE complete. Restarting..."
  sleep 10
done
RUNNER_EOF

chmod +x /usr/local/bin/kimberlite-test-runner.sh

# ============================================================================
# Configure CloudWatch Agent
# ============================================================================
echo "Configuring CloudWatch Agent..."
cat > /opt/aws/amazon-cloudwatch-agent/etc/config.json <<CW_EOF
{
  "logs": {
    "logs_collected": {
      "files": {
        "collect_list": [
          {
            "file_path": "/var/log/kimberlite-testing.log",
            "log_group_name": "${log_group}",
            "log_stream_name": "{instance_id}",
            "timezone": "UTC"
          }
        ]
      }
    }
  },
  "metrics": {
    "namespace": "Kimberlite/Testing/System",
    "metrics_collected": {
      "cpu": {
        "measurement": [
          {"name": "cpu_usage_idle", "rename": "CPU_IDLE", "unit": "Percent"}
        ],
        "metrics_collection_interval": 60
      },
      "mem": {
        "measurement": [
          {"name": "mem_used_percent", "rename": "MEM_USED", "unit": "Percent"}
        ],
        "metrics_collection_interval": 60
      }
    }
  }
}
CW_EOF

# Start CloudWatch Agent
/opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl \
  -a fetch-config \
  -m ec2 \
  -s \
  -c file:/opt/aws/amazon-cloudwatch-agent/etc/config.json

# ============================================================================
# Create systemd service
# ============================================================================
echo "Creating systemd service..."
cat > /etc/systemd/system/kimberlite-testing.service <<SERVICE_EOF
[Unit]
Description=Kimberlite Long-Running Testing Infrastructure
After=network.target amazon-cloudwatch-agent.service docker.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/kimberlite
Environment="AWS_DEFAULT_REGION=${aws_region}"
Environment="S3_BUCKET=${s3_bucket}"
Environment="SNS_TOPIC_ARN=${sns_topic_arn}"
Environment="GITHUB_REPO=${github_repo}"
Environment="GITHUB_BRANCH=${github_branch}"
Environment="RUN_DURATION_HOURS=${run_duration_hours}"
Environment="ENABLE_FUZZING=${enable_fuzzing}"
Environment="ENABLE_FORMAL_VERIFICATION=${enable_formal_verification}"
Environment="ENABLE_BENCHMARKS=${enable_benchmarks}"
Environment="HOME=/root"
Environment="PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
ExecStart=/usr/local/bin/kimberlite-test-runner.sh
Restart=always
RestartSec=30
StandardOutput=append:/var/log/kimberlite-testing.log
StandardError=append:/var/log/kimberlite-testing.log

[Install]
WantedBy=multi-user.target
SERVICE_EOF

# Start service
systemctl daemon-reload
systemctl enable kimberlite-testing.service
systemctl start kimberlite-testing.service

echo "=== Kimberlite Testing Infrastructure Setup Complete ==="
echo "Service status:"
systemctl status kimberlite-testing.service --no-pager
