# EPYC Hetzner Deployment for Formal Verification

**Status:** Infrastructure landed 2026-04-17 (first deploy in progress)
**Server:** `root@142.132.137.52` (shared with DST campaign; separate tree)
**Tree:** `/opt/kimberlite-fv/` (parallel to `/opt/kimberlite-dst/`)
**Specs:** EPYC 7502P (32c/64t), 128GB ECC, FSN1-DC18 Falkenstein

## Context

GitHub Actions free runners can run TLC only against the *Small* configs
(`VSR_Small.cfg`, `HashChain-quick.als`, depth 10). That is enough to catch
type-level regressions but misses deeper state-space bugs. Kani, Coq, and
MIRI are not in PR CI at all.

The EPYC box that runs DST campaigns already has the compute headroom we
need (32c/64t, 128GB) to run the full-scope verification:
- `VSR.cfg` (MaxView=3, MaxOp=4) with `tlc -workers 32 -depth 20`.
- TLAPS on every proof file with `--stretch 10000`.
- `HashChain.als` at scope 10 and `Quorum.als` at scope 8.
- Every Coq proof compiled.
- Kani with `--default-unwind 128 -j$(nproc)`.
- MIRI on `kimberlite-storage`, `kimberlite-crypto`, `kimberlite-types`.
- VOPR with `--features sim` producing a property-annotation report.

We share the same SSH key and host as the DST tree but pin FV to
`/opt/kimberlite-fv/` so results, caches, and Docker images don't collide
with chaos campaign artifacts.

## Directory Layout

```
/opt/kimberlite-fv/
├── repo/                       # rsync'd source tree
├── tla/
│   └── tla2tools.jar           # v1.8.0, SHA-256 pinned by bootstrap.sh
├── alloy/
│   └── alloy-6.2.0.jar         # SHA-256 pinned by bootstrap.sh
├── results/
│   ├── tla-full-<ts>/          # TLC logs per spec
│   ├── tlaps-<ts>/             # TLAPS logs per proof
│   ├── alloy-full-<ts>/        # Alloy logs per spec
│   ├── ivy-<ts>/               # Ivy logs
│   ├── coq-<ts>/               # coqc logs per .v file
│   ├── kani-<ts>/              # cargo kani output
│   ├── miri-<ts>/              # cargo miri output
│   └── properties-<ts>/        # VOPR property-annotation JSONL
└── artifacts/                  # profiling, coverage, future use
```

Matches the convention already used by `/opt/kimberlite-dst/` so operators
only need to learn one mental model.

## Recipe Catalogue

| Recipe | Purpose | Typical wall-clock on EPYC |
|---|---|---|
| `just fv-epyc-deploy` | rsync source, excl. target/.git/.artifacts | <30s |
| `just fv-epyc-setup` | Bootstrap host (Java, Docker, Rust+nightly+miri, Kani, jars, docker images) | ~15 min (first run) |
| `just fv-epyc-smoke` | TLC quick + Alloy quick + Kani unwind 8 (vsr only) | ~5 min |
| `just fv-epyc-tla-full` | TLC on 4 specs, workers 32, depth 20 | ~20 min |
| `just fv-epyc-tlaps-full` | TLAPS with --stretch 10000 | ~60–90 min |
| `just fv-epyc-alloy-full` | Alloy on Simple, HashChain scope 10, Quorum scope 8 | ~15 min |
| `just fv-epyc-ivy` | Ivy Byzantine model (aspirational) | ~5 min |
| `just fv-epyc-coq` | All 8 Coq files | ~10 min |
| `just fv-epyc-kani-full` | Kani workspace, unwind 128, parallel | ~60 min |
| `just fv-epyc-miri` | MIRI on storage/crypto/types libs | ~20 min |
| `just fv-epyc-properties` | VOPR 100k iterations, property JSONL | ~15 min |
| `just fv-epyc-all` | Orchestrator: all of the above sequentially | ~3–4 hours |
| `just fv-epyc-results` | rsync results back to `.artifacts/epyc-fv-results/` | <30s |
| `just fv-epyc-tail` | Tail the most recent FV log | n/a (long-running) |
| `just fv-epyc-status` | uptime, free, docker images, disk usage | <5s |

All recipes are defined in `justfile` under the "EPYC Hetzner
Formal-Verification Targets" section (below the DST block). The FV tree uses
the same `EPYC_HOST` constant but a separate `EPYC_FV_PATH` /
`EPYC_FV_RESULTS`.

## Bootstrap Details

