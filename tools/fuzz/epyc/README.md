# Kimberlite Fuzz — EPYC Campaign Runner

Purpose-built fuzzing campaign that runs on the Hetzner EPYC 7502P alongside
VOPR simulation (`/opt/kimberlite-dst/`) and formal verification
(`/opt/kimberlite-fv/`). Fuzz owns a dedicated tree at
`/opt/kimberlite-fuzz/` so corpora and crash artifacts persist across deploys
without colliding with the other two campaigns.

## Tree layout on EPYC

```
/opt/kimberlite-fuzz/
├── repo/         # rsync'd source tree (deploy target)
├── corpora/      # per-target corpora; grows across runs
├── artifacts/    # crash bundles (raw libFuzzer output)
└── results/      # per-campaign logs (nightly-<ts>/, smoke-<ts>/, etc.)
```

The `fuzz-epyc-deploy` recipe symlinks `/opt/kimberlite-fuzz/repo/fuzz/corpus`
and `.../fuzz/artifacts` to the persistent `corpora/` and `artifacts/` trees
so libFuzzer's default paths land in the right place.

## Tier structure

| Tier | Cadence | Duration | Cores | Recipe |
|------|---------|----------|-------|--------|
| 1    | PR / CI | ~5 min   | 1     | local `just fuzz-smoke` |
| 2    | Nightly | ~3 h     | 12    | `just fuzz-epyc-nightly` |
| 3    | Weekly  | ~24 h    | 16    | `just fuzz-epyc-weekly` |

Core budget against 64 HT total: VOPR bursts ~60 threads for ~30 min,
FV ~32 threads sequential for ~3-4 h. Tier 2 fuzz holds 12 cores
continuously; Tier 3 takes 16.

## First-time bootstrap

```
just fuzz-epyc-bootstrap      # installs rustup + nightly + cargo-fuzz
just fuzz-epyc-deploy          # rsync source tree + wire up corpus symlinks
just fuzz-epyc-smoke           # 60s per target — verifies toolchain
```

## Operating

```
just fuzz-epyc-nightly          # full Tier 2 campaign (run ad-hoc)
just fuzz-epyc-status           # recent runs, corpus sizes, crashes
just fuzz-epyc-tail             # tail the most recent per-target log
just fuzz-epyc-results          # rsync results + artifacts back to .artifacts/
just fuzz-epyc-minimize         # cargo fuzz cmin each corpus (run weekly)
```

## Scheduled nightly via systemd

The nightly campaign runs every day at 02:00 UTC (04:00 CEST on the
Hetzner box) under `kimberlite-fuzz-nightly.timer`. One-time install:

```
just fuzz-epyc-timer-install    # deploy + enable --now
```

This copies `nightly.sh` to `/opt/kimberlite-fuzz/bin/` and the
`.service` + `.timer` unit files to `/etc/systemd/system/`, then
enables the timer.

Operational commands:

```
just fuzz-epyc-timer-status      # timer + last service run + recent journal
just fuzz-epyc-timer-run-now     # trigger campaign via systemd, block until done
just fuzz-epyc-timer-disable     # stop the schedule, keep unit files
```

Timer design rationale:

- **02:00 UTC / 04:00 CEST** — outside apt-daily-upgrade (~06:50 CEST)
  and outside operator hours, so the campaign doesn't compete with
  manually-triggered VOPR or FV runs.
- **`Persistent=true`** — if the box was down at 02:00, catch up on next
  boot rather than silently skipping a night's coverage.
- **`TimeoutStartSec=10800` (3 h)** — a runaway campaign can't pin the
  machine into the next day's scheduling window.
- **`Nice=10`** — yields to latency-sensitive VOPR scenarios that may
  start in parallel during the day.
- **`MemoryHigh=80G` / `MemoryMax=120G`** — 12 ASan fuzz workers per
  target legitimately peak at several GB each; the hard cap prevents
  runaway OOM-amplification from a single bad corpus entry.

Unit files are version-controlled in `tools/fuzz/epyc/`:

- `nightly.sh` — the campaign runner (idempotent, fail-open per target)
- `kimberlite-fuzz-nightly.service` — systemd oneshot wrapper
- `kimberlite-fuzz-nightly.timer` — daily trigger

## Crash workflow

1. `just fuzz-epyc-status` surfaces crashes in
   `/opt/kimberlite-fuzz/artifacts/<target>/crash-*`.
2. `just fuzz-epyc-results` rsyncs artifacts to
   `.artifacts/epyc-fuzz-artifacts/` on your workstation.
3. Reproduce locally: `cargo +nightly fuzz run <target> <crash-file>` from
   `fuzz/`.
4. File a bug with the minimized input. Convert to a regression test case
   under the target's normal corpus.
