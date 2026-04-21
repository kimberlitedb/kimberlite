#!/usr/bin/env bash
#
# EPYC VOPR nightly campaign runner.
#
# Invoked by `kimberlite-vopr-nightly.service` (systemd) or manually via
# `/opt/kimberlite-dst/bin/vopr-nightly.sh`. Runs the `combined` scenario
# for VOPR_ITERATIONS iterations (default 50_000), with determinism
# validated in-band (same seed → same hash across independent runs is
# already enforced by CI, so nightly is a coverage + regression run).
#
# Design:
#   * Fail-open: if VOPR crashes or hits an invariant violation, the .kmb
#     bundle is preserved and the script exits non-zero so systemd marks
#     the service failed for operator triage.
#   * Results land in /opt/kimberlite-dst/results/vopr-nightly-<ts>/ with
#     JSONL output + any generated .kmb bundles.
#   * No source rsync — deploys are explicit (`just epyc-deploy`) so the
#     timer runs a known, tested tree rather than HEAD.
#
# Exit codes:
#   0 — campaign completed with 0 invariant violations
#   1 — setup failure (cargo env missing, repo tree absent)
#   2 — VOPR reported one or more failures (inspect .kmb bundles)

set -uo pipefail

DST_ROOT="/opt/kimberlite-dst"
REPO="${DST_ROOT}/repo"
RESULTS="${DST_ROOT}/results"

VOPR_ITERATIONS=${VOPR_ITERATIONS:-50000}
VOPR_SCENARIO=${VOPR_SCENARIO:-combined}

if [ ! -d "${REPO}/crates/kimberlite-sim" ]; then
    echo "ERROR: ${REPO}/crates/kimberlite-sim does not exist — run 'just epyc-deploy' first" >&2
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
out="${RESULTS}/vopr-nightly-${ts}"
mkdir -p "${out}"

echo "=== VOPR nightly campaign ${ts} starting ==="
echo "    scenario:   ${VOPR_SCENARIO}"
echo "    iterations: ${VOPR_ITERATIONS}"
echo "    results:    ${out}"

cd "${REPO}"

# Build once — `just epyc-deploy` typically precedes a deploy + build,
# but re-run here to catch any mid-week source updates via rsync.
cargo build --release -p kimberlite-sim --bin vopr 2>&1 | tail -20

vopr_log="${out}/vopr.jsonl"
./target/release/vopr \
    -n "${VOPR_ITERATIONS}" \
    --scenario "${VOPR_SCENARIO}" \
    --json \
    > "${vopr_log}" 2>&1
vopr_exit=$?

# Extract a summary for operators. VOPR's jsonl includes a final
# "summary" record; pull it + any failure bundles.
if [ -f "${vopr_log}" ]; then
    tail -5 "${vopr_log}" > "${out}/summary.jsonl" 2>/dev/null || true
fi

# .kmb bundles are written next to the binary (cwd). Move any new ones.
find . -maxdepth 2 -name '*.kmb' -newer "${vopr_log}" -print0 2>/dev/null \
    | xargs -0 -r -I{} mv {} "${out}/" || true

echo "=== nightly campaign complete: ${out} ==="
echo "    vopr exit: ${vopr_exit}"
echo "    bundles:   $(find "${out}" -maxdepth 1 -name '*.kmb' | wc -l)"

if [ "${vopr_exit}" -ne 0 ]; then
    echo "VOPR reported failures — inspect ${out}/ for .kmb bundles" >&2
    exit 2
fi

exit 0