`tools/formal-verification/epyc/bootstrap.sh` (runs as root on EPYC) is
idempotent and pinned:

- **Java 17** (`openjdk-17-jre-headless`) — required for TLC and Alloy.
- **Docker** (`docker.io`) — TLAPS/Ivy/Coq run in Docker.
- **Graphviz dev** — pygraphviz dependency for Ivy.
- **Rust** — stable + nightly toolchains; nightly has `miri` and `rust-src`.
- **Kani** — `cargo install --locked kani-verifier` + `cargo kani setup`.
- **TLA+ tools v1.8.0** — downloaded to `/opt/kimberlite-fv/tla/` with
  SHA-256 `4c1d62e0f67c1d89f833619d7edad9d161e74a54b153f4f81dcef6043ea0d618`.
- **Alloy v6.2.0** — downloaded to `/opt/kimberlite-fv/alloy/` with SHA-256
  `6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d`.
- **Coq image** — `docker pull coqorg/coq:8.18`.
- **TLAPS image** — `docker build tools/formal-verification/docker/tlaps/`.
- **Ivy image** — `docker build tools/formal-verification/docker/ivy/`.

Both jar SHA-256s were previously placeholder hex in
`.github/workflows/formal-verification.yml`; the real hashes above are now
pinned in both CI and the bootstrap script.

## CI vs EPYC Split

| Layer | PR CI (GHA free runner) | Nightly on EPYC |
|---|---|---|
| TLC | VSR_Small.cfg, depth 10 | VSR.cfg, depth 20, workers 32 |
| TLAPS | 3 theorems, stretch 3000–5000 | all proof files, stretch 10000 |
| Alloy | HashChain-quick.als (scope 5) | HashChain.als (scope 10), Quorum.als (scope 8) |
| Ivy | same (aspirational workflow) | same (aspirational) |
| Coq | all 8 files | all 8 files + extraction |
| Kani | workspace, unwind 32 | workspace, unwind 128 |
| MIRI | storage/crypto/types | same |
| VOPR properties | n/a (too long for GHA) | 100k iterations |

PR CI blocks merges on TLC, TLAPS, Alloy, Kani, Coq, MIRI (but not Ivy,
which has a known Python 2/3 compat issue upstream). EPYC runs the full
suite manually today; nightly scheduling via systemd-timer is a follow-up
(see ROADMAP).

## Running Your First Campaign

```bash
# 1) Deploy + bootstrap (one-time)
just fv-epyc-deploy
just fv-epyc-setup              # ~15 min first time, ~5s after

# 2) Smoke the infrastructure
just fv-epyc-smoke              # ~5 min

# 3) Full campaign
just fv-epyc-all                # ~3-4 hours on EPYC

# 4) Pull results back to local
just fv-epyc-results
ls .artifacts/epyc-fv-results/

# 5) Inspect failures
cat .artifacts/epyc-fv-results/kani-*/kani.log
```

## Operational Notes

- **No VM isolation** unlike chaos — FV tools run natively on the host.
- **Docker layer cache** lives under `/var/lib/docker/`. A `docker system
  prune` may be needed monthly to reclaim disk.
- **`target/` cache** lives under `/opt/kimberlite-fv/repo/target/` — not
  purged by `fv-epyc-deploy` (rsync `--delete` excludes `target/`). First
  `cargo kani` compile takes ~10 min; subsequent invocations take ~20s.
- **Logs are timestamped** — no automatic retention. Sweep
  `/opt/kimberlite-fv/results/` quarterly if it grows >50GB.
- **Port exposure** — none. All tools run locally; no network listeners.

## Future Work

- Nightly systemd-timer: `systemctl --user enable fv-epyc-nightly.timer`
  runs `just fv-epyc-all` + `rsync` to a web-accessible dashboard.
- Replace Ivy (Python 2/3 compat rot) with Apalache or a pure TLA+ backend.
- Coq → Rust extraction via `coq-of-rust` or custom extractor (currently we
  wrap proofs in `kimberlite-crypto::verified::*` modules rather than
  auto-extracting).
- Flux refinement-type expansion once `flux-rs` stabilises enough to run in
  CI reliably.

## References

- `/opt/kimberlite-dst/` deployment template: `docs-internal/design-docs/active/dst-epyc-deployment.md`
- Spec inventory: `specs/README.md`
- Traceability matrix: `docs/internals/formal-verification/traceability-matrix.md` (Phase 3)
- Bootstrap source: `tools/formal-verification/epyc/bootstrap.sh`
- Recipes: `justfile` §"EPYC Hetzner Formal-Verification Targets"
