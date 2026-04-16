//! Host-side network controller for chaos VMs.
//!
//! Manages Linux bridges, tap devices, iptables rules for partitions, and
//! tc qdisc rules for delay/loss injection.
//!
//! # Safety
//!
//! Chaos rules are installed into a dedicated iptables chain `KMB_CHAOS` so
//! that all chaos-induced state is isolated from other host firewall rules
//! and can be cleaned up with a single `iptables -F KMB_CHAOS`.
//!
//! The controller defaults to **dry-run mode** — it logs what it would do
//! without touching host state. To apply real rules, construct with
//! [`NetworkController::with_apply`] or call [`NetworkController::set_apply_mode`].
//! This prevents accidental rule installation on shared hosts.

use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Name of the dedicated iptables chain for chaos rules.
const CHAOS_CHAIN: &str = "KMB_CHAOS";

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("ip command failed: {0}")]
    IpCommand(String),
    #[error("iptables command failed: {0}")]
    Iptables(String),
    #[error("tc command failed: {0}")]
    Tc(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ============================================================================
// Bridge Configuration
// ============================================================================

/// Configuration for a Linux bridge used by a chaos cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Bridge interface name (e.g. "kmb-c1-br").
    pub name: String,
    /// CIDR for the bridge's subnet (e.g. "10.42.0.0/24").
    pub subnet: String,
}

// ============================================================================
// Network Controller
// ============================================================================

/// Execution mode for the network controller.
///
/// `DryRun` (default): logs intended actions without touching host state.
/// `Apply`: executes real `ip`/`iptables`/`tc` commands. Requires root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    DryRun,
    Apply,
}

impl Default for ExecMode {
    fn default() -> Self {
        Self::DryRun
    }
}

/// Host-side network controller for one or more chaos clusters.
#[derive(Debug, Default)]
pub struct NetworkController {
    bridges: Vec<BridgeConfig>,
    taps: Vec<String>,
    active_partitions: Vec<PartitionRule>,
    mode: ExecMode,
    /// Whether we initialized the iptables chain yet (apply mode only).
    chain_initialized: bool,
}

/// An active partition rule (iptables DROP).
#[derive(Debug, Clone)]
struct PartitionRule {
    from_replica: String,
    to_replica: String,
    rule_id: u64,
}

impl NetworkController {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a controller in apply mode — will execute real host commands.
    #[must_use]
    pub fn with_apply() -> Self {
        Self {
            mode: ExecMode::Apply,
            ..Default::default()
        }
    }

    /// Toggles apply/dry-run mode.
    pub fn set_apply_mode(&mut self, apply: bool) {
        self.mode = if apply { ExecMode::Apply } else { ExecMode::DryRun };
    }

    /// Returns the current execution mode.
    #[must_use]
    pub fn mode(&self) -> ExecMode {
        self.mode
    }

