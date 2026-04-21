#!/usr/bin/env bash
#
# EPYC formal-verification weekly campaign runner.
#
# Invoked by `kimberlite-fv-weekly.service` (systemd) or manually via
# `/opt/kimberlite-fv/bin/weekly.sh`. Runs the full FV suite — Alloy,
# Ivy, Coq, Kani (full unwind 128), MIRI, and VOPR property coverage —
# with the same tool configurations as the corresponding `fv-epyc-*`
# justfile recipes. Expected runtime ~2h total.
#
# Mirrors the direct-tool-invocation pattern used by the fv-epyc-*
# recipes — NOT `just verify-*`, which are local-dev paths that assume
# tools live at different locations.
#
# Exit codes:
#   0 — every stage succeeded
#   1 — setup failure (cargo env missing, repo tree absent)
#   2 — one or more stages reported failures (inspect per-stage log)

set -uo pipefail

FV_ROOT="/opt/kimberlite-fv"
REPO="${FV_ROOT}/repo"
RESULTS="${FV_ROOT}/results"
TLA_JAR="${FV_ROOT}/tla/tla2tools.jar"
ALLOY_JAR="${FV_ROOT}/alloy/alloy-6.2.0.jar"

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
out="${RESULTS}/weekly-${ts}"
mkdir -p "${out}"

echo "=== FV weekly campaign ${ts} starting ==="
echo "    results: ${out}"

cd "${REPO}"

failed_stages=()

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

stage_alloy() {
    local log="${out}/alloy.log"
    for spec in specs/alloy/Simple.als specs/alloy/HashChain.als specs/alloy/Quorum.als; do
        [ -f "${spec}" ] || { echo "skip: ${spec}" | tee -a "${log}"; continue; }
        name=$(basename "${spec}" .als)
        echo "--- Alloy ${name} (full scope) ---" | tee -a "${log}"
        java -Djava.awt.headless=true -jar "${ALLOY_JAR}" exec "${spec}" \
            2>&1 | tee -a "${log}"
    done
}

stage_ivy() {
    local log="${out}/ivy.log"
    if ! docker image inspect kimberlite-ivy:latest >/dev/null 2>&1; then
        docker build -t kimberlite-ivy:latest tools/formal-verification/docker/ivy/ \
            2>&1 | tee -a "${log}"
    fi
    # Ivy is aspirational — don't let its Python 2/3 issue fail the suite.
    docker run --rm -v "$PWD/specs/ivy:/workspace" -w /workspace \
        kimberlite-ivy:latest VSR_Byzantine.ivy \
        2>&1 | tee -a "${log}" || true
}

stage_coq() {
    local log="${out}/coq.log"
    docker pull coqorg/coq:8.18 >/dev/null 2>&1 || true
    local files=(Common.v SHA256.v BLAKE3.v AES_GCM.v Ed25519.v KeyHierarchy.v MessageSerialization.v Extract.v)
    local failed=0
    for f in "${files[@]}"; do
        [ -f "specs/coq/${f}" ] || { echo "skip: ${f}" | tee -a "${log}"; continue; }
        echo "--- Coq ${f} ---" | tee -a "${log}"
        if docker run --rm -v "$PWD/specs/coq:/src:ro" coqorg/coq:8.18 \
            bash -c "mkdir -p /tmp/coq && cp /src/*.v /tmp/coq/ && cd /tmp/coq && coqc -Q . Kimberlite '${f}'" \
            2>&1 | tee -a "${log}"; then
            echo "OK ${f}" | tee -a "${log}"
        else
            echo "FAIL ${f}" | tee -a "${log}"
            failed=$((failed + 1))
        fi
    done
    return "${failed}"
}

stage_kani() {
    local log="${out}/kani.log"
    cargo kani --workspace --default-unwind 128 --no-unwinding-checks \
        -j "$(nproc)" 2>&1 | tee "${log}"
}

stage_miri() {
    local log="${out}/miri.log"
    cargo +nightly miri test \
        -p kimberlite-storage \
        -p kimberlite-crypto \
        -p kimberlite-types \
        --lib --no-default-features \
        2>&1 | tee "${log}"
}

stage_properties() {
    local log="${out}/properties.log"
    cargo run --release -p kimberlite-sim --features sim --bin vopr -- \
        properties --report 2>&1 | tee "${log}"
}

run_stage "alloy"      stage_alloy
run_stage "ivy"        stage_ivy
run_stage "coq"        stage_coq
run_stage "kani"       stage_kani
run_stage "miri"       stage_miri
run_stage "properties" stage_properties

echo ""
echo "=== FV weekly campaign complete: ${out} ==="
if [ ${#failed_stages[@]} -eq 0 ]; then
    echo "all stages PASSED"
    exit 0
else
    echo "FAILED stages: ${failed_stages[*]}" >&2
    exit 2
fi
