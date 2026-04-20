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
    /// Handle to the background workload generator thread (if StartWorkload
    /// is active). Cleared by StopWorkload or at teardown.
    workload: Option<WorkloadHandle>,
}

/// Background workload: spams POST /kv/chaos-probe across all known
/// replica endpoints at a target ops/sec. Exits when the signalled
/// AtomicBool flips to true.
///
/// Each probe includes a unique `write_id` in the JSON body. Probes that
/// receive a 200 OK are recorded in `acknowledged` so that post-scenario
/// invariant checkers can verify durability.
struct WorkloadHandle {
    thread: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Write IDs that received a 200 OK from any replica.
    acknowledged: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
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
            workload: None,
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
            workload: None,
        }
    }

    /// Provisions the topology required by a scenario.
    ///
    /// 1. Creates the Linux bridge(s) for each cluster.
    /// 2. For each replica in each cluster, records a [`VmSpec`] whose
    ///    kernel cmdline bakes in its replica identity, bind addresses,
    ///    and the SAME-CLUSTER peer list, and creates a tap device
    ///    attached to the cluster's bridge.
    ///
    /// Peer lists are per-cluster: real VSR clusters are independent state
    /// machines, and feeding a replica a cross-cluster peer list would
    /// violate the consensus invariants. (The legacy shim intentionally
    /// gossiped cross-cluster as a simplification; the real binary cannot.)
    /// `cross_cluster_failover` therefore tests *two independent clusters*,
    /// which is the honest semantics.
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

        // Per-cluster peer list in VSR's required `id=ip:port` format.
        // Port 5433 is VSR peer transport; 5432 is the client-facing binary
        // protocol listener on the same host (they can't share a port).
        // HTTP sidecar lives on 9000.
        let cluster_peers: Vec<String> = (0..replicas)
            .map(|r| format!("{}={}:5433", r, replica_ip(cluster, r)))
            .collect();
        let cluster_peers_csv = cluster_peers.join(",");

        for r in 0..replicas {
            let tap_name = format!("tap-c{cluster}-r{r}");
            self.network.create_tap(&tap_name, &bridge_name)?;
            self.record_vm_spec(cluster, r, &cluster_peers_csv);
        }
        Ok(())
    }

    fn record_vm_spec(&mut self, cluster: u16, replica: u8, cluster_peers_csv: &str) {
        let ip = replica_ip(cluster, replica);
        // Binary protocol binds on 5432; chaos HTTP probe surface on 9000.
        // Binding 0.0.0.0 sidesteps the race where /sbin/init assigns the
        // IP after the server starts listening.
        let bind = "0.0.0.0:5432".to_string();
        let http_bind = "0.0.0.0:9000".to_string();
        let gateway = gateway_ip(cluster);
        // Ubuntu's kernel has CONFIG_IP_PNP=n, so we roll our own kmb.ip=
        // and kmb.gw= parameters which /sbin/init parses and plumbs into
        // `ip addr add` + `ip route add default`.
        let own_addr = format!("{ip}:5432");
        let kernel_cmdline = format!(
            "console=ttyS0 root=/dev/vda rw nokaslr panic=5 \
             kmb.replica_id={replica} kmb.bind={bind} kmb.http_bind={http_bind} \
             kmb.own={own_addr} kmb.cluster_peers={cluster_peers_csv} \
             kmb.ip={ip}/24 kmb.gw={gateway}"
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

        // In Apply mode, boot every provisioned VM and wait briefly for the
        // guest to become reachable before we start executing actions. In
        // DryRun, VMs are never started — actions log what they would do.
        if self.network.mode() == crate::cluster_network::ExecMode::Apply {
            let mut boot_errors = Vec::new();
            for ((c, r), vm) in &mut self.vms {
                tracing::info!(cluster = %c, replica = %r, "booting VM");
                if let Err(e) = vm.boot() {
                    boot_errors.push(format!("c{c}-r{r}: {e}"));
                }
            }
            if !boot_errors.is_empty() {
                return Err(ChaosError::Scenario(format!(
                    "failed to boot one or more VMs: {}",
                    boot_errors.join("; ")
                )));
            }
            // Poll each replica's /health endpoint until it responds with
            // 200, with a 60-second budget per VM. Replaces the old 15-second
            // blanket sleep — most VMs come up in 2–5s, so this is faster in
            // the common case and more reliable in the slow case (high host
            // load, cold page cache). Readiness failures are surfaced as a
            // scenario error so the run fails fast instead of silently
            // proceeding against unbooted VMs.
            let ready_timeout = Duration::from_secs(60);
            let poll_interval = Duration::from_millis(250);
            let mut ready_errors = Vec::new();
            for ((c, r), vm) in &self.vms {
                let Some(url) = self.replica_endpoint(*c, *r) else {
                    continue;
                };
                if let Err(e) = vm.wait_for_http_ready(&url, ready_timeout, poll_interval) {
                    ready_errors.push(format!("c{c}-r{r}: {e}"));
                }
            }
            if !ready_errors.is_empty() {
                return Err(ChaosError::Scenario(format!(
                    "VMs failed to become HTTP-ready: {}",
                    ready_errors.join("; ")
                )));
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

        // Final invariant checks (before shutdown — some probes need the
        // VMs still reachable).
        let now_ms = duration.as_millis() as u64;
        for inv_name in &scenario.invariants {
            self.invariants.check(inv_name, now_ms);
        }

        // Shut down any booted VMs. Graceful first (5s budget), then kill.
        if self.network.mode() == crate::cluster_network::ExecMode::Apply {
            for vm in self.vms.values_mut() {
                if vm.state() == VmState::Running {
                    if let Err(e) = vm.shutdown_graceful() {
                        tracing::warn!(err = %e, "graceful shutdown failed; continuing");
                    }
                }
            }
        }

        let vm_states_final = self
            .vms
            .iter()
            .map(|((c, r), vm)| (format!("c{c}-r{r}"), format!("{:?}", vm.state())))
            .collect();

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
        let elapsed_ms = self
            .start_time
            .map_or(0, |t| t.elapsed().as_millis() as u64);
        match action {
            ChaosAction::Wait { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            ChaosAction::StartWorkload { ops_per_sec } => {
                // Background thread posts /kv/chaos-probe across every
                // endpoint at the target rate. We only bother when probes
                // are enabled (Apply mode) — DryRun just logs.
                if self.network.mode() != crate::cluster_network::ExecMode::Apply {
                    tracing::info!(ops_per_sec, "StartWorkload (dry-run)");
                    return Ok(());
                }
                if self.workload.is_some() {
                    tracing::info!("StartWorkload: already running");
                    return Ok(());
                }
                let endpoints: Vec<String> = self
                    .vm_specs
                    .keys()
                    .filter_map(|(c, r)| self.replica_endpoint(*c, *r))
                    .collect();
                if endpoints.is_empty() {
                    return Ok(());
                }
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let stop_clone = stop.clone();
                let acknowledged = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
                let acked_clone = acknowledged.clone();
                let rate = *ops_per_sec;
                let sleep = std::time::Duration::from_millis(if rate == 0 {
                    100
                } else {
                    (1000 / rate).max(1)
                });
                let thread = std::thread::spawn(move || {
                    // HTTP client timeout must exceed the server's
                    // CHAOS_PROBE_TIMEOUT (5s) so in-flight commits through
                    // VSR have time to return a real 200/503/421 instead
                    // of being cut off as transport errors. Too-short
                    // timeouts here silently empty the acknowledged list
                    // and make downstream durability checks trivially
                    // pass/fail against the liveness-proxy fallback.
                    let agent = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(8))
                        .build();
                    let mut cursor = 0u64;
                    while !stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        let ep = &endpoints[cursor as usize % endpoints.len()];
                        let write_id = cursor.to_string();
                        cursor = cursor.wrapping_add(1);
                        let url = format!("{}/kv/chaos-probe", ep.trim_end_matches('/'));
                        let body = format!("{{\"op\":\"workload\",\"write_id\":\"{write_id}\"}}");
                        // ureq 2.x returns Err(Status) for non-2xx responses —
                        // we need to distinguish 200 from non-2xx (which is
                        // also "the server responded, just rejected").
                        // Only truly transient transport errors stop us from
                        // recording a proper ack decision.
                        let status = match agent
                            .post(&url)
                            .set("content-type", "application/json")
                            .send_string(&body)
                        {
                            Ok(resp) => resp.status(),
                            Err(ureq::Error::Status(code, _)) => code,
                            Err(_) => 0,
                        };
                        if status == 200 {
                            if let Ok(mut acked) = acked_clone.lock() {
                                acked.push(write_id);
                            }
                        }
                        std::thread::sleep(sleep);
                    }
                });
                self.workload = Some(WorkloadHandle {
                    thread: Some(thread),
                    stop,
                    acknowledged,
                });
                tracing::info!(ops_per_sec, "StartWorkload started");
                Ok(())
            }
            ChaosAction::StopWorkload => {
                if let Some(mut h) = self.workload.take() {
                    h.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    if let Some(thread) = h.thread.take() {
                        let _ = thread.join();
                    }
                    // Hand acknowledged write_ids to the invariant checker so
                    // post-scenario checks can verify each one is still present
                    // in at least one replica's write log.
                    if let Ok(acked) = h.acknowledged.lock() {
                        self.invariants.set_acknowledged_writes(acked.clone());
                        tracing::info!(
                            count = acked.len(),
                            "StopWorkload: {} acknowledged writes registered with checker",
                            acked.len()
                        );
                    }
                    tracing::info!("StopWorkload: joined");
                }
                Ok(())
            }
            ChaosAction::KillReplica { cluster, replica } => {
                if let Some(vm) = self.vms.get_mut(&(*cluster, *replica)) {
                    vm.kill_hard()?;
                }
                // Intentional kill: deregister so liveness probes skip it.
                self.invariants.remove_endpoint(*cluster, *replica);
                Ok(())
            }
            ChaosAction::RestartReplica { cluster, replica } => {
                if let Some(vm) = self.vms.get_mut(&(*cluster, *replica)) {
                    if vm.state() == VmState::Crashed || vm.state() == VmState::Stopped {
                        vm.boot()?;
                        if let Some(url) = self.replica_endpoint(*cluster, *replica) {
                            self.invariants.set_endpoint(*cluster, *replica, url);
                        }
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
                // iptables -s/-d want IPs, not hostnames. Use the same
                // replica_ip convention that we bake into kernel cmdline.
                let from = replica_ip(*from_cluster, *from_replica);
                let to = replica_ip(*to_cluster, *to_replica);
                let rule_id = self.network.partition(&from, &to)?;
                self.partition_rules.insert(
                    ((*from_cluster, *from_replica), (*to_cluster, *to_replica)),
                    rule_id,
                );
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
            ChaosAction::CorruptDisk {
                cluster,
                replica,
                offset,
                length,
            } => {
                // Write /dev/urandom into the replica's qcow2 backing file
                // while the VM is stopped. Real bit-rot simulation. Requires
                // the VM to be killed first (callers typically queue a
                // KillReplica just before CorruptDisk).
                if let Some(vm) = self.vms.get_mut(&(*cluster, *replica)) {
                    if vm.state() == VmState::Running {
                        let _ = vm.kill_hard();
                    }
                    let path = vm.spec().disk_image.clone();
                    if self.network.mode() == crate::cluster_network::ExecMode::Apply {
                        corrupt_disk_region(&path, *offset, *length)
                            .map_err(ChaosError::Scenario)?;
                    } else {
                        tracing::info!(?path, offset, length, "CorruptDisk (dry-run)");
                    }
                }
                Ok(())
            }
            ChaosAction::SkewClock {
                cluster,
                replica,
                skew_ms,
            } => {
                // Issue a QMP `rtc-reset-reinjection` + `qom-set rtc.date`
                // on the running VM. We only bother in Apply mode (QMP
                // socket exists only for live VMs).
                if self.network.mode() == crate::cluster_network::ExecMode::Apply {
                    if let Some(vm) = self.vms.get(&(*cluster, *replica)) {
                        if vm.state() == VmState::Running {
                            match crate::qmp::QmpClient::connect(&vm.spec().qmp_socket) {
                                Ok(mut client) => {
                                    let secs = skew_ms / 1000;
                                    let _ = client.send_command(
                                        "qom-set",
                                        Some(serde_json::json!({
                                            "path": "/machine/rtc",
                                            "property": "date-offset",
                                            "value": secs,
                                        })),
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(err = %e, "SkewClock: QMP connect failed");
                                }
                            }
                        }
                    }
                } else {
                    tracing::info!(cluster, replica, skew_ms, "SkewClock (dry-run)");
                }
                Ok(())
            }
            ChaosAction::FillDisk {
                cluster,
                replica,
                percent,
            } => {
                // Inflates host disk usage by fallocating a sibling file
                // next to the qcow2 backing file. Does NOT require the VM
                // to be stopped — fallocate is a pure host-side op and
                // the VM keeps running while storage pressure grows.
                if let Some(vm) = self.vms.get(&(*cluster, *replica)) {
                    let path = vm.spec().disk_image.clone();
                    if self.network.mode() == crate::cluster_network::ExecMode::Apply {
                        fill_disk_image(&path, *percent).map_err(ChaosError::Scenario)?;
                    } else {
                        tracing::info!(?path, percent, "FillDisk (dry-run)");
                    }
                }
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
            ChaosAction::WaitForConvergence {
                poll_ms,
                stable_for_ms,
                timeout_ms,
            } => {
                // Progress-based wait — polls /state/commit_watermark on
                // every reachable replica and returns when the value is
                // identical and unchanging for `stable_for_ms`. Unreachable
                // endpoints are tolerated. On timeout we return Ok: the
                // following CheckInvariant action will still surface any
                // real divergence, and timing out here vs. the fixed Wait
                // it replaces is not itself a scenario failure.
                self.invariants
                    .wait_for_convergence(*poll_ms, *stable_for_ms, *timeout_ms);
                Ok(())
            }
        }
    }

    /// Shuts down all VMs and cleans up network state.
    pub fn teardown(&mut self) -> Result<(), ChaosError> {
        if let Some(mut h) = self.workload.take() {
            h.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(t) = h.thread.take() {
                let _ = t.join();
            }
        }
        for vm in self.vms.values_mut() {
            let _ = vm.shutdown_graceful();
        }
        self.vms.clear();
        Ok(())
    }
}

/// Corrupts `length` bytes of the qcow2 file at `path` starting at
/// `offset` by writing /dev/urandom over the region.
fn corrupt_disk_region(path: &std::path::Path, offset: u64, length: u64) -> Result<(), String> {
    let status = std::process::Command::new("dd")
        .arg("if=/dev/urandom")
        .arg(format!("of={}", path.display()))
        .arg("bs=1")
        .arg(format!("seek={offset}"))
        .arg(format!("count={length}"))
        .arg("conv=notrunc")
        .output()
        .map_err(|e| format!("dd spawn failed: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "dd failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    Ok(())
}

/// Inflates disk usage by placing a sparse filler file next to the qcow2
/// sized to `percent%` of the image's virtual size. Simulates disk
/// exhaustion at the host filesystem level.
fn fill_disk_image(path: &std::path::Path, percent: u8) -> Result<(), String> {
    // --force-share lets us read metadata while the VM holds a write lock.
    let info = std::process::Command::new("qemu-img")
        .arg("info")
        .arg("--output=json")
        .arg("--force-share")
        .arg(path)
        .output()
        .map_err(|e| format!("qemu-img info failed: {e}"))?;
    if !info.status.success() {
        return Err(format!(
            "qemu-img info error: {}",
            String::from_utf8_lossy(&info.stderr)
        ));
    }
    let body: serde_json::Value = serde_json::from_slice(&info.stdout)
        .map_err(|e| format!("qemu-img info JSON parse: {e}"))?;
    let virtual_size = body
        .get("virtual-size")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "missing virtual-size in qemu-img info".to_string())?;
    let target = virtual_size.saturating_mul(u64::from(percent)) / 100;
    let filler = path.with_extension("fill");
    let status = std::process::Command::new("fallocate")
        .arg("-l")
        .arg(target.to_string())
        .arg(&filler)
        .output()
        .map_err(|e| format!("fallocate spawn failed: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "fallocate failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    Ok(())
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
        // Binary protocol on 5432; chaos HTTP probe surface on 9000.
        assert!(spec.kernel_cmdline.contains("kmb.bind=0.0.0.0:5432"));
        assert!(spec.kernel_cmdline.contains("kmb.http_bind=0.0.0.0:9000"));
        // Per-cluster peer list in VSR's `id=ip:port` format.
        // VSR peer transport uses 5433 (5432 is reserved for the client
        // binary protocol listener on the same host).
        assert!(
            spec.kernel_cmdline.contains(
                "kmb.cluster_peers=0=10.42.0.10:5433,1=10.42.0.11:5433,2=10.42.0.12:5433"
            )
        );
        // kmb.ip= + kmb.gw= (our own params — Ubuntu kernel has CONFIG_IP_PNP=n
        // so we configure the interface manually in /sbin/init).
        assert!(spec.kernel_cmdline.contains("kmb.ip=10.42.0.11/24"));
        assert!(spec.kernel_cmdline.contains("kmb.gw=10.42.0.1"));
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
        assert_eq!(
            controller.replica_endpoint(0, 2).as_deref(),
            Some("http://10.42.0.12:9000")
        );
        assert!(controller.replica_endpoint(9, 0).is_none());
    }
}
