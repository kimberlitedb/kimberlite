# Multi-Cluster Chaos Testing via KVM Hypervisor

**Status:** Design only — not yet implemented
**Target crate:** `kimberlite-chaos` (new)
**Target deployment:** EPYC Hetzner server (`root@142.132.137.52`)

## Context

kimberlite-sim models the distributed system at the event-queue level. It
catches protocol bugs (VSR safety, Byzantine detection, hash chain integrity),
but cannot catch bugs that manifest only when:

- Real OS file systems reorder writes differently than the storage model
  predicts (ext4/XFS journal quirks, write barriers).
- Real kernel TCP exhibits backpressure the simulated network does not model.
- Multiple independent clusters coordinate via kimberlite-directory (cross-
  cluster routing is not in the sim).
- The production `kimberlite-server` binary differs from the instrumented
  sim build (compiler optimizations, linker behavior, OS scheduler).

These surfaces are observable only by running real binaries against real
infrastructure, with fault injection coordinated at the hypervisor level.

## Goals

1. Run multiple independent kimberlite clusters as VMs on a single host.
2. Inject faults at the VM network and storage level: kill node, partition
   network, corrupt disk sectors, skew clocks.
3. Exercise scenarios that kimberlite-sim cannot: split-brain, rolling
   restart under load, cross-cluster failover, cascading failure, storage
   exhaustion.
4. Detect violations via external client invariant checks (no sim invariants
   — we run the real binary).

## Architecture

### Topology (on EPYC)

```
Host (EPYC 7502P, 32c/64t, 128GB)
├── VM 0..2: cluster A (3-replica kimberlite)
├── VM 3..5: cluster B (3-replica kimberlite)
├── VM 6..8: optional cluster C
├── bridge0: intra-cluster-A network
├── bridge1: intra-cluster-B network
└── bridge2: inter-cluster directory traffic
```

Up to 8 clusters (48 VMs) fit in 128GB RAM with 2GB per VM.

### VM Lifecycle (adapted from cuddly-duddly)

```rust
pub struct ClusterVm {
    kvm_vm: KvmVm,              // from cuddly-hypervisor::KvmVm
    guest_memory: *mut u8,
    virtio_net: VirtioNet,
    virtio_blk: VirtioBlk,
    cluster_id: ClusterId,
    replica_id: ReplicaId,
    disk_image: PathBuf,         // ext4 image per replica
}

impl ClusterVm {
    pub fn boot(spec: VmSpec) -> Result<Self>;
    pub fn shutdown_graceful(&mut self) -> Result<()>;
    pub fn kill_hard(&mut self) -> Result<()>;        // KVM_EXIT_SHUTDOWN
    pub fn snapshot_memory(&self) -> Vec<u8>;         // KVM dirty page tracking
}
```

Reuses cuddly-duddly's KVM ioctl wrappers (`kvm.rs`) and VM loop (`vm.rs`), but
swaps the guest from "freestanding Rust ELF" to "boot bzImage + Alpine
initramfs + kimberlite-server".

### Fault Injection API

| Fault | Method | Effect |
|---|---|---|
| Kill node | `KVM_EXIT_SHUTDOWN` ioctl | Instant VM death |
| Crash OS | Inject triple fault | Hard crash, test log recovery |
| Network partition | `iptables -I FORWARD -m physdev --physdev-in $vm -j DROP` | Block VM-to-VM |
| Partial partition | `iptables -m statistic --mode random --probability 0.5 -j DROP` | 50% loss |
| Storage corruption | `dd if=/dev/urandom of=disk.img bs=4096 seek=N count=1` | Bit flip at 4KB block N |
| Slow network | `tc qdisc add dev bridge0 root netem delay 100ms 20ms loss 1%` | Realistic jitter |
| Clock skew | `kvm_clock_offset` per VM | NTP-style divergence (up to 1s) |
| Storage exhaustion | Truncate disk image to 95% full | Test graceful degradation |

### Scenarios (priority order)

1. **split_brain_prevention** — Partition 3-node cluster as [2, 1]. Verify
   minority refuses writes. Re-merge. Verify no divergence.
