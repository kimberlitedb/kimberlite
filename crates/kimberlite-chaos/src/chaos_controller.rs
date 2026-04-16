//! Top-level chaos scenario orchestrator.
//!
//! Takes a `ChaosScenario`, provisions the cluster topology, executes each
//! `ChaosAction` in order, checks invariants, and emits a `ChaosReport`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cluster_network::{BridgeConfig, NetworkController, NetworkError};
use crate::cluster_vm::{ClusterVm, VmError, VmSpec, VmState};
use crate::invariant_checker::{InvariantChecker, InvariantResult};
use crate::scenarios::{ChaosAction, ChaosScenario, Topology};

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum ChaosError {
    #[error("VM error: {0}")]
    Vm(#[from] VmError),
    #[error("network error: {0}")]
    Network(#[from] NetworkError),
    #[error("scenario error: {0}")]
    Scenario(String),
    #[error("invariant violated: {name} — {message}")]
    InvariantViolated { name: String, message: String },
}

// ============================================================================
// Chaos Report
// ============================================================================

/// Report produced after running a chaos scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosReport {
    pub scenario_id: String,
    pub duration_ms: u64,
    pub actions_executed: u32,
    pub invariant_results: Vec<InvariantResult>,
    pub vm_states_final: HashMap<String, String>,
    pub success: bool,
    pub error: Option<String>,
}

// ============================================================================
// Chaos Controller
// ============================================================================

/// Orchestrates a chaos scenario across a provisioned cluster.
pub struct ChaosController {
    vms: HashMap<(u16, u8), ClusterVm>,
    network: NetworkController,
    invariants: InvariantChecker,
    /// Partition rule IDs indexed by (from, to) tuple for heal operations.
    partition_rules: HashMap<((u16, u8), (u16, u8)), u64>,
    /// VM specs provisioned at setup; used by RestartReplica.
    vm_specs: HashMap<(u16, u8), VmSpec>,
    /// Start time for reporting.
    start_time: Option<Instant>,
}

impl ChaosController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            vms: HashMap::new(),
            network: NetworkController::new(),
            invariants: InvariantChecker::builtin(),
            partition_rules: HashMap::new(),
            vm_specs: HashMap::new(),
            start_time: None,
        }
    }

    /// Creates a controller that will execute real host commands.
    ///
    /// Requires root (or CAP_NET_ADMIN) and the host-side tools. Use with
    /// care on shared systems.
    #[must_use]
    pub fn with_apply() -> Self {
        Self {
            vms: HashMap::new(),
            network: NetworkController::with_apply(),
            invariants: InvariantChecker::builtin(),
            partition_rules: HashMap::new(),
            vm_specs: HashMap::new(),
            start_time: None,
        }
    }

    /// Provisions the topology required by a scenario.
    ///
    /// Note: this is a skeleton. Real provisioning will:
    /// 1. Create Linux bridges for each cluster.
    /// 2. Create tap devices for each VM and attach to bridges.
    /// 3. Clone base disk images for each replica.
    /// 4. Boot each VM and wait for readiness.
    pub fn provision(&mut self, scenario: &ChaosScenario) -> Result<(), ChaosError> {
        match scenario.topology {
            Topology::SingleCluster { replicas } => {
                self.network.create_bridge(BridgeConfig {
                    name: "kmb-c0-br".into(),
                    subnet: "10.42.0.0/24".into(),
                })?;
                for r in 0..replicas {
                    self.record_vm_spec(0, r)?;
                }
            }
            Topology::MultiCluster {
                clusters,
                replicas_per,
            } => {
                for c in 0..clusters {
                    self.network.create_bridge(BridgeConfig {
                        name: format!("kmb-c{c}-br"),
                        subnet: format!("10.42.{c}.0/24"),
                    })?;
                    for r in 0..replicas_per {
                        self.record_vm_spec(c as u16, r)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn record_vm_spec(&mut self, cluster: u16, replica: u8) -> Result<(), ChaosError> {
        let spec = VmSpec::new(
            cluster,
            replica,
            // TODO: derive from per-replica disk image path (cloned at provision time).
            std::path::PathBuf::from(format!(
                "/opt/kimberlite-dst/vm-images/replica-c{cluster}-r{replica}.qcow2"
            )),
            std::path::PathBuf::from("/opt/kimberlite-dst/vm-images/bzImage"),
        );
        self.vm_specs.insert((cluster, replica), spec.clone());
        let vm = ClusterVm::new(spec);
        self.vms.insert((cluster, replica), vm);
        Ok(())
    }

    /// Executes a chaos scenario end-to-end.
    pub fn run(&mut self, scenario: &ChaosScenario) -> Result<ChaosReport, ChaosError> {
        self.start_time = Some(Instant::now());
        self.provision(scenario)?;

        let mut actions_executed = 0u32;
        let mut error: Option<String> = None;

        for action in &scenario.actions {
            actions_executed += 1;
            if let Err(e) = self.execute_action(action) {
                error = Some(e.to_string());
                break;
            }
        }

        let duration = self.start_time.map_or(Duration::ZERO, |t| t.elapsed());
        let vm_states_final = self
            .vms
            .iter()
            .map(|((c, r), vm)| (format!("c{c}-r{r}"), format!("{:?}", vm.state())))
            .collect();

        // Final invariant checks.
        let now_ms = duration.as_millis() as u64;
        for inv_name in &scenario.invariants {
            self.invariants.check(inv_name, now_ms);
        }

        let invariant_results = self.invariants.results().to_vec();
        let success = error.is_none() && invariant_results.iter().all(|r| r.held);

        Ok(ChaosReport {
            scenario_id: scenario.id.clone(),
            duration_ms: duration.as_millis() as u64,
            actions_executed,
            invariant_results,
            vm_states_final,
            success,
            error,
        })
    }

    fn execute_action(&mut self, action: &ChaosAction) -> Result<(), ChaosError> {
        let elapsed_ms = self.start_time.map_or(0, |t| t.elapsed().as_millis() as u64);
        match action {
            ChaosAction::Wait { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            ChaosAction::StartWorkload { ops_per_sec } => {
                tracing::info!(ops_per_sec, "start workload (stub)");
                // TODO: spawn HTTP client process
                Ok(())
            }
            ChaosAction::StopWorkload => {
                tracing::info!("stop workload (stub)");
                Ok(())
            }
            ChaosAction::KillReplica { cluster, replica } => {
                if let Some(vm) = self.vms.get_mut(&(*cluster, *replica)) {
                    vm.kill_hard()?;
                }
                Ok(())
            }
            ChaosAction::RestartReplica { cluster, replica } => {
                if let Some(vm) = self.vms.get_mut(&(*cluster, *replica)) {
                    if vm.state() == VmState::Crashed || vm.state() == VmState::Stopped {
                        vm.boot()?;
                    }
                }
                Ok(())
            }
            ChaosAction::Partition {
                from_cluster,
                from_replica,
                to_cluster,
                to_replica,
            } => {
                let from = format!("c{from_cluster}-r{from_replica}");
                let to = format!("c{to_cluster}-r{to_replica}");
                let rule_id = self.network.partition(&from, &to)?;
                self.partition_rules
                    .insert(((*from_cluster, *from_replica), (*to_cluster, *to_replica)), rule_id);
                Ok(())
            }
            ChaosAction::Heal { rule_id } => {
                self.network.heal(*rule_id)?;
                Ok(())
            }
            ChaosAction::AddNetem {
                bridge,
                delay_ms,
                loss_percent,
            } => {
                self.network.add_netem(bridge, *delay_ms, *loss_percent)?;
                Ok(())
            }
            ChaosAction::CorruptDisk { .. } => {
                // TODO: dd a bit flip into the VM's disk image (while VM is stopped,
                // or via QEMU monitor's block device commands).
                tracing::warn!(?action, "CorruptDisk: not yet implemented");
                Ok(())
            }
            ChaosAction::SkewClock { .. } => {
                // TODO: QEMU monitor's rtc adjustment.
                tracing::warn!(?action, "SkewClock: not yet implemented");
                Ok(())
            }
            ChaosAction::FillDisk { .. } => {
                // TODO: ssh into VM and dd /dev/zero of=filler, or pre-size the qcow2.
                tracing::warn!(?action, "FillDisk: not yet implemented");
                Ok(())
            }
            ChaosAction::CheckInvariant { name } => {
                let result = self.invariants.check(name, elapsed_ms);
                if !result.held {
                    return Err(ChaosError::InvariantViolated {
                        name: name.clone(),
                        message: result.message,
                    });
                }
                Ok(())
            }
        }
    }

    /// Shuts down all VMs and cleans up network state.
    pub fn teardown(&mut self) -> Result<(), ChaosError> {
        for vm in self.vms.values_mut() {
            let _ = vm.shutdown_graceful();
        }
        self.vms.clear();
        Ok(())
    }
}

impl Default for ChaosController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ChaosController {
    fn drop(&mut self) {
        let _ = self.teardown();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::ScenarioCatalog;

    #[test]
    fn controller_provisions_single_cluster_topology() {
        let mut controller = ChaosController::new();
        let scenario = ChaosScenario {
            id: "test".into(),
            description: "".into(),
            topology: Topology::SingleCluster { replicas: 3 },
            actions: vec![],
            invariants: vec![],
        };
        controller.provision(&scenario).unwrap();
        assert_eq!(controller.vms.len(), 3);
        assert_eq!(controller.network.bridges().len(), 1);
    }

    #[test]
    fn controller_provisions_multi_cluster_topology() {
        let mut controller = ChaosController::new();
        let scenario = ChaosScenario {
            id: "test".into(),
            description: "".into(),
            topology: Topology::MultiCluster {
                clusters: 2,
                replicas_per: 3,
            },
            actions: vec![],
            invariants: vec![],
        };
        controller.provision(&scenario).unwrap();
        assert_eq!(controller.vms.len(), 6);
        assert_eq!(controller.network.bridges().len(), 2);
    }

    #[test]
    fn controller_executes_scenario_with_waits_only() {
        let mut controller = ChaosController::new();
        let catalog = ScenarioCatalog::builtin();
        let scenario = catalog.find("split_brain_prevention").unwrap().clone();
        // Replace actions with just waits so the test doesn't try real VMs.
        let trimmed = ChaosScenario {
            actions: vec![ChaosAction::Wait { ms: 1 }],
            ..scenario
        };
        let report = controller.run(&trimmed).unwrap();
        assert_eq!(report.actions_executed, 1);
        // Default topology provisioned 3 VMs.
        assert_eq!(report.vm_states_final.len(), 3);
    }
}
