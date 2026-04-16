//! QEMU/KVM VM lifecycle management for chaos testing.
//!
//! Each VM hosts one kimberlite-server replica. This module wraps
//! `qemu-system-x86_64` as a child process and provides:
//!
//! - Boot from an Alpine+kimberlite disk image
//! - Graceful shutdown via QMP (QEMU Machine Protocol)
//! - Hard kill (for chaos scenarios)
//! - Disk corruption injection (direct writes to the qcow2 backing file
//!   when the VM is stopped, or via the QEMU monitor when running)

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::qmp::{QmpClient, QmpError};

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum VmError {
    #[error("failed to spawn qemu: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("VM failed to boot within {0:?}")]
    BootTimeout(Duration),
    #[error("VM is not running")]
    NotRunning,
    #[error("VM is already running")]
    AlreadyRunning,
    #[error("QMP command failed: {0}")]
    QmpFailed(String),
}

impl From<QmpError> for VmError {
    fn from(e: QmpError) -> Self {
        VmError::QmpFailed(e.to_string())
    }
}

// ============================================================================
// VM State
// ============================================================================

/// Lifecycle state of a chaos VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmState {
    /// Never started.
    NotStarted,
    /// Running and reachable via QMP.
    Running,
    /// Shut down cleanly.
    Stopped,
    /// Crashed or killed hard.
    Crashed,
}

// ============================================================================
// VM Specification
// ============================================================================

/// Parameters for spawning a kimberlite chaos VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmSpec {
    /// Replica identifier within the cluster (0, 1, 2).
    pub replica_id: u8,
    /// Cluster identifier (for multi-cluster scenarios).
    pub cluster_id: u16,
    /// Memory in MiB.
    pub memory_mb: u32,
    /// vCPU count.
    pub vcpus: u32,
    /// Path to the disk image (qcow2).
    pub disk_image: PathBuf,
    /// Path to the kernel (bzImage).
    pub kernel_image: PathBuf,
    /// Path to the initramfs.
    pub initrd: Option<PathBuf>,
    /// Kernel command-line.
    pub kernel_cmdline: String,
    /// Tap device name to attach to the cluster bridge.
    pub tap_device: String,
    /// QMP UNIX socket path (for control).
    pub qmp_socket: PathBuf,
    /// VNC display port offset (for debugging; 0 = disabled).
    pub vnc_port: u16,
    /// Optional path to append QEMU serial console output. If `None`, the
    /// console is written to stdout.
    pub console_log: Option<PathBuf>,
}

impl VmSpec {
    /// Creates a default-sized spec (2GiB, 2 vCPU, reasonable kimberlite workload).
    #[must_use]
    pub fn new(cluster_id: u16, replica_id: u8, disk: PathBuf, kernel: PathBuf) -> Self {
        Self {
            replica_id,
            cluster_id,
            memory_mb: 2048,
            vcpus: 2,
            disk_image: disk,
            kernel_image: kernel,
            initrd: None,
            kernel_cmdline: "console=ttyS0 root=/dev/vda rw nokaslr panic=5".to_string(),
            tap_device: format!("tap-c{cluster_id}-r{replica_id}"),
            qmp_socket: PathBuf::from(format!("/tmp/kmb-chaos-c{cluster_id}-r{replica_id}.qmp")),
            vnc_port: 0,
            console_log: None,
        }
    }
}

// ============================================================================
// Cluster VM
// ============================================================================

/// A running (or stoppable) kimberlite replica VM.
#[derive(Debug)]
pub struct ClusterVm {
    spec: VmSpec,
    state: VmState,
    qemu_process: Option<Child>,
}

impl ClusterVm {
    /// Creates a new VM (not yet started).
    #[must_use]
    pub fn new(spec: VmSpec) -> Self {
        Self {
            spec,
            state: VmState::NotStarted,
            qemu_process: None,
        }
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub fn state(&self) -> VmState {
        self.state
    }

    /// Returns the spec.
    #[must_use]
    pub fn spec(&self) -> &VmSpec {
        &self.spec
    }

    /// Spawns the QEMU process, booting the VM.
    ///
    /// Returns once QEMU has started; the VM may not yet have finished booting
    /// the guest OS. Use [`Self::wait_for_ready`] to block until the kimberlite
    /// HTTP health check responds.
    pub fn boot(&mut self) -> Result<(), VmError> {
        if self.state == VmState::Running {
            return Err(VmError::AlreadyRunning);
        }

        let mut cmd = Command::new("qemu-system-x86_64");
        cmd.arg("-enable-kvm")
            .arg("-cpu")
            .arg("host")
            .arg("-smp")
            .arg(self.spec.vcpus.to_string())
            .arg("-m")
            .arg(format!("{}M", self.spec.memory_mb))
            .arg("-drive")
            .arg(format!(
                // cache=writethrough: writes go to both the host page cache and the
                // storage device before the guest sees them as complete. Prevents
                // data loss when the QEMU process is hard-killed (kill -9), which
                // is what KillReplica does. Slower than writeback but essential for
                // the persistent write-log invariant checks to work across restarts.
                "file={},if=virtio,cache=writethrough",
                self.spec.disk_image.display()
            ))
            .arg("-kernel")
            .arg(&self.spec.kernel_image)
            .arg("-append")
            .arg(&self.spec.kernel_cmdline)
            .arg("-netdev")
            .arg(format!(
                "tap,id=net0,ifname={},script=no,downscript=no",
                self.spec.tap_device
            ))
            .arg("-device")
            .arg(format!(
                "virtio-net,netdev=net0,mac={}",
                virtio_mac(self.spec.cluster_id, self.spec.replica_id)
            ))
            .arg("-qmp")
            .arg(format!(
                "unix:{},server,nowait",
                self.spec.qmp_socket.display()
            ))
            .arg("-display")
            .arg("none")
            .arg("-serial")
            .arg("stdio");

        if let Some(ref initrd) = self.spec.initrd {
            cmd.arg("-initrd").arg(initrd);
        }
        if self.spec.vnc_port > 0 {
            cmd.arg("-vnc").arg(format!(":{}", self.spec.vnc_port));
        }

        cmd.stdin(Stdio::null());

        // If a console log path is configured, redirect QEMU's -serial stdio
        // output there so scenarios can capture post-mortem boot logs.
        if let Some(ref path) = self.spec.console_log {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                Ok(f) => {
                    let stderr_handle = match f.try_clone() {
                        Ok(dup) => Stdio::from(dup),
                        Err(_) => Stdio::null(),
                    };
                    cmd.stdout(Stdio::from(f)).stderr(stderr_handle);
                }
                Err(e) => {
                    tracing::warn!(err = %e, path = %path.display(), "failed to open console log; falling back to null");
                    cmd.stdout(Stdio::null()).stderr(Stdio::null());
                }
            }
        } else {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }

        let child = cmd.spawn()?;
        self.qemu_process = Some(child);
        self.state = VmState::Running;
        Ok(())
    }