2. **rolling_restart_under_load** — While client runs workload, restart
   replicas 0, 1, 2 sequentially. Verify all writes preserved.
3. **leader_kill_mid_commit** — Kill leader between Prepare and Commit.
   Verify new leader completes the commit (not re-proposes).
4. **cross_cluster_failover** — Kill all replicas in cluster A. Verify
   kimberlite-directory reroutes tenants to cluster B within SLA.
5. **cascading_failure** — Kill replica 0. Before it recovers, kill replica 1
   (now f+1 failures). Verify quorum loss detected and writes refused
   rather than corrupted.
6. **storage_exhaustion** — Fill replica's disk to 95% during workload.
   Verify graceful enforcement, no panic.

## What We Reuse from cuddly-duddly

| Component | Usage |
|---|---|
| `cuddly-hypervisor::kvm` | Direct KVM ioctl wrappers |
| `cuddly-hypervisor::vm` | VM deterministic execution loop |
| `cuddly-hypervisor::kernel_boot` | bzImage loading, IRQCHIP, paging |
| `cuddly-core::ReproTrace` | Event log for fault reproduction |
| cuddly-duddly's ioctl safety patterns | Rust FFI around KVM |

**Not reused:**
- DPOR kernel-source instrumentation (`tools/cuddly_dpor/`) — irrelevant for
  userspace DST.
- Freestanding guest ELF loader — we boot a real Linux kernel.
- Kernel-specific oracle (KASAN, kernel UAF) — we run userspace binaries.

## What We Build New

- `kimberlite-chaos/src/cluster_vm.rs` — higher-level cluster VM abstraction.
- `kimberlite-chaos/src/cluster_network.rs` — iptables/tc-based fault injection.
- `kimberlite-chaos/src/chaos_controller.rs` — scenario orchestration.
- `kimberlite-chaos/src/invariant_checker.rs` — external HTTP client running
  Jepsen-style checks (linearizability, monotonic reads, etc.).
- Alpine + kimberlite-server VM image builder (`justfile` target).

## Determinism Caveat

Unlike kimberlite-sim, real-VM chaos testing **is not fully deterministic**.
Two runs with the same seed can diverge if:
- Host CPU scheduler interleaves VMs differently.
- Real network devices reorder packets differently.
- Real disk I/O completes in different orders.

This is acceptable — we use chaos testing for *breadth of real behaviors*,
not reproduction. When a chaos run fails, we capture:
- Complete disk snapshots of all VMs
- Tcpdump captures of bridge traffic
- Clock skew profile
- Full event log (ChaosController decisions)

This captures enough to reproduce deterministically in sim, which IS the
debug workflow.

## Three-Tier Testing Strategy

```
Tier 1: kimberlite-sim (fast fuzz + DPOR)      → Primary, per-commit
Tier 2: kimberlite-chaos (real binaries)       → Nightly, EPYC server
Tier 3: Antithesis / Jepsen (external audit)   → Future, release gate
```

Tier 2 catches what Tier 1 cannot see (real kernel, real network, real disk,
multi-cluster). Tier 3 provides independent validation once Tier 1+2 mature.

## Why Not Skip Directly to Antithesis?

Antithesis provides everything Tier 2 does (deterministic hypervisor, fault
injection) and more. But:
1. It is external/commercial — latency between test run and audit result.
2. Our own tier 2 gives us faster iteration and full observability.
3. Antithesis complements internal tooling; TigerBeetle uses both.

## Implementation Prerequisites

1. EPYC server SSH access confirmed (done — ed25519 key).
2. KVM enabled, hugepages configured (manual setup required).
3. Alpine + kimberlite-server VM image (buildable from `examples/docker/`).
4. `kimberlite-properties::sim` feature NOT enabled (we run production binaries).

## References

- cuddly-duddly `crates/cuddly-hypervisor/` — KVM plumbing.
- Jepsen test framework — external distributed systems correctness testing.
- Antithesis — deterministic hypervisor DST, gold standard for Tier 3.