    /// Runs a command, returning an error if apply mode and it fails.
    /// Dry-run just logs and returns Ok.
    fn run(&self, program: &str, args: &[&str]) -> Result<(), NetworkError> {
        if self.mode == ExecMode::DryRun {
            tracing::info!(program, ?args, "dry-run");
            return Ok(());
        }
        let output = Command::new(program).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let msg = format!("{program} {args:?} failed: {stderr}");
            return Err(match program {
                "iptables" => NetworkError::Iptables(msg),
                "tc" => NetworkError::Tc(msg),
                _ => NetworkError::IpCommand(msg),
            });
        }
        Ok(())
    }

    /// Ensures the KMB_CHAOS iptables chain exists. Idempotent.
    fn ensure_chain(&mut self) -> Result<(), NetworkError> {
        if self.mode == ExecMode::DryRun || self.chain_initialized {
            return Ok(());
        }
        // Create chain if absent (silently ignore if it exists).
        let _ = Command::new("iptables").args(["-N", CHAOS_CHAIN]).output();
        // Jump FORWARD to our chain (idempotent — check first).
        let check = Command::new("iptables")
            .args(["-C", "FORWARD", "-j", CHAOS_CHAIN])
            .output()?;
        if !check.status.success() {
            self.run("iptables", &["-I", "FORWARD", "-j", CHAOS_CHAIN])?;
        }
        self.chain_initialized = true;
        Ok(())
    }

    /// Creates a Linux bridge via `ip link` and assigns the gateway IP from
    /// the configured subnet (first usable address: `.1` of a `/24`). The
    /// gateway IP is what lets the host reach replica IPs in the subnet —
    /// without it, `http://10.42.0.10:9000` has no route.
    pub fn create_bridge(&mut self, config: BridgeConfig) -> Result<(), NetworkError> {
        tracing::info!(bridge = %config.name, subnet = %config.subnet, mode = ?self.mode, "create bridge");
        if self.mode == ExecMode::Apply {
            // Idempotent: ignore "file exists" error (bridge may already exist).
            let _ = Command::new("ip")
                .args(["link", "add", "name", &config.name, "type", "bridge"])
                .output();
            // Compute the gateway IP from the subnet (X.Y.Z.0/N -> X.Y.Z.1/N).
            if let Some(gateway) = derive_gateway(&config.subnet) {
                // Also idempotent: ignore "file exists" (addr already set).
                let _ = Command::new("ip")
                    .args(["addr", "add", &gateway, "dev", &config.name])
                    .output();
            }
            self.run("ip", &["link", "set", &config.name, "up"])?;
        }
        self.bridges.push(config);
        Ok(())
    }

    /// Creates a tap device and attaches it to the given bridge, then brings
    /// it up. Required for QEMU to reach the bridge — qemu is invoked with
    /// `script=no,downscript=no` so it will not auto-configure the tap.
    ///
    /// Idempotent: if the tap already exists (e.g. leftover from a previous
    /// run) the create step is silently ignored and we still attempt to
    /// attach to the bridge and bring it up.
    pub fn create_tap(&mut self, tap_name: &str, bridge_name: &str) -> Result<(), NetworkError> {
        tracing::info!(tap = %tap_name, bridge = %bridge_name, mode = ?self.mode, "create tap");
        if self.mode == ExecMode::Apply {
            let _ = Command::new("ip")
                .args(["tuntap", "add", "dev", tap_name, "mode", "tap"])
                .output();
            self.run("ip", &["link", "set", "dev", tap_name, "master", bridge_name])?;
            self.run("ip", &["link", "set", "dev", tap_name, "up"])?;
        }
        if !self.taps.iter().any(|t| t == tap_name) {
            self.taps.push(tap_name.to_string());
        }
        Ok(())
    }

    /// Deletes a tap device. Silently ignores a missing device.
    pub fn delete_tap(&mut self, tap_name: &str) -> Result<(), NetworkError> {
        tracing::info!(tap = %tap_name, mode = ?self.mode, "delete tap");
        if self.mode == ExecMode::Apply {
            let _ = Command::new("ip").args(["link", "del", tap_name]).output();
        }
        self.taps.retain(|t| t != tap_name);
        Ok(())
    }

    /// Partitions network between two replicas by inserting iptables DROP rules
    /// into the dedicated `KMB_CHAOS` chain.
    pub fn partition(
        &mut self,
        from_replica: &str,
        to_replica: &str,
    ) -> Result<u64, NetworkError> {
        let rule_id = self.active_partitions.len() as u64;
        tracing::info!(from = %from_replica, to = %to_replica, rule = rule_id, mode = ?self.mode, "partition");

        self.ensure_chain()?;
        if self.mode == ExecMode::Apply {
            // Use a comment tag so we can find and remove this specific rule later.
            let comment = format!("kmb-chaos-{rule_id}-{from_replica}-{to_replica}");
            self.run(
                "iptables",
                &[
                    "-A",
                    CHAOS_CHAIN,
                    "-s",
                    from_replica,
                    "-d",
                    to_replica,
                    "-m",
                    "comment",
                    "--comment",
                    &comment,
                    "-j",
                    "DROP",
                ],
            )?;
        }

        self.active_partitions.push(PartitionRule {
            from_replica: from_replica.to_string(),
            to_replica: to_replica.to_string(),
            rule_id,
        });
        Ok(rule_id)
    }

    /// Heals a partition by removing the iptables rule.
    pub fn heal(&mut self, rule_id: u64) -> Result<(), NetworkError> {
        let Some(idx) = self
            .active_partitions
            .iter()
            .position(|r| r.rule_id == rule_id)
        else {
            return Ok(());
        };
        let rule = self.active_partitions.remove(idx);
        tracing::info!(from = %rule.from_replica, to = %rule.to_replica, mode = ?self.mode, "heal partition");

        if self.mode == ExecMode::Apply {
            let comment = format!(
                "kmb-chaos-{}-{}-{}",
                rule.rule_id, rule.from_replica, rule.to_replica
            );
            self.run(
                "iptables",
                &[
                    "-D",
                    CHAOS_CHAIN,
                    "-s",
                    &rule.from_replica,
                    "-d",
                    &rule.to_replica,
                    "-m",
                    "comment",
                    "--comment",
                    &comment,
                    "-j",
                    "DROP",
                ],
            )?;
        }
        Ok(())
    }

    /// Injects network delay/loss via tc netem on a bridge.
    pub fn add_netem(
        &mut self,
        bridge: &str,
        delay_ms: u32,
        loss_percent: f32,
    ) -> Result<(), NetworkError> {
        tracing::info!(bridge, delay_ms, loss_percent, mode = ?self.mode, "netem");
        if self.mode == ExecMode::Apply {
            let delay_str = format!("{delay_ms}ms");
            let loss_str = format!("{loss_percent}%");
            // `tc qdisc replace` is idempotent (adds if missing, replaces if present).
            self.run(
                "tc",
                &[
                    "qdisc", "replace", "dev", bridge, "root", "netem", "delay", &delay_str,
                    "loss", &loss_str,
                ],
            )?;
        }
        Ok(())
    }

    /// Tears down all chaos network state: removes partitions, taps, bridges.
    pub fn teardown(&mut self) -> Result<(), NetworkError> {
        tracing::info!(mode = ?self.mode, "teardown chaos network state");
        if self.mode == ExecMode::Apply {
            // Flush our chain (removes all partition rules at once).
            let _ = Command::new("iptables").args(["-F", CHAOS_CHAIN]).output();
            // Remove the FORWARD jump.
            let _ = Command::new("iptables")
                .args(["-D", "FORWARD", "-j", CHAOS_CHAIN])
                .output();
            // Delete the chain.
            let _ = Command::new("iptables").args(["-X", CHAOS_CHAIN]).output();
            // Delete taps BEFORE the bridges they're attached to.
            for tap in &self.taps {
                let _ = Command::new("ip").args(["link", "del", tap]).output();
            }
            // Drop bridges.
            for bridge in &self.bridges {
                let _ = Command::new("ip")
                    .args(["link", "del", &bridge.name])
                    .output();
            }
        }
        self.active_partitions.clear();
        self.bridges.clear();
        self.taps.clear();
        self.chain_initialized = false;
        Ok(())
    }

    /// Returns the list of configured bridges.
    #[must_use]
    pub fn bridges(&self) -> &[BridgeConfig] {
        &self.bridges
    }

    /// Returns true if a partition exists between the given replicas.
    #[must_use]
    pub fn is_partitioned(&self, from: &str, to: &str) -> bool {
        self.active_partitions
            .iter()
            .any(|r| r.from_replica == from && r.to_replica == to)
    }
}

