//! Chaos scenario catalog for multi-cluster real-binary testing.
//!
//! Each scenario is a sequence of actions against a pre-provisioned cluster.
//! Scenarios are deliberately under-specified (timings are windows, not exact
//! instants) — they test robustness under realistic non-determinism.

use serde::{Deserialize, Serialize};

// ============================================================================
// Scenario Definition
// ============================================================================

/// A high-level chaos scenario.
///
/// Scenarios are data-defined so that they can be loaded from JSON/TOML at
/// campaign time and extended without recompiling the runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosScenario {
    /// Scenario identifier (e.g. "split_brain_prevention").
    pub id: String,
    /// Short description of what this scenario exercises.
    pub description: String,
    /// Cluster topology required (single-cluster vs multi-cluster).
    pub topology: Topology,
    /// Ordered list of actions to execute.
    pub actions: Vec<ChaosAction>,
    /// Invariants to check throughout and after execution.
    pub invariants: Vec<String>,
}

/// Cluster topology for a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Topology {
    /// Single cluster with N replicas.
    SingleCluster { replicas: u8 },
    /// Multiple clusters with M replicas each.
    MultiCluster { clusters: u8, replicas_per: u8 },
}

/// An action in a chaos scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChaosAction {
    /// Wait for a specified duration (milliseconds).
    Wait { ms: u64 },
    /// Start the workload generator (HTTP client hitting the cluster).
    StartWorkload { ops_per_sec: u64 },
    /// Stop the workload generator.
    StopWorkload,
    /// Kill a specific replica hard.
    KillReplica { cluster: u16, replica: u8 },
    /// Restart a previously killed replica.
    RestartReplica { cluster: u16, replica: u8 },
    /// Partition network between two replicas.
    Partition {
        from_cluster: u16,
        from_replica: u8,
        to_cluster: u16,
        to_replica: u8,
    },
    /// Heal a previously created partition (by rule ID).
    Heal { rule_id: u64 },
    /// Inject network delay on a bridge.
    AddNetem {
        bridge: String,
        delay_ms: u32,
        loss_percent: f32,
    },
    /// Corrupt disk sector on a specific replica.
    CorruptDisk {
        cluster: u16,
        replica: u8,
        offset: u64,
        length: u64,
    },
    /// Skew clock on a specific replica (milliseconds).
    SkewClock {
        cluster: u16,
        replica: u8,
        skew_ms: i64,
    },
    /// Fill disk to a percentage.
    FillDisk {
        cluster: u16,
        replica: u8,
        percent: u8,
    },
    /// Check an invariant at this point in the scenario.
    CheckInvariant { name: String },
}

// ============================================================================
// Scenario Catalog
// ============================================================================

/// The built-in chaos scenario catalog.
#[derive(Debug, Clone, Default)]
pub struct ScenarioCatalog {
    scenarios: Vec<ChaosScenario>,
}

impl ScenarioCatalog {
    /// Returns the default built-in catalog (6 scenarios).
    #[must_use]
    pub fn builtin() -> Self {
        let mut catalog = Self::default();
        catalog.add(split_brain_prevention());
        catalog.add(rolling_restart_under_load());
        catalog.add(leader_kill_mid_commit());
        catalog.add(cross_cluster_failover());
        catalog.add(cascading_failure());
        catalog.add(storage_exhaustion());
        catalog
    }

    pub fn add(&mut self, scenario: ChaosScenario) {
        self.scenarios.push(scenario);
    }

    #[must_use]
    pub fn list(&self) -> &[ChaosScenario] {
        &self.scenarios
    }

    #[must_use]
    pub fn find(&self, id: &str) -> Option<&ChaosScenario> {
        self.scenarios.iter().find(|s| s.id == id)
    }
}

// ============================================================================
// Built-in Scenarios
// ============================================================================

fn split_brain_prevention() -> ChaosScenario {
    ChaosScenario {
        id: "split_brain_prevention".into(),
        description: "Partition 3-node cluster as [2, 1]. Minority must refuse writes; \
                      merge must not produce divergence.".into(),
        topology: Topology::SingleCluster { replicas: 3 },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 100 },
            ChaosAction::Wait { ms: 2000 },
            // Partition: replica 2 isolated from replicas 0, 1.
            ChaosAction::Partition {
                from_cluster: 0,
                from_replica: 2,
                to_cluster: 0,
                to_replica: 0,
            },
            ChaosAction::Partition {
                from_cluster: 0,
                from_replica: 2,
                to_cluster: 0,
                to_replica: 1,
            },
            ChaosAction::Wait { ms: 10_000 },
            ChaosAction::CheckInvariant {
                name: "minority_refuses_writes".into(),
            },
            ChaosAction::Heal { rule_id: 0 },
            ChaosAction::Heal { rule_id: 1 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::StopWorkload,
            ChaosAction::CheckInvariant {
                name: "no_divergence_after_heal".into(),
            },
        ],
        invariants: vec![
            "minority_refuses_writes".into(),
            "no_divergence_after_heal".into(),
            "hash_chain_valid_all_replicas".into(),
        ],
    }
}