    /// Blocks until the VM responds to a health check, or times out.
    ///
    /// Currently this is a placeholder that just sleeps for a boot budget. A
    /// full implementation would poll an HTTP endpoint or listen for a serial
    /// readiness message.
    pub fn wait_for_ready(&self, timeout: Duration) -> Result<(), VmError> {
        if self.state != VmState::Running {
            return Err(VmError::NotRunning);
        }
        // TODO: replace with HTTP /healthz poll once kimberlite-server is in the image.
        std::thread::sleep(timeout.min(Duration::from_secs(5)));
        Ok(())
    }

    /// Gracefully shuts down the VM via QMP `system_powerdown`.
    ///
    /// Connects to the QMP UNIX socket, negotiates capabilities, issues
    /// `system_powerdown` (ACPI powerdown — clean shutdown from the guest
    /// side), then polls the QEMU process for up to `timeout` before
    /// falling back to [`kill_hard`] if the guest ignored the request.
    ///
    /// If QMP is unreachable (socket missing, handshake fails, guest
    /// missing acpid), the call still transitions to Stopped after the
    /// fallback — graceful shutdown is best-effort.
    pub fn shutdown_graceful(&mut self) -> Result<(), VmError> {
        self.shutdown_graceful_with_timeout(Duration::from_secs(5))
    }

    /// Variant that accepts an explicit timeout.
    pub fn shutdown_graceful_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<(), VmError> {
        if self.state != VmState::Running {
            return Err(VmError::NotRunning);
        }

        let qmp_ok = match QmpClient::connect(&self.spec.qmp_socket) {
            Ok(mut client) => {
                if let Err(e) = client.system_powerdown() {
                    tracing::warn!(err = %e, "system_powerdown failed; falling back to kill_hard");
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                tracing::warn!(err = %e, "QMP connect failed; falling back to kill_hard");
                false
            }
        };

        if qmp_ok {
            let deadline = Instant::now() + timeout;
            if let Some(child) = self.qemu_process.as_mut() {
                loop {
                    match child.try_wait() {
                        Ok(Some(_status)) => {
                            // Process exited cleanly.
                            self.qemu_process.take();
                            self.state = VmState::Stopped;
                            return Ok(());
                        }
                        Ok(None) => {
                            if Instant::now() >= deadline {
                                tracing::warn!(
                                    "graceful shutdown timed out after {:?}; falling back",
                                    timeout
                                );
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, "try_wait failed; falling back to kill_hard");
                            break;
                        }
                    }
                }
            }
        }

        // Fallback: hard kill.
        self.kill_hard()?;
        self.state = VmState::Stopped;
        Ok(())
    }

    /// Kills the VM process immediately (SIGKILL).
    ///
    /// Simulates a hard crash: power cut, kernel panic, etc.
    pub fn kill_hard(&mut self) -> Result<(), VmError> {
        if let Some(mut child) = self.qemu_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.state = VmState::Crashed;
        Ok(())
    }
}

/// Deterministically derives a unique locally-administered MAC address from
/// a replica's (cluster, replica) coordinates. Prefix `52:54:00` is the
/// QEMU vendor OUI; remaining three octets encode cluster/replica so every
/// VM on the same host bridge has a distinct MAC.
fn virtio_mac(cluster: u16, replica: u8) -> String {
    let hi = ((cluster >> 8) & 0xff) as u8;
    let lo = (cluster & 0xff) as u8;
    format!("52:54:00:{hi:02x}:{lo:02x}:{replica:02x}")
}

impl Drop for ClusterVm {
    fn drop(&mut self) {
        if self.state == VmState::Running {
            let _ = self.kill_hard();
        }
        // Best-effort cleanup of QMP socket.
        let _ = std::fs::remove_file(&self.spec.qmp_socket);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_spec_defaults_are_reasonable() {
        let spec = VmSpec::new(
            1,
            2,
            PathBuf::from("/tmp/disk.qcow2"),
            PathBuf::from("/tmp/bzImage"),
        );
        assert_eq!(spec.cluster_id, 1);
        assert_eq!(spec.replica_id, 2);
        assert_eq!(spec.memory_mb, 2048);
        assert_eq!(spec.tap_device, "tap-c1-r2");
    }

    #[test]
    fn vm_starts_not_started() {
        let spec = VmSpec::new(
            0,
            0,
            PathBuf::from("/tmp/disk.qcow2"),
            PathBuf::from("/tmp/bzImage"),
        );
        let vm = ClusterVm::new(spec);
        assert_eq!(vm.state(), VmState::NotStarted);
    }
}
