#!/usr/bin/env bash
#
# EPYC formal-verification nightly-lite runner.
#
# Invoked by `kimberlite-fv-nightly-lite.service` (systemd) or manually
# via `/opt/kimberlite-fv/bin/nightly-lite.sh`. Runs only the cheap
# daily-worthy FV checks: MIRI (UB in storage/crypto/types, ~20 min) and
# Kani with unwind=8 (bounded check, ~10 min). Total runtime target:
# under 40 minutes so it fits the 01:00-02:00 UTC window before the
# fuzz nightly.
#
# The heavy proofs (Coq, TLAPS, Kani full unwind) only run weekly — they
# don't benefit from daily execution because specs and proofs change
# rarely.
#
# Uses direct tool invocations matching the fv-epyc-* recipes, NOT
# `just verify-*`.
#
# Exit codes:
#   0 — both stages succeeded
#   1 — setup failure (cargo env missing, repo tree absent)
#   2 — one or more stages failed (inspect per-stage log)

set -uo pipefail

FV_ROOT="/opt/kimberlite-fv"
REPO="${FV_ROOT}/repo"
RESULTS="${FV_ROOT}/results"

if [ ! -d "${REPO}/crates" ]; then
    echo "ERROR: ${REPO}/crates does not exist — run 'just fv-epyc-deploy' first" >&2
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
out="${RESULTS}/nightly-lite-${ts}"
mkdir -p "${out}"

echo "=== FV nightly-lite ${ts} starting ==="
echo "    results: ${out}"

cd "${REPO}"

failed_stages=()

stage_miri() {
    local log="${out}/miri.log"
    # -Zmiri-disable-isolation so (a) proptest's FileFailurePersistence
    # getcwd call doesn't abort the run and (b) PROPTEST_CASES is visible
    # to the interpreted test binary (miri's default isolation blocks
    # env::var reads). PROPTEST_CASES=8 because MIRI interprets generically,
    # so one case catches the same UB as 256 — running 256 cases × ~20
    # proptests × MIRI's interpretation overhead blows the 90m budget.
    MIRIFLAGS="-Zmiri-disable-isolation" \
    PROPTEST_CASES=8 \
    cargo +nightly miri test \
        -p kimberlite-storage \
        -p kimberlite-crypto \
        -p kimberlite-types \
        --lib --no-default-features \
        2>&1 | tee "${log}"
}

stage_kani_smoke() {
    local log="${out}/kani-smoke.log"
    # Unwind 8 is the smoke config — catches most violations quickly.
    # --output-format=terse required when --jobs > 1 (cargo-kani >= 0.55).
    cargo kani --workspace --default-unwind 8 --no-unwinding-checks \
        --output-format=terse -j "$(nproc)" 2>&1 | tee "${log}"
}

run_stage() {
    local name="$1"
    shift
    echo ""
    echo "=== stage: ${name} ==="
    if "$@"; then
        echo "${name}: PASS"
    else
        echo "${name}: FAIL" >&2
        failed_stages+=("${name}")
    fi
}

run_stage "miri"       stage_miri
run_stage "kani-smoke" stage_kani_smoke

echo ""
echo "=== FV nightly-lite complete: ${out} ==="
if [ ${#failed_stages[@]} -eq 0 ]; then
    echo "all stages PASSED"
    exit 0
else
    echo "FAILED stages: ${failed_stages[*]}" >&2
    exit 2
fi
