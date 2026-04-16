//! Host-side network controller for chaos VMs.
//!
//! Manages Linux bridges, tap devices, iptables rules for partitions, and
//! tc qdisc rules for delay/loss injection.

use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// Host-side network controller for one or more chaos clusters.
#[derive(Debug, Default)]
pub struct NetworkController {
    bridges: Vec<BridgeConfig>,
    active_partitions: Vec<PartitionRule>,
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

    /// Creates a Linux bridge via `ip link`.
    ///
    /// TODO(chaos-runner): wire to actual `ip` command on EPYC host; currently
    /// this is a dry-run that records the config but does not touch the host.
    pub fn create_bridge(&mut self, config: BridgeConfig) -> Result<(), NetworkError> {
        tracing::info!(bridge = %config.name, subnet = %config.subnet, "create bridge (dry-run)");
        // Dry-run: record config without executing.
        self.bridges.push(config);
        Ok(())
    }

    /// Partitions network between two replicas by inserting iptables DROP rules.
    ///
    /// TODO(chaos-runner): execute actual iptables commands.
    pub fn partition(
        &mut self,
        from_replica: &str,
        to_replica: &str,
    ) -> Result<u64, NetworkError> {
        let rule_id = self.active_partitions.len() as u64;
        tracing::info!(from = %from_replica, to = %to_replica, rule = rule_id, "partition (dry-run)");
        self.active_partitions.push(PartitionRule {
            from_replica: from_replica.to_string(),
            to_replica: to_replica.to_string(),
            rule_id,
        });
        Ok(rule_id)
    }

    /// Heals a partition by removing the iptables rule.
    pub fn heal(&mut self, rule_id: u64) -> Result<(), NetworkError> {
        if let Some(idx) = self
            .active_partitions
            .iter()
            .position(|r| r.rule_id == rule_id)
        {
            let rule = self.active_partitions.remove(idx);
            tracing::info!(from = %rule.from_replica, to = %rule.to_replica, "heal partition (dry-run)");
        }
        Ok(())
    }

    /// Injects network delay/loss via tc netem.
    ///
    /// TODO(chaos-runner): execute `tc qdisc add dev <bridge> root netem ...`.
    pub fn add_netem(
        &mut self,
        bridge: &str,
        delay_ms: u32,
        loss_percent: f32,
    ) -> Result<(), NetworkError> {
        tracing::info!(bridge, delay_ms, loss_percent, "netem (dry-run)");
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
}
