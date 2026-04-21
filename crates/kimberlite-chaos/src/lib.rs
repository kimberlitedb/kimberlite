//! Multi-cluster chaos testing orchestration for kimberlite.
//!
//! This crate drives **real kimberlite-server binaries** running inside QEMU/KVM
//! VMs, with fault injection at the hypervisor and host-network level. It
//! complements the userspace simulation in `kimberlite-sim` by exercising:
//!
//! - Real ext4/XFS file system semantics (fsync, journal, write barriers).
//! - Real kernel TCP behavior (backpressure, socket buffers, reordering).
//! - Multi-cluster topologies (kimberlite-directory routing).
//! - Coordinated faults (kill + partition + clock skew together).
//!
//! # Architecture
//!
//! The host spawns one QEMU VM per replica. Each VM boots a minimal Linux
//! kernel + initramfs containing the `kimberlite-server` binary. VMs are
//! connected via host-level bridges (tap devices + Linux bridges), and faults
//! are injected via:
//!
//! - `virsh destroy` / `qemu monitor quit` — kill node hard.
//! - `iptables -I FORWARD` — network partition.
//! - `tc qdisc add` — slow network / packet loss.
//! - `dd` on the VM's disk image — disk corruption.
//! - `qemu-monitor` RTC adjust — clock skew.
//!
//! # Status
//!
//! This is a skeleton. The public API and scenario definitions are stable;
//! the underlying QEMU/KVM control paths are stubbed with clear TODO markers
//! and will be filled in incrementally on the EPYC campaign server.
//!
//! # Requirements (host)
//!
//! - Linux with KVM enabled (`/dev/kvm` accessible)
//! - `qemu-system-x86_64` ≥ 6.0
//! - `bridge-utils` (`ip link add`, `brctl`)
//! - `iptables` + `iproute2` (tc)
//! - Root or CAP_NET_ADMIN for bridge setup
//!
//! # Non-goals
//!
//! - Determinism. Real-VM chaos testing is non-deterministic by design. Use
//!   `kimberlite-sim` for deterministic replay; use `kimberlite-chaos` for
//!   breadth of real-OS behaviors.
//!
//! # Platform support
//!
//! This crate is Linux-only by design: QMP speaks over a UNIX-domain socket
//! (`std::os::unix::net::UnixStream`), bridge setup relies on `ip`/`brctl`,
//! and partitioning drives `iptables`. On non-Unix hosts the crate builds
//! to an empty shell so `cargo check --workspace` on Windows stays green —
//! no target in the workspace depends on the chaos API.

#[cfg(unix)]
pub mod chaos_controller;
#[cfg(unix)]
pub mod cluster_network;
#[cfg(unix)]
pub mod cluster_vm;
#[cfg(unix)]
pub mod invariant_checker;
#[cfg(unix)]
pub mod qmp;
#[cfg(unix)]
pub mod scenarios;

#[cfg(unix)]
pub use chaos_controller::{ChaosController, ChaosError, ChaosReport};
#[cfg(unix)]
pub use cluster_network::{BridgeConfig, NetworkController};
#[cfg(unix)]
pub use cluster_vm::{ClusterVm, VmError, VmSpec, VmState};
#[cfg(unix)]
pub use invariant_checker::{Invariant, InvariantChecker, InvariantResult};
#[cfg(unix)]
pub use scenarios::{ChaosScenario, ScenarioCatalog};
