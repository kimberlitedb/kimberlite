# EPYC Hetzner Deployment for Extended DST Campaigns

**Status:** Infrastructure plan — not yet deployed
**Server:** `root@142.132.137.52` (EPYC 7502P, 128GB ECC, NVMe RAID1)
**Location:** FSN1-DC18, Falkenstein Germany

## Context

Local development machines can run short VOPR campaigns (10k–100k seeds in a
few minutes), but exhaustive coverage needs dedicated hardware:
- Million-seed fuzzing campaigns.
- DPOR exhaustive alternative-trace exploration.
- Multi-cluster chaos testing (8 cluster-pairs simultaneously).
- Overnight/weekly scheduled runs.

The Hetzner EPYC server provides 32c/64t and 128GB ECC, giving ~60× the
throughput of a laptop for embarrassingly parallel fuzzing.

## Target Campaign Configuration

```toml
# dst-campaign.toml
[vopr]
# 60 threads for parallel fuzzing (4 threads reserved for system)
parallel_seeds = 60
seed_range_per_run = 100_000
scenarios = "all"
coverage_threshold = 0.95

[dpor]
# 8 threads for systematic DPOR exploration
parallel_traces = 8
max_interleavings_per_scenario = 10_000
scenarios = ["view_change_safety", "recovery_safety", "byzantine_commit_desync"]

[chaos]
# 16 threads for multi-VM chaos (8 cluster-pairs)
vm_pairs = 8
scenarios = ["split_brain", "rolling_restart", "leader_kill_mid_commit"]
vm_memory_mb = 2048  # 8 × 6 × 2GB = 96GB, within 128GB
```

## Host Setup (one-time)

```bash
# SSH in
ssh root@142.132.137.52

# Verify KVM available (EPYC has kvm_amd)
egrep -c '(vmx|svm)' /proc/cpuinfo        # > 0
lsmod | grep kvm_amd                        # module loaded

# Install prerequisites
apt update
apt install -y build-essential pkg-config libssl-dev libclang-dev clang \
                qemu-kvm qemu-system-x86 bridge-utils iptables tc \
                just git curl

# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Huge pages (64GB worth of 2MB pages for VM memory)
echo 'vm.nr_hugepages = 32768' >> /etc/sysctl.conf
sysctl -p

# IOMMU for future NVMe passthrough
sed -i 's/GRUB_CMDLINE_LINUX=""/GRUB_CMDLINE_LINUX="amd_iommu=on iommu=pt"/' \
       /etc/default/grub
update-grub
# (reboot required)
```

## Directory Layout

```
/opt/kimberlite-dst/
├── repo/                       # git clone of kimberlite
├── bin/                        # release binaries
├── vm-images/                  # Alpine + kimberlite-server images
│   ├── base-alpine.qcow2
│   └── kimberlite-replica.qcow2
├── results/                    # campaign output
│   ├── vopr-YYYYMMDD.jsonl
│   ├── dpor-YYYYMMDD.jsonl
│   └── chaos-YYYYMMDD.jsonl
├── bundles/                    # .kmb failure reproduction bundles
└── artifacts/                  # profiling, coverage
```

## justfile Targets (to be added)

```bash
# Deployment
just epyc-deploy            # rsync source to /opt/kimberlite-dst/repo
just epyc-build             # cargo build --release on server
just epyc-status            # show current campaign status

# Campaign execution
just epyc-vopr N            # run N fuzzing iterations (parallel 60-way)
just epyc-dpor              # run DPOR exploration (8-way)
just epyc-chaos             # run chaos scenarios (16-way)
just epyc-full              # run all tiers overnight

# Results & debugging
just epyc-results           # rsync .kmb bundles back to local
just epyc-logs              # tail current campaign log
just epyc-reproduce N.kmb   # reproduce failure N.kmb on local dev box
```

Each target SSH's into the server and runs the corresponding campaign under
tmux/systemd so campaigns survive disconnects. Results are appended to JSONL
files for easy grep / jq analysis.

## Scheduling Strategy

**Nightly (00:00 UTC):**
- 1M VOPR seeds across all scenarios, 60-way parallel (~30 min wall clock)
- DPOR exploration on top-10 scenarios (~30 min)
- Results stored in `results/vopr-$(date +%Y%m%d).jsonl`

**Weekly (Sunday):**
- Multi-cluster chaos suite (16-way, 6 scenarios × 10 iterations each, ~2 hours)
- Cross-version upgrade/downgrade tests
- Storage corruption + recovery validation
- Results: `results/chaos-$(date +%Y%m%d).jsonl`

**On-demand (manual):**
- Reproduction runs after a failure bundle is captured (seconds).
- Coverage campaigns when new scenarios are added.

## Result Collection

After each campaign, a cron job rsyncs new `.kmb` bundles and summary JSONL
back to the development machine. Failures trigger a Slack webhook (future).

Format:
```json
{"ts": "2026-04-17T00:15:42Z", "campaign": "vopr-nightly",
 "scenario": "view_change_safety", "seed": 4892341,
 "outcome": "invariant_violation", "invariant": "vsr.view_change_commit_le_op",
 "bundle": "bundles/vopr-20260417/seed-4892341.kmb"}
```

## Cost/Throughput Estimates

Hetzner server: €109/month flat (as of 2026-04).

Throughput at 85k–167k sims/sec × 60 cores:
- 5M–10M seeds per hour
- ~150M seeds per day if running 24/7
- For comparison: TigerBeetle's VOPR runs are 1000× speedup per sim, so 1 day
  ≈ 2700 years of simulated time. We would achieve similar after optimizing
  kimberlite-sim hot paths (currently 85k sims/sec is on-par with an old
  TigerBeetle baseline).

## Security Considerations

- Server has no public inbound SSH beyond port 22 (ed25519 key only).
- VMs are on private bridges — no Internet egress.
- Artifacts shipped via rsync over SSH.
- No secrets stored on server (campaign binaries only).
- Failure bundles contain event logs but no production data (simulated only).

## Not Currently Included

- GitHub Actions integration (campaigns too long for free runners).
- Public-facing result dashboard.
- Antithesis integration (future, paid service).
- GPU for coverage-guided fuzzing with learned models.

## References

- Hetzner AX102 specs: https://www.hetzner.com/dedicated-rootserver/ax102/
- Kimberlite VOPR documentation: `docs-internal/vopr/`
- Memory file: `reference_epyc_server.md` in user's auto-memory.
