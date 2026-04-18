#!/usr/bin/env bash
# Weekly chaos-campaign entry point invoked by kimberlite-chaos-weekly.service.
#
# Runs all 6 built-in scenarios sequentially against the EPYC host's KVM.
# Each scenario's artifacts (console logs, report.json, pcap) land under
# /opt/kimberlite-dst/results/weekly-<ts>/<scenario>/ so operators can rsync
# them to a workstation for analysis.
#
# Exits non-zero on the first scenario failure so the systemd unit records
# the failure and stops — bisecting which scenario regressed is easier when
# the journal shows one red scenario rather than six.

set -euo pipefail

readonly REPO="${REPO:-/opt/kimberlite-dst/repo}"
readonly RESULTS_ROOT="${RESULTS_ROOT:-/opt/kimberlite-dst/results}"
readonly SCENARIOS=(
    split_brain_prevention
    rolling_restart_under_load
    leader_kill_mid_commit
    cross_cluster_failover
    cascading_failure
    storage_exhaustion
)

ts="$(date -u +%Y-%m-%d-%H%M%S)"
out_root="${RESULTS_ROOT}/weekly-${ts}"
mkdir -p "${out_root}"

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }

log "weekly chaos campaign ts=${ts}"
log "results: ${out_root}"

# Preflight: kill any stragglers from a prior aborted run. Harmless if clean.
pkill -f 'qemu-system-x86_64.*kimberlite' 2>/dev/null || true
pkill -f 'tcpdump.*10.42' 2>/dev/null || true
for i in 0 1 2 3 4 5; do
    ip link del "kmb-c${i}-br" 2>/dev/null || true
    for r in 0 1 2; do
        ip link del "tap-c${i}-r${r}" 2>/dev/null || true
    done
done
iptables -D FORWARD -j KMB_CHAOS 2>/dev/null || true
iptables -F KMB_CHAOS 2>/dev/null || true
iptables -X KMB_CHAOS 2>/dev/null || true

cd "${REPO}"
# shellcheck disable=SC1091
[[ -f "${HOME}/.cargo/env" ]] && . "${HOME}/.cargo/env"

# Run each scenario. Collect exit status; bail on the first failure.
failed=""
for scenario in "${SCENARIOS[@]}"; do
    scenario_out="${out_root}/${scenario}"
    mkdir -p "${scenario_out}"
    log "=== scenario=${scenario} ==="

    if ./target/release/kimberlite-chaos run "${scenario}" \
           --apply --output-dir "${scenario_out}" \
           >"${scenario_out}/run.log" 2>&1; then
        log "scenario=${scenario} PASSED"
    else
        log "scenario=${scenario} FAILED — see ${scenario_out}/run.log"
        failed="${scenario}"
        break
    fi
done

log "weekly campaign done. results under ${out_root}"
ls -lh "${out_root}" || true

if [[ -n "${failed}" ]]; then
    exit 1
fi
