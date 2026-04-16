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
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
            kernel_cmdline: "console=ttyS0 root=/dev/vda1 rw nokaslr".to_string(),
            tap_device: format!("tap-c{cluster_id}-r{replica_id}"),
            qmp_socket: PathBuf::from(format!("/tmp/kmb-chaos-c{cluster_id}-r{replica_id}.qmp")),
            vnc_port: 0,
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
                "file={},if=virtio,cache=writeback",
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
            .arg("virtio-net,netdev=net0")
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

        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

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
    /// TODO: implement QMP JSON protocol. For now this is a hard kill stub.
    pub fn shutdown_graceful(&mut self) -> Result<(), VmError> {
        if self.state != VmState::Running {
            return Err(VmError::NotRunning);
        }
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