/// Given a subnet in CIDR form like `10.42.0.0/24`, returns the first
/// usable gateway address + prefix, e.g. `10.42.0.1/24`. Returns `None`
/// for anything that's not a plausible IPv4 CIDR.
fn derive_gateway(subnet: &str) -> Option<String> {
    let (addr, prefix) = subnet.split_once('/')?;
    let octets: Vec<u8> = addr.split('.').filter_map(|s| s.parse().ok()).collect();
    if octets.len() != 4 {
        return None;
    }
    let gateway = format!(
        "{}.{}.{}.{}/{}",
        octets[0],
        octets[1],
        octets[2],
        octets[3].saturating_add(1),
        prefix
    );
    Some(gateway)
}

/// Checks whether the host has the tools required for network control.
#[must_use]
pub fn host_capabilities_report() -> String {
    let mut lines = Vec::new();
    for tool in &["ip", "iptables", "tc", "qemu-system-x86_64"] {
        let status = Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map_or("MISSING", |_| "OK");
        lines.push(format!("  {tool:20}  {status}"));
    }
    lines.join("\n")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_controller_records_bridges() {
        let mut nc = NetworkController::new();
        nc.create_bridge(BridgeConfig {
            name: "kmb-test-br".into(),
            subnet: "10.42.0.0/24".into(),
        })
        .unwrap();
        assert_eq!(nc.bridges().len(), 1);
        assert_eq!(nc.bridges()[0].name, "kmb-test-br");
    }

    #[test]
    fn partition_and_heal_roundtrip() {
        let mut nc = NetworkController::new();
        let rule_id = nc.partition("r0", "r1").unwrap();
        assert!(nc.is_partitioned("r0", "r1"));
        nc.heal(rule_id).unwrap();
        assert!(!nc.is_partitioned("r0", "r1"));
    }

    #[test]
    fn default_mode_is_dry_run() {
        let nc = NetworkController::new();
        assert_eq!(nc.mode(), ExecMode::DryRun);
    }

    #[test]
    fn with_apply_sets_mode() {
        let nc = NetworkController::with_apply();
        assert_eq!(nc.mode(), ExecMode::Apply);
    }

    #[test]
    fn teardown_clears_state() {
        let mut nc = NetworkController::new();
        nc.create_bridge(BridgeConfig {
            name: "kmb-test".into(),
            subnet: "10.42.0.0/24".into(),
        })
        .unwrap();
        nc.partition("r0", "r1").unwrap();
        nc.teardown().unwrap();
        assert_eq!(nc.bridges().len(), 0);
        assert!(!nc.is_partitioned("r0", "r1"));
    }

    #[test]
    fn tap_tracking_is_idempotent() {
        let mut nc = NetworkController::new();
        nc.create_tap("tap-c0-r0", "kmb-c0-br").unwrap();
        nc.create_tap("tap-c0-r0", "kmb-c0-br").unwrap();
        nc.create_tap("tap-c0-r1", "kmb-c0-br").unwrap();
        assert_eq!(nc.taps.len(), 2);
        nc.delete_tap("tap-c0-r0").unwrap();
        assert_eq!(nc.taps.len(), 1);
        assert!(nc.taps.contains(&"tap-c0-r1".to_string()));
    }

    #[test]
    fn teardown_clears_taps() {
        let mut nc = NetworkController::new();
        nc.create_tap("tap-1", "br").unwrap();
        nc.teardown().unwrap();
        assert!(nc.taps.is_empty());
    }
}
