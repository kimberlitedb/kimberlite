#!/usr/bin/env bash
#
# EPYC fuzz nightly campaign runner.
#
# Invoked by `kimberlite-fuzz-nightly.service` (systemd) or manually via
# `/opt/kimberlite-fuzz/bin/nightly.sh`. Runs every registered fuzz target
# for PER_TARGET_SECONDS (default 480s = 8 min) with PARALLEL_WORKERS jobs
# per target (default 12).
#
# Design:
#   * Fail-open per target — a single crashing target does not abort the
#     campaign. Each non-zero exit is logged and the next target runs.
#   * Results land in /opt/kimberlite-fuzz/results/nightly-<ts>/ so each
#     campaign is self-contained and diffable.
#   * Corpora persist across runs at /opt/kimberlite-fuzz/corpora/.
#   * The script intentionally does not rsync source — deploys are
#     explicit (`just fuzz-epyc-deploy`) so the timer runs a known, tested
#     tree rather than HEAD.
#
# Exit codes:
#   0 — every target completed (including those that crashed internally)
#   1 — setup failure (cargo env missing, repo tree absent)

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
out="${RESULTS}/nightly-${ts}"
mkdir -p "${out}" "${CORPORA}"

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

echo "=== nightly campaign ${ts} starting ==="
echo "    per-target: ${PER_TARGET_SECONDS}s, workers: ${PARALLEL_WORKERS}"
echo "    targets:    ${#targets[@]}"
echo "    results:    ${out}"

cd "${REPO}/fuzz"

for t in "${targets[@]}"; do
    mkdir -p "${CORPORA}/${t}"
    echo ""
    echo "=== ${t} (${PER_TARGET_SECONDS}s, ${PARALLEL_WORKERS} workers) ==="
    cargo +nightly fuzz run "${t}" "${CORPORA}/${t}" -- \
        -max_total_time=${PER_TARGET_SECONDS} \
        -jobs=${PARALLEL_WORKERS} \
        -workers=${PARALLEL_WORKERS} \
        -print_final_stats=1 \
        2>&1 | tee "${out}/${t}.log" || echo "${t} exited non-zero, continuing"
done

echo ""
echo "=== nightly campaign complete: ${out} ==="

# Surface a crash count to journal so systemctl status shows something useful.
crashes=$(find "${FUZZ_ROOT}/artifacts" -type f -newer "${out}" 2>/dev/null | wc -l)
echo "    new artifact files this run: ${crashes}"