fn rolling_restart_under_load() -> ChaosScenario {
    ChaosScenario {
        id: "rolling_restart_under_load".into(),
        description: "Restart each replica sequentially while client drives workload. \
                      All writes must be preserved and linearizable."
            .into(),
        topology: Topology::SingleCluster { replicas: 3 },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 100 },
            ChaosAction::Wait { ms: 2000 },
            ChaosAction::KillReplica { cluster: 0, replica: 0 },
            ChaosAction::Wait { ms: 3000 },
            ChaosAction::RestartReplica { cluster: 0, replica: 0 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::KillReplica { cluster: 0, replica: 1 },
            ChaosAction::Wait { ms: 3000 },
            ChaosAction::RestartReplica { cluster: 0, replica: 1 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::KillReplica { cluster: 0, replica: 2 },
            ChaosAction::Wait { ms: 3000 },
            ChaosAction::RestartReplica { cluster: 0, replica: 2 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::StopWorkload,
            ChaosAction::CheckInvariant {
                name: "all_writes_preserved".into(),
            },
        ],
        invariants: vec![
            "all_writes_preserved".into(),
            "linearizability".into(),
        ],
    }
}

fn leader_kill_mid_commit() -> ChaosScenario {
    ChaosScenario {
        id: "leader_kill_mid_commit".into(),
        description: "Kill leader between Prepare and Commit. New leader must complete \
                      the commit (not re-propose). Client sees exactly-once."
            .into(),
        topology: Topology::SingleCluster { replicas: 3 },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 50 },
            ChaosAction::Wait { ms: 1000 },
            // TODO: precise mid-commit timing requires coordination with workload.
            ChaosAction::KillReplica { cluster: 0, replica: 0 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::RestartReplica { cluster: 0, replica: 0 },
            ChaosAction::Wait { ms: 3000 },
            ChaosAction::StopWorkload,
            ChaosAction::CheckInvariant {
                name: "exactly_once_semantics".into(),
            },
        ],
        invariants: vec![
            "exactly_once_semantics".into(),
            "no_lost_commits".into(),
        ],
    }
}

fn cross_cluster_failover() -> ChaosScenario {
    ChaosScenario {
        id: "cross_cluster_failover".into(),
        description: "Kill all replicas in cluster A. kimberlite-directory must reroute \
                      tenants to cluster B within SLA without data loss."
            .into(),
        topology: Topology::MultiCluster {
            clusters: 2,
            replicas_per: 3,
        },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 50 },
            ChaosAction::Wait { ms: 2000 },
            ChaosAction::KillReplica { cluster: 0, replica: 0 },
            ChaosAction::KillReplica { cluster: 0, replica: 1 },
            ChaosAction::KillReplica { cluster: 0, replica: 2 },
            ChaosAction::Wait { ms: 10_000 },
            ChaosAction::CheckInvariant {
                name: "directory_reroutes_to_cluster_b".into(),
            },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::StopWorkload,
            ChaosAction::CheckInvariant {
                name: "no_data_loss_across_failover".into(),
            },
        ],
        invariants: vec![
            "directory_reroutes_to_cluster_b".into(),
            "no_data_loss_across_failover".into(),
        ],
    }
}

fn cascading_failure() -> ChaosScenario {
    ChaosScenario {
        id: "cascading_failure".into(),
        description: "Kill replica 0. Before it recovers, kill replica 1 (f+1 failures). \
                      Cluster must detect quorum loss and refuse writes, not corrupt."
            .into(),
        topology: Topology::SingleCluster { replicas: 3 },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 100 },
            ChaosAction::Wait { ms: 2000 },
            ChaosAction::KillReplica { cluster: 0, replica: 0 },
            ChaosAction::Wait { ms: 500 },
            // Quickly kill replica 1 before replica 0 recovers.
            ChaosAction::KillReplica { cluster: 0, replica: 1 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::CheckInvariant {
                name: "quorum_loss_detected".into(),
            },
            ChaosAction::CheckInvariant {
                name: "no_corruption_under_quorum_loss".into(),
            },
            ChaosAction::RestartReplica { cluster: 0, replica: 0 },
            ChaosAction::RestartReplica { cluster: 0, replica: 1 },
            ChaosAction::Wait { ms: 5000 },
            ChaosAction::StopWorkload,
        ],
        invariants: vec![
            "quorum_loss_detected".into(),
            "no_corruption_under_quorum_loss".into(),
        ],
    }
}

fn storage_exhaustion() -> ChaosScenario {
    ChaosScenario {
        id: "storage_exhaustion".into(),
        description: "Fill one replica's disk to 95%. Storage limit must be enforced \
                      gracefully without panics or data corruption."
            .into(),
        topology: Topology::SingleCluster { replicas: 3 },
        actions: vec![
            ChaosAction::StartWorkload { ops_per_sec: 200 },
            ChaosAction::Wait { ms: 2000 },
            ChaosAction::FillDisk {
                cluster: 0,
                replica: 0,
                percent: 95,
            },
            ChaosAction::Wait { ms: 10_000 },
            ChaosAction::CheckInvariant {
                name: "graceful_enforcement".into(),
            },
            ChaosAction::StopWorkload,
            ChaosAction::CheckInvariant {
                name: "no_panic_or_corruption".into(),
            },
        ],
        invariants: vec![
            "graceful_enforcement".into(),
            "no_panic_or_corruption".into(),
        ],
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_has_six_scenarios() {
        let catalog = ScenarioCatalog::builtin();
        assert_eq!(catalog.list().len(), 6);
    }

    #[test]
    fn builtin_scenarios_have_actions() {
        let catalog = ScenarioCatalog::builtin();
        for scenario in catalog.list() {
            assert!(!scenario.actions.is_empty(), "{} has no actions", scenario.id);
            assert!(
                !scenario.invariants.is_empty(),
                "{} declares no invariants",
                scenario.id
            );
        }
    }

    #[test]
    fn find_by_id() {
        let catalog = ScenarioCatalog::builtin();
        assert!(catalog.find("split_brain_prevention").is_some());
        assert!(catalog.find("nonexistent").is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let catalog = ScenarioCatalog::builtin();
        let scenario = &catalog.list()[0];
        let json = serde_json::to_string(scenario).unwrap();
        let parsed: ChaosScenario = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, scenario.id);
        assert_eq!(parsed.actions.len(), scenario.actions.len());
    }
}
