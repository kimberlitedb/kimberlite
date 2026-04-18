# New-Session Prompt: Replace chaos shim with real `kimberlite-server`

Paste everything below this line into a fresh Claude Code session.

---

Kimberlite's chaos-testing tier (`crates/kimberlite-chaos`) runs real KVM VMs
on our EPYC Hetzner box and injects faults (iptables partitions, kill/restart,
disk fill, clock skew) to verify cluster-level invariants. It works
end-to-end: 6 scenarios pass on every run
(`just epyc-chaos-all` on EPYC).

**The problem:** those VMs don't run the real Kimberlite binary. They run a
deliberate ~500-line stand-in at `crates/kimberlite-chaos-shim/src/main.rs`
that mimics Kimberlite's HTTP probe surface (`/health`,
`POST /kv/chaos-probe`, `/state/{commit_hash,write_log,commit_watermark}`)
with a hand-rolled sync-replication-before-ack loop. As a result, the chaos
tier today tests the *harness's model of Kimberlite*, not Kimberlite itself.

**Why the shim exists:** `kimberlite-server` can't be compiled as a
musl-static binary because `libduckdb-sys` (a transitive dep via
`kimberlite-query`) needs a C++ cross-compiler for
`x86_64-unknown-linux-musl`. Ubuntu's repositories don't ship
`x86_64-linux-musl-g++`, so the chaos VM image builder
(`tools/chaos/build-vm-image.sh`) falls back to the shim as the only binary
it can reasonably embed in an Alpine rootfs. Also documented in memory:
`macOS x86_64 cross: use regular cargo build; aws-lc-sys/libduckdb-sys C++
deps break with zigbuild`.

**Your goal:** get `kimberlite-server` running inside the chaos VMs so the
invariant checks (`minority_refuses_writes`, `no_divergence_after_heal`,
`all_writes_preserved`, `commit_watermark_consistent`, `linearizability`,
etc.) actually exercise Kimberlite's real VSR consensus, storage, and
compliance code paths. Success = `just epyc-chaos-all` passes against the
real binary for at least 5 of 6 scenarios (`storage_exhaustion` may need
scenario tuning for real disk-full semantics).

**Plausible approaches to evaluate (pick whichever is most tractable —
don't commit to a path before understanding the trade-offs):**

1. **Glibc base image.** Switch the chaos rootfs from Alpine (musl) to a
   slim Debian or Ubuntu minimal (glibc). Build `kimberlite-server` with
   the default `x86_64-unknown-linux-gnu` target (no musl cross), copy it
   + its dynamic deps into the rootfs. Requires a larger image
   (~150 MiB vs 26 MiB), but everything compiles with stock Rust + host
   gcc/g++. Update `tools/chaos/build-vm-image.sh` accordingly.

2. **Static-libstdc++.** Keep musl as the C target but configure
   `libduckdb-sys`'s build to statically link `libstdc++.a` (or
   `libc++.a`). Needs a C++ toolchain that produces musl-compatible
   static archives. AUR has `x86_64-linux-musl-gcc`/`g++` packages;
   alternatively build them from source via musl-cross-make. This keeps
   the image small (Alpine works as-is) but is the most fiddly.

3. **Chaos sidecar feature flag.** Regardless of (1) or (2), the real
   `kimberlite-server` doesn't currently serve `POST /kv/chaos-probe`.
   Either (a) add these endpoints behind a `cfg(feature = "chaos")` in
   `crates/kimberlite-server/src/http.rs` so the chaos image can enable
   them, OR (b) adapt `InvariantChecker` to use Kimberlite's native
   binary protocol (`kimberlite-wire`) instead of the ad-hoc HTTP probe
   surface. Option (a) is smaller work and preserves the harness
   architecture; option (b) is more principled.

4. **Alternative DB backend (escape hatch).** If DuckDB-via-musl proves
   genuinely intractable, `kimberlite-query` could be made optional for
   the chaos binary (a minimal kimberlite-server build without the query
   engine). Probably overkill — investigate (1) first.

**Key files to read first:**

- `crates/kimberlite-chaos-shim/src/main.rs` — the current HTTP contract
  the chaos harness expects. Your new binary must serve this surface (or
  the harness must be updated to speak the real wire protocol).
- `crates/kimberlite-chaos/src/invariant_checker.rs` — defines the probe
  semantics (what the checkers actually ask for).
- `crates/kimberlite-chaos/src/chaos_controller.rs` — how replicas are
  provisioned, including `KMB_PEERS` / `KMB_BIND_ADDR` env vars. For
  multi-cluster scenarios, `provision()` passes all-cluster peers.
- `tools/chaos/build-vm-image.sh` — current image builder. You'll probably
  rewrite substantial parts of this.
- `crates/kimberlite-server/src/http.rs` — existing HTTP sidecar that
  already serves `/health`, `/ready`, `/metrics`. Chaos-probe endpoints
  would naturally live here.
- `crates/kimberlite-server/src/server.rs` — main server entry point.
  Figure out how to run it as a standalone binary in the VM (what
  config it needs, how it's bootstrapped).
- `Cargo.toml` + `crates/kimberlite-query/Cargo.toml` — where DuckDB
  enters the dep tree.

**Don't break:**

- Existing shim tests (`cargo test -p kimberlite-chaos-shim`) — either
  keep the shim as a fallback option or delete it cleanly.
- The existing 6 chaos scenarios — they define the contract the new
  binary must satisfy.
- The systemd weekly timer (`tools/chaos/epyc/kimberlite-chaos-weekly.service`) —
  its `ExecStart` path should still work or be updated.
- VOPR / fuzzing / FV campaigns — these are separate tracks on EPYC
  (`/opt/kimberlite-dst`, `/opt/kimberlite-fuzz`, `/opt/kimberlite-fv`)
  and must not be affected.

**Working environment:**

- EPYC box: `root@142.132.137.52`, source at `/opt/kimberlite-dst/repo`.
  Deploy via `just epyc-deploy`. Build on EPYC via `just epyc-build` (and
  related).
- Local Mac: develop + test locally, rsync to EPYC for actual VM runs.
- Everything can run locally up to the VM-image build, which needs Linux
  (nbd + mkfs.ext4 + qemu-img). Run image builds on EPYC.

**Suggested first 30 minutes:**

1. Read the shim's HTTP contract thoroughly.
2. Try `cargo build --release -p kimberlite-server` (default x86_64-linux-gnu
   target) to confirm it builds without cross-compile gymnastics.
3. Check the binary's size + dynamic library deps (`ldd
   target/release/kimberlite-server`). This tells you whether option (1)
   (glibc base image) is viable.
4. Come back with a recommendation before writing code.

**Don't:**

- Try to make the chaos shim more capable. Its job is to emulate the
  contract; your job is to serve the contract from the real binary.
- Reimplement VSR in the shim. The real binary already has VSR.
- Touch the VOPR simulator (`crates/kimberlite-sim`) — the chaos tier is
  a separate track.
- Break the existing passing-tests baseline — the 6 chaos scenarios
  must still pass with the current shim right up until you flip the
  image builder over to the real binary.

---

End of prompt.
