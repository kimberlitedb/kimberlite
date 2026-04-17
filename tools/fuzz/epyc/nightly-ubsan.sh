#!/usr/bin/env bash
#
# EPYC fuzz nightly UBSan campaign runner (Tier 2, separate sanitizer).
#
# The default nightly (`nightly.sh`) runs with ASan instrumentation — heap
# corruption, use-after-free, out-of-bounds. The 5 bugs found in the first
# Apr 2026 campaign were all logic bugs; none were memory safety. UBSan
# (UndefinedBehaviorSanitizer) catches a different bug class — integer
# overflow, signed overflow, division by zero, invalid enum discriminants —
# and is exactly the class that preceded the LZ4 OOM. Running UBSan as a
# second nightly campaign doubles bug-class coverage without perturbing the
# ASan schedule or corpora.
#
# Design:
#   * Fires at 06:00 UTC — 4h after `nightly.sh` (02:00 UTC), which completes
#     in 80-120 min normally, so the two campaigns do not overlap on CPU.
#   * Writes to /opt/kimberlite-fuzz/results/ubsan-<ts>/ so ASan / UBSan
#     results stay independently diffable.
#   * Shares corpora with ASan at /opt/kimberlite-fuzz/corpora/ — coverage
#     discovered by one sanitizer benefits the other.
#   * Fail-open per target (same pattern as nightly.sh).

set -uo pipefail

FUZZ_ROOT="/opt/kimberlite-fuzz"
REPO="${FUZZ_ROOT}/repo"
CORPORA="${FUZZ_ROOT}/corpora"
RESULTS="${FUZZ_ROOT}/results"

PER_TARGET_SECONDS=${PER_TARGET_SECONDS:-480}
PARALLEL_WORKERS=${PARALLEL_WORKERS:-12}

if [ ! -d "${REPO}/fuzz" ]; then
    echo "ERROR: ${REPO}/fuzz does not exist — run 'just fuzz-epyc-deploy' first" >&2
    exit 1
fi

if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo not in PATH after sourcing $HOME/.cargo/env" >&2
    exit 1
fi

ts=$(date -u +%Y%m%d-%H%M%S)
out="${RESULTS}/ubsan-${ts}"
mkdir -p "${out}" "${CORPORA}"

# Same target list as nightly.sh — UBSan adds integer/UB coverage on the
# same surfaces, not a different one.
targets=(
    fuzz_wire_deserialize
    fuzz_wire_vsr
    fuzz_wire_typed
    fuzz_vsr_typed
    fuzz_crypto_encrypt
    fuzz_sql_parser
    fuzz_storage_record
    fuzz_storage_decompress
    fuzz_superblock
    fuzz_kernel_command
    fuzz_rbac_rewrite
    fuzz_rbac_bypass
    fuzz_rbac_injection
    fuzz_abac_evaluator
    fuzz_auth_token
    fuzz_sql_metamorphic
    fuzz_vsr_protocol
)

echo "=== UBSan campaign ${ts} starting ==="
echo "    per-target: ${PER_TARGET_SECONDS}s, workers: ${PARALLEL_WORKERS}"
echo "    targets:    ${#targets[@]}"
echo "    results:    ${out}"
echo "    sanitizer:  undefined (UBSan)"

cd "${REPO}/fuzz"

# Environment for cargo-fuzz to build with UBSan. `cargo fuzz run --sanitizer`
# would be cleaner but pins cargo-fuzz >= 0.12; falling back to RUSTFLAGS
# matches the approach the manual `cargo +nightly fuzz run` invocation in
# the README documents for custom sanitizers.
export RUSTFLAGS="-Zsanitizer=undefined -Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 -Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table -Cllvm-args=-sanitizer-coverage-trace-compares"
export RUSTDOCFLAGS="-Zsanitizer=undefined"

for t in "${targets[@]}"; do
    mkdir -p "${CORPORA}/${t}"
    echo ""
    echo "=== ${t} UBSan (${PER_TARGET_SECONDS}s, ${PARALLEL_WORKERS} workers) ==="
    cargo +nightly fuzz run --sanitizer=none "${t}" "${CORPORA}/${t}" -- \
        -max_total_time=${PER_TARGET_SECONDS} \
        -jobs=${PARALLEL_WORKERS} \
        -workers=${PARALLEL_WORKERS} \
        -print_final_stats=1 \
        2>&1 | tee "${out}/${t}.log" || echo "${t} exited non-zero, continuing"
done

echo ""
echo "=== UBSan campaign complete: ${out} ==="

crashes=$(find "${FUZZ_ROOT}/artifacts" -type f -newer "${out}" 2>/dev/null | wc -l)
echo "    new artifact files this run: ${crashes}"
