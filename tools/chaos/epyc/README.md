# Kimberlite Chaos — EPYC Weekly Campaign Runner

Weekly KVM-based chaos campaign that runs on the Hetzner EPYC 7502P. Runs
all 6 built-in scenarios against real VMs, exercising fault-injection paths
that in-process VOPR cannot reach (fsync semantics under power loss, kernel
signal handling, actual network partitions via iptables, etc.).

Complements the two other campaign tracks:

- **VOPR DST** (`/opt/kimberlite-dst/`) — deterministic in-process simulation, ad-hoc via `epyc-vopr`
- **Fuzzing** (`/opt/kimberlite-fuzz/`) — libfuzzer nightly via `kimberlite-fuzz-nightly.timer`
- **Chaos** (this) — KVM-based hypervisor-level weekly via `kimberlite-chaos-weekly.timer`

## Schedule

| Timer | Cadence | Duration | Scenarios |
|-------|---------|----------|-----------|
| `kimberlite-chaos-weekly.timer` | Sunday 03:00 UTC | ~20 min typical, 4h cap | All 6 scenarios sequentially |

Runs after the nightly fuzz campaign has completed (fuzz fires daily at
02:00 UTC and takes ~2 hours). The two units are also marked `Conflicts=`
at the service level as a belt-and-braces guard.

## Tree layout on EPYC

```
/opt/kimberlite-dst/
├── repo/                       # rsync'd source tree (deploy target)
├── bin/
│   └── weekly.sh               # campaign entry point (installed by `chaos-epyc-timer-install`)
├── vm-images/                  # qcow2 disks + bzImage (built by `just epyc-build-vm-image`)
└── results/
    └── weekly-<ts>/
        └── <scenario>/         # per-scenario: run.log, report.json, console-c*-r*.log
```

## Commands

All recipes live at the repo root as `just chaos-epyc-*`:

| Recipe | Purpose |
|--------|---------|
| `chaos-epyc-timer-install` | Deploy + enable the weekly timer (idempotent) |
| `chaos-epyc-timer-disable` | Stop scheduling; preserve unit files |
| `chaos-epyc-timer-status` | Show timer + last-run + recent journal |
| `chaos-epyc-timer-run-now` | Kick a run on demand (blocks until done) |

Ad-hoc (non-timer) scenario runs use the existing `epyc-chaos-e2e` recipe,
which is kept for manual bisection work outside the weekly schedule.

## Resource budget

- **Memory**: 2 GiB per VM × 6 VMs = ~12 GiB. Service requests
  `MemoryHigh=40G`, `MemoryMax=64G` to leave generous headroom for QEMU
  overhead and other processes.
- **CPU**: `Nice=10` so interactive work and any concurrent VOPR yields
  take priority.
- **Disk**: Each run produces a pcap + console logs per VM
  (~50–200 MiB/scenario); the `weekly-<ts>` tree persists indefinitely
  until manually pruned.

## Debugging a failing scenario

1. `just chaos-epyc-timer-status` — see which scenario failed in the last run.
2. Rsync the results locally: `rsync -az EPYC:/opt/kimberlite-dst/results/weekly-<ts>/<scenario>/ .`
3. `run.log` shows the controller's action trace. Per-VM `console-c*-r*.log`
   shows the guest kernel + shim output.
4. If the failure is reproducible, `just epyc-chaos-e2e <scenario>` to loop on it.

## Adding a new scenario

1. Add it to `crates/kimberlite-chaos/src/scenarios.rs::builtin_catalog()`.
2. Append its id to the `SCENARIOS` array in `weekly.sh`.
3. Redeploy: `just chaos-epyc-timer-install` (idempotent, picks up the new list).
