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
/// Addresses a specific replica by its (cluster, replica-within-cluster) pair.
pub type ReplicaKey = (u16, u8);

/// Key for [`ChaosController::partition_rules`] — maps a `(from, to)` replica
/// pair to the iptables rule ID that drops that flow.
pub type PartitionKey = (ReplicaKey, ReplicaKey);

pub struct ChaosController {
    vms: HashMap<ReplicaKey, ClusterVm>,
    network: NetworkController,
    invariants: InvariantChecker,
    /// Partition rule IDs indexed by (from, to) tuple for heal operations.
    partition_rules: HashMap<PartitionKey, u64>,
    /// VM specs provisioned at setup; used by RestartReplica.
    vm_specs: HashMap<ReplicaKey, VmSpec>,
    /// Start time for reporting.
    start_time: Option<Instant>,
    /// Optional directory for per-run artifacts (console logs, report.json).
    output_dir: Option<std::path::PathBuf>,
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
            output_dir: None,
        }
    }

    /// Configures a per-run output directory. Per-VM serial console logs
    /// are written to `<dir>/console-c{cluster}-r{replica}.log`; the final
    /// report.json is written by the binary once the run ends.
    pub fn set_output_dir(&mut self, dir: impl Into<std::path::PathBuf>) {
        let path = dir.into();
        let _ = std::fs::create_dir_all(&path);
        self.output_dir = Some(path);
    }

    /// Creates a controller that will execute real host commands.
    ///
    /// Requires root (or CAP_NET_ADMIN) and the host-side tools. Use with
    /// care on shared systems.
    #[must_use]
    pub fn with_apply() -> Self {
        let mut invariants = InvariantChecker::builtin();
        // Only probe real HTTP endpoints when actually running against
        // live VMs. Probes are short-circuited in DryRun.
        invariants.set_probes_enabled(true);
        Self {
            vms: HashMap::new(),
            network: NetworkController::with_apply(),
            invariants,
            partition_rules: HashMap::new(),
            vm_specs: HashMap::new(),
            start_time: None,
            output_dir: None,
        }
    }

    /// Provisions the topology required by a scenario.
    ///
    /// 1. Creates the Linux bridge(s) for each cluster.
    /// 2. For each replica in each cluster, records a [`VmSpec`] whose
    ///    kernel cmdline bakes in its replica identity, bind address, and
    ///    peer list, and creates a tap device attached to the cluster's
    ///    bridge.
    ///
    /// VMs are constructed but not booted here — boot is a separate step
    /// driven by the scenario's `ChaosAction::RestartReplica` events or by
    /// an explicit `boot_all()` call (future work).
    pub fn provision(&mut self, scenario: &ChaosScenario) -> Result<(), ChaosError> {
        match scenario.topology {
            Topology::SingleCluster { replicas } => {
                self.provision_cluster(0, replicas)?;
            }
            Topology::MultiCluster {
                clusters,
                replicas_per,
            } => {
                for c in 0..clusters {
                    self.provision_cluster(u16::from(c), replicas_per)?;
                }
            }
        }
        Ok(())
    }

    fn provision_cluster(&mut self, cluster: u16, replicas: u8) -> Result<(), ChaosError> {
        let bridge_name = format!("kmb-c{cluster}-br");
        self.network.create_bridge(BridgeConfig {
            name: bridge_name.clone(),
            subnet: format!("10.42.{cluster}.0/24"),
        })?;
        for r in 0..replicas {
            let tap_name = format!("tap-c{cluster}-r{r}");
            self.network.create_tap(&tap_name, &bridge_name)?;
            self.record_vm_spec(cluster, r, replicas);
        }
        Ok(())
    }

    fn record_vm_spec(&mut self, cluster: u16, replica: u8, replicas_in_cluster: u8) {
        let ip = replica_ip(cluster, replica);
        let bind = format!("{ip}:9000");
        let peers: Vec<String> = (0..replicas_in_cluster)
            .map(|r| format!("{}:9000", replica_ip(cluster, r)))
            .collect();
        let peers_csv = peers.join(",");
        let gateway = gateway_ip(cluster);
        // Kernel built-in ip= format:
        //   client-ip::gateway:netmask:hostname:device:autoconf
        let ip_param = format!("{ip}::{gateway}:255.255.255.0::eth0:off");
        let kernel_cmdline = format!(
            "console=ttyS0 root=/dev/vda1 rw nokaslr \
             ip={ip_param} \
             kmb.replica_id={replica} kmb.bind={bind} kmb.peers={peers_csv}"
        );

        let mut spec = VmSpec::new(
            cluster,
            replica,
            std::path::PathBuf::from(format!(
                "/opt/kimberlite-dst/vm-images/replica-c{cluster}-r{replica}.qcow2"
            )),
            std::path::PathBuf::from("/opt/kimberlite-dst/vm-images/bzImage"),
        );
        spec.kernel_cmdline = kernel_cmdline;
        if let Some(ref dir) = self.output_dir {
            spec.console_log = Some(dir.join(format!("console-c{cluster}-r{replica}.log")));
        }
        self.vm_specs.insert((cluster, replica), spec.clone());
        let vm = ClusterVm::new(spec);
        self.vms.insert((cluster, replica), vm);
    }

    /// Returns the host-side URL to reach a replica's health endpoint.
    /// Used by invariant probes.
    #[must_use]
    pub fn replica_endpoint(&self, cluster: u16, replica: u8) -> Option<String> {
        self.vm_specs
            .get(&(cluster, replica))
            .map(|_| format!("http://{}:9000", replica_ip(cluster, replica)))
    }

    /// Executes a chaos scenario end-to-end.
    pub fn run(&mut self, scenario: &ChaosScenario) -> Result<ChaosReport, ChaosError> {
        self.start_time = Some(Instant::now());
        self.provision(scenario)?;

        // Register every provisioned replica's endpoint with the invariant
        // checker so the HTTP probes in Phase 2.4 can reach them.
        for &(cluster, replica) in self.vm_specs.keys() {
            if let Some(url) = self.replica_endpoint(cluster, replica) {
                self.invariants.set_endpoint(cluster, replica, url);
            }
        }

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
                // The source of the partition rule is being cut off from the
                // destination — mark the source as minority so the HTTP probe
                // expects it to refuse writes.
                self.invariants.mark_minority(*from_cluster, *from_replica);
                Ok(())
            }
            ChaosAction::Heal { rule_id } => {
                self.network.heal(*rule_id)?;
                // Find the source replica of this rule, clear its minority
                // status, then drop the rule from our tracking map.
                let healed_sources: Vec<(u16, u8)> = self
                    .partition_rules
                    .iter()
                    .filter(|(_, id)| **id == *rule_id)
                    .map(|((from, _to), _)| *from)
                    .collect();
                for (from_c, from_r) in healed_sources {
                    self.invariants.clear_minority(from_c, from_r);
                }
                self.partition_rules.retain(|_, id| *id != *rule_id);
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

/// Returns the IP address assigned to replica `replica` within cluster
/// `cluster`. Convention: `10.42.{cluster}.{10 + replica}`.
#[must_use]
fn replica_ip(cluster: u16, replica: u8) -> String {
    format!("10.42.{}.{}", cluster, 10 + u16::from(replica))
}

/// Returns the gateway IP for cluster `cluster` (the bridge itself).
/// Convention: `10.42.{cluster}.1`.
#[must_use]
fn gateway_ip(cluster: u16) -> String {
    format!("10.42.{cluster}.1")
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
            description: String::new(),
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
            description: String::new(),
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

    #[test]
    fn replica_ip_convention() {
        assert_eq!(replica_ip(0, 0), "10.42.0.10");
        assert_eq!(replica_ip(0, 2), "10.42.0.12");
        assert_eq!(replica_ip(5, 1), "10.42.5.11");
        assert_eq!(gateway_ip(0), "10.42.0.1");
        assert_eq!(gateway_ip(3), "10.42.3.1");
    }

    #[test]
    fn provision_bakes_identity_into_kernel_cmdline() {
        let mut controller = ChaosController::new();
        let scenario = ChaosScenario {
            id: "t".into(),
            description: String::new(),
            topology: Topology::SingleCluster { replicas: 3 },
            actions: vec![],
            invariants: vec![],
        };
        controller.provision(&scenario).unwrap();
        let spec = controller.vm_specs.get(&(0, 1)).unwrap();
        // replica_id= is present and matches the VmSpec's replica_id.
        assert!(spec.kernel_cmdline.contains("kmb.replica_id=1"));
        assert!(spec.kernel_cmdline.contains("kmb.bind=10.42.0.11:9000"));
        // peers covers all three replicas.
        assert!(spec.kernel_cmdline.contains(
            "kmb.peers=10.42.0.10:9000,10.42.0.11:9000,10.42.0.12:9000"
        ));
        // kernel ip= format: client::gateway:netmask:host:dev:autoconf
        assert!(
            spec.kernel_cmdline
                .contains("ip=10.42.0.11::10.42.0.1:255.255.255.0::eth0:off")
        );
    }

    #[test]
    fn replica_endpoint_points_at_cluster_subnet() {
        let mut controller = ChaosController::new();
        let scenario = ChaosScenario {
            id: "t".into(),
            description: String::new(),
            topology: Topology::SingleCluster { replicas: 3 },
            actions: vec![],
            invariants: vec![],
        };
        controller.provision(&scenario).unwrap();
        assert_eq!(
            controller.replica_endpoint(0, 0).as_deref(),
            Some("http://10.42.0.10:9000")
        );
        assert_eq!(controller.replica_endpoint(0, 2).as_deref(), Some("http://10.42.0.12:9000"));
        assert!(controller.replica_endpoint(9, 0).is_none());
    }
}
