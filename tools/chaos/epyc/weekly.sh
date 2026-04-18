#!/usr/bin/env bash
# Weekly chaos-campaign entry point invoked by kimberlite-chaos-weekly.service.
#
# Runs all 6 built-in scenarios sequentially against the EPYC host's KVM,
# using the REAL `kimberlite-server` binary (not the legacy shim). Each
# scenario's artifacts (console logs, report.json, pcap) land under
# /opt/kimberlite-dst/results/weekly-<ts>/<scenario>/ so operators can
# rsync them to a workstation for analysis.
#
# Exits non-zero on the first scenario failure so the systemd unit records
# the failure and stops — bisecting which scenario regressed is easier when
# the journal shows one red scenario rather than six.

set -euo pipefail

readonly REPO="${REPO:-/opt/kimberlite-dst/repo}"
readonly RESULTS_ROOT="${RESULTS_ROOT:-/opt/kimberlite-dst/results}"
readonly VM_IMAGE_DIR="${VM_IMAGE_DIR:-/opt/kimberlite-dst/vm-images}"
readonly SCENARIOS=(
    split_brain_prevention
    rolling_restart_under_load
    leader_kill_mid_commit
    independent_cluster_isolation
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

# Build the real kimberlite CLI binary (includes kimberlite-server).
# `kimberlite-chaos` is the scenario orchestrator on the host. Both must
# be rebuilt in case the repo state drifted from the last weekly run.
log "building kimberlite + kimberlite-chaos (release)"
cargo build --release -p kimberlite-cli -p kimberlite-chaos 2>&1 | tail -5

# Rebuild VM images so the clones pick up the just-built binary + any
# rootfs changes. Cheap via cp --reflink=auto once the base is ready.
log "rebuilding VM images"
bash "${REPO}/tools/chaos/build-vm-image.sh" 2>&1 | tail -5

# Run each scenario. Between scenarios, reset qcow2 disks from base.qcow2
# so VSR superblock + chaos write log state doesn't leak forward. Collect
# exit status; bail on the first failure.
failed=""
for scenario in "${SCENARIOS[@]}"; do
    scenario_out="${out_root}/${scenario}"
    mkdir -p "${scenario_out}"
    log "=== scenario=${scenario} ==="

    # Reset replica disks so each scenario starts from a clean rootfs.
    if [[ -f "${VM_IMAGE_DIR}/base.qcow2" ]]; then
        for c in 0 1; do
            for r in 0 1 2; do
                cp --reflink=auto \
                    "${VM_IMAGE_DIR}/base.qcow2" \
                    "${VM_IMAGE_DIR}/replica-c${c}-r${r}.qcow2"
            done
        done
    fi

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
