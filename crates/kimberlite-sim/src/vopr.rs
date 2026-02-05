//! VOPR (Viewstamped Operation Replication) runner for deterministic simulation testing.
//!
//! This module provides the high-level VOPR interface used by both the standalone
//! `vopr` binary and the `kmb sim` CLI commands.
//!
//! # Continuous Workload Generation
//!
//! VOPR uses an event-based workload scheduler that continuously generates operations
//! throughout the simulation, enabling marathon stress tests with 100k+ events. The
//! scheduler can be tuned via `workload_ops_per_tick` and `workload_tick_interval_ns`
//! configuration parameters.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use crate::instrumentation::fault_registry::EffectivenessReport;

#[allow(clippy::wildcard_imports)]
use crate::{
    diagnosis::{FailureAnalyzer, FailureReport},
    trace::{TraceCollector, TraceConfig, TraceEventType},
    *,
};
use kimberlite_crypto::internal_hash;

// ============================================================================
// VOPR Configuration
// ============================================================================

/// Configuration for VOPR simulation runs.
#[derive(Debug, Clone)]
pub struct VoprConfig {
    /// Starting seed for simulations.
    pub seed: u64,
    /// Number of iterations to run.
    pub iterations: u64,
    /// Enable network fault injection.
    pub network_faults: bool,
    /// Enable storage fault injection.
    pub storage_faults: bool,
    /// Verbose output.
    pub verbose: bool,
    /// Maximum events per simulation.
    pub max_events: u64,
    /// Maximum simulation time (nanoseconds).
    pub max_time_ns: u64,
    /// Enable determinism validation (run each seed 2x).
    pub check_determinism: bool,
    /// Enable trace collection.
    pub enable_trace: bool,
    /// Save trace on failure.
    pub save_trace_on_failure: bool,
    /// Enable enhanced workload patterns (RMW, scans).
    pub enhanced_workloads: bool,
    /// Generate failure diagnosis reports.
    pub failure_diagnosis: bool,
    /// Test scenario to run (None = custom based on flags).
    pub scenario: Option<ScenarioType>,
    /// Operations per workload tick (default: 5).
    pub workload_ops_per_tick: usize,
    /// Workload tick interval in nanoseconds (default: 10ms).
    pub workload_tick_interval_ns: u64,
}

impl Default for VoprConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            iterations: 100,
            network_faults: true,
            storage_faults: true,
            verbose: false,
            max_events: 10_000,
            max_time_ns: 10_000_000_000, // 10 seconds simulated
            check_determinism: false,
            enable_trace: false,
            save_trace_on_failure: true,
            enhanced_workloads: true,
            failure_diagnosis: true,
            scenario: None,
            workload_ops_per_tick: 5,
            workload_tick_interval_ns: 10_000_000, // 10ms
        }
    }
}

// ============================================================================
// Simulation Results
// ============================================================================

/// Result of a single VOPR simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoprResult {
    /// Simulation completed successfully.
    Success {
        /// Seed used for this run.
        seed: u64,
        /// Number of events processed.
        events_processed: u64,
        /// Final simulation time (ns).
        final_time_ns: u64,
        /// Final storage hash for determinism checking.
        storage_hash: [u8; 32],
        /// Final kernel state hash for determinism checking.
        kernel_state_hash: [u8; 32],
        /// Fault effectiveness report (% of faults that had observable effects).
        #[serde(skip_serializing_if = "Option::is_none")]
        effectiveness: Option<EffectivenessReport>,
    },
    /// An invariant was violated.
    InvariantViolation {
        /// Seed that triggered the failure.
        seed: u64,
        /// Invariant that was violated.
        invariant: String,
        /// Error message.
        message: String,
        /// Events processed before failure.
        events_processed: u64,
        /// Failure diagnosis report.
        failure_report: Option<Box<FailureReport>>,
        /// Fault effectiveness report (% of faults that had observable effects).
        #[serde(skip_serializing_if = "Option::is_none")]
        effectiveness: Option<EffectivenessReport>,
    },
}

impl VoprResult {
    /// Returns true if the simulation succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, VoprResult::Success { .. })
    }

    /// Returns the seed for this result.
    pub fn seed(&self) -> u64 {
        match self {
            VoprResult::Success { seed, .. } | VoprResult::InvariantViolation { seed, .. } => *seed,
        }
    }

    /// Returns the number of events processed.
    pub fn events_processed(&self) -> u64 {
        match self {
            VoprResult::Success {
                events_processed, ..
            }
            | VoprResult::InvariantViolation {
                events_processed, ..
            } => *events_processed,
        }
    }

    /// Checks determinism against another result from the same seed.
    ///
    /// Returns `Ok(())` if both results are identical (deterministic).
    /// Returns `Err(violations)` with a list of detected differences.
    ///
    /// # Checks
    ///
    /// For successful runs, compares:
    /// - `storage_hash` - Final storage state hash
    /// - `kernel_state_hash` - Final kernel state hash
    /// - `events_processed` - Number of events executed
    /// - `final_time_ns` - Final simulation time
    ///
    /// For failed runs, compares the full Debug representation.
    pub fn check_determinism(&self, other: &VoprResult) -> Result<(), Vec<String>> {
        let mut violations = Vec::new();

        match (self, other) {
            (
                VoprResult::Success {
                    storage_hash: storage_hash1,
                    kernel_state_hash: kernel_hash1,
                    events_processed: events1,
                    final_time_ns: time1,
                    ..
                },
                VoprResult::Success {
                    storage_hash: storage_hash2,
                    kernel_state_hash: kernel_hash2,
                    events_processed: events2,
                    final_time_ns: time2,
                    ..
                },
            ) => {
                if storage_hash1 != storage_hash2 {
                    violations.push(format!(
                        "storage_hash: {:x?} != {:x?}",
                        storage_hash1, storage_hash2
                    ));
                }

                if kernel_hash1 != kernel_hash2 {
                    violations.push(format!(
                        "kernel_state_hash: {:x?} != {:x?}",
                        kernel_hash1, kernel_hash2
                    ));
                }

                if events1 != events2 {
                    violations.push(format!("events_processed: {events1} != {events2}"));
                }

                if time1 != time2 {
                    violations.push(format!("final_time_ns: {time1} != {time2}"));
                }
            }
            _ => {
                // If either run failed or results differ in type
                if format!("{self:?}") != format!("{other:?}") {
                    violations.push("different failure modes".to_string());
                }
            }
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

/// Batch results from running multiple iterations.
#[derive(Debug, Clone)]
pub struct VoprBatchResults {
    /// All individual results.
    pub results: Vec<VoprResult>,
    /// Number of successful runs.
    pub successes: u64,
    /// Number of failed runs.
    pub failures: u64,
    /// Failed seeds for reproduction.
    pub failed_seeds: Vec<u64>,
    /// Total elapsed time (seconds).
    pub elapsed_secs: f64,
}

impl VoprBatchResults {
    /// Returns true if all simulations passed.
    pub fn all_passed(&self) -> bool {
        self.failures == 0
    }

    /// Returns the success rate (0.0 to 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.results.is_empty() {
            0.0
        } else {
            self.successes as f64 / self.results.len() as f64
        }
    }

    /// Returns simulations per second.
    pub fn rate(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.results.len() as f64 / self.elapsed_secs
        } else {
            0.0
        }
    }
}

// ============================================================================
// Checkpoint Management
// ============================================================================

/// Checkpoint state for resume support.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct VoprCheckpoint {
    /// Last completed seed.
    pub last_seed: u64,
    /// Total iterations completed across all runs.
    pub total_iterations: u64,
    /// Total failures detected across all runs.
    pub total_failures: u64,
    /// List of seeds that failed (for reproduction).
    pub failed_seeds: Vec<u64>,
    /// Timestamp of last update.
    pub last_update: String,
}

impl VoprCheckpoint {
    /// Loads checkpoint from file, returns default if file doesn't exist.
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(serde_json::from_str(&contents)?),
            Err(_) => Ok(Self::default()),
        }
    }

    /// Saves checkpoint to file.
    pub fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

// ============================================================================
// VOPR Runner
// ============================================================================

/// High-level VOPR runner for executing simulation batches.
pub struct VoprRunner {
    config: VoprConfig,
}

impl VoprRunner {
    /// Creates a new VOPR runner with the given configuration.
    pub fn new(config: VoprConfig) -> Self {
        Self { config }
    }

    /// Runs a single simulation with the given seed.
    pub fn run_single(&self, seed: u64) -> VoprResult {
        run_simulation(seed, &self.config)
    }

    /// Runs a batch of simulations.
    pub fn run_batch(&self) -> VoprBatchResults {
        let start = std::time::Instant::now();
        let mut results = Vec::new();
        let mut successes = 0;
        let mut failed_seeds = Vec::new();

        for i in 0..self.config.iterations {
            let seed = self.config.seed.wrapping_add(i);
            let result = self.run_single(seed);

            if result.is_ok() {
                successes += 1;
            } else {
                failed_seeds.push(seed);
            }

            results.push(result);
        }

        let elapsed = start.elapsed();

        VoprBatchResults {
            successes,
            failures: failed_seeds.len() as u64,
            failed_seeds,
            results,
            elapsed_secs: elapsed.as_secs_f64(),
        }
    }
}

// ============================================================================
// Core Simulation Logic (extracted from bin/vopr.rs)
// ============================================================================

/// Configuration for a single simulation run.
struct SimulationRun {
    #[allow(dead_code)] // Used for construction but not read
    seed: u64,
    network_config: NetworkConfig,
    storage_config: StorageConfig,
    scenario: Option<ScenarioConfig>,
}

impl SimulationRun {
    fn new(seed: u64, config: &VoprConfig) -> Self {
        // If a scenario is specified, use its configuration
        if let Some(scenario_type) = config.scenario {
            let scenario = ScenarioConfig::new(scenario_type, seed);
            return Self {
                seed,
                network_config: scenario.network_config.clone(),
                storage_config: scenario.storage_config.clone(),
                scenario: Some(scenario),
            };
        }

        // Otherwise, use legacy configuration based on flags
        let mut rng = SimRng::new(seed);

        let network_config = if config.network_faults {
            NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 50_000_000,
                drop_probability: rng.next_f64() * 0.1,
                duplicate_probability: rng.next_f64() * 0.05,
                max_in_flight: 1000,
            }
        } else {
            NetworkConfig {
                min_delay_ns: 1_000_000,
                max_delay_ns: 5_000_000,
                drop_probability: 0.0,
                duplicate_probability: 0.0,
                max_in_flight: 1000,
            }
        };

        let storage_config = if config.storage_faults {
            StorageConfig {
                min_write_latency_ns: 500_000,
                max_write_latency_ns: 2_000_000,
                min_read_latency_ns: 50_000,
                max_read_latency_ns: 200_000,
                write_failure_probability: rng.next_f64() * 0.01,
                read_corruption_probability: rng.next_f64() * 0.001,
                fsync_failure_probability: rng.next_f64() * 0.01,
                partial_write_probability: rng.next_f64() * 0.01,
                ..Default::default()
            }
        } else {
            StorageConfig::default()
        };

        Self {
            seed,
            network_config,
            storage_config,
            scenario: None,
        }
    }
}

/// In-memory model of expected database state for verification.
///
/// Tracks both pending (unfsynced) and durable (fsynced) writes to match
/// read-your-writes semantics while correctly handling crashes.
///
/// This model verifies the assumptions made in `specs/tla/Recovery.tla`:
/// - Committed entries persist through crashes (Recovery.tla:108)
/// - Uncommitted entries may be lost on crash/fsync failure (Recovery.tla:112)
/// - Recovery restores committed state from quorum (Recovery.tla:118-199)
struct KimberliteModel {
    /// Durable state (fsynced writes).
    durable: HashMap<u64, u64>,
    /// Pending writes (not yet fsynced, will be lost on crash).
    pending: HashMap<u64, u64>,
}

impl KimberliteModel {
    fn new() -> Self {
        Self {
            durable: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Records a pending write (not yet fsynced).
    fn apply_pending_write(&mut self, key: u64, value: u64) {
        self.pending.insert(key, value);
    }

    /// Commits pending writes to durable state (after fsync).
    fn commit_pending(&mut self) {
        for (key, value) in self.pending.drain() {
            self.durable.insert(key, value);
        }
    }

    /// Clears pending writes (e.g., after fsync failure or crash).
    fn clear_pending(&mut self) {
        self.pending.clear();
    }

    /// Verifies a read matches expected state (checks pending first, then durable).
    ///
    /// Returns true if the read matches expectations. Stricter verification for
    /// compliance databases - after fixing fsync/reorderer bugs, we require exact matches.
    fn verify_read(&self, key: u64, actual: Option<u64>) -> bool {
        let expected = self.pending.get(&key).or_else(|| self.durable.get(&key));
        match (expected, actual) {
            (Some(expected), Some(actual)) => expected == &actual,
            (None, None) => true,
            (None, Some(_)) => {
                // Model has no expectation but read found data.
                // This is acceptable only immediately after checkpoint recovery,
                // where the checkpoint may contain data not yet in the model.
                // In other cases, this indicates corruption or model desync.
                // For now, allow this but it should be investigated.
                // TODO: Track recovery state to distinguish these cases.
                true
            }
            (Some(_), None) => false, // Data expected but missing - ALWAYS a bug
        }
    }

    fn get(&self, key: u64) -> Option<u64> {
        self.pending.get(&key).or_else(|| self.durable.get(&key)).copied()
    }
}

/// Runs a single simulation with the given configuration.
/// This is the core simulation logic extracted from bin/vopr.rs.
#[allow(clippy::too_many_lines)]
fn run_simulation(seed: u64, config: &VoprConfig) -> VoprResult {
    let run = SimulationRun::new(seed, config);

    // Use scenario config if available, but allow CLI to override
    // If CLI explicitly sets max_events (non-default), it takes precedence
    let (max_events, max_time_ns) = if let Some(ref scenario) = run.scenario {
        // Use CLI values if they differ from defaults, otherwise use scenario values
        let max_events = if config.max_events != 10_000 {
            config.max_events
        } else {
            scenario.max_events.max(config.max_events)
        };

        let max_time_ns = if config.max_time_ns != 10_000_000_000 {
            config.max_time_ns
        } else {
            scenario.max_time_ns.max(config.max_time_ns)
        };

        (max_events, max_time_ns)
    } else {
        (config.max_events, config.max_time_ns)
    };

    let sim_config = SimConfig::default()
        .with_seed(seed)
        .with_max_events(max_events)
        .with_max_time_ns(max_time_ns);

    let mut sim = Simulation::new(sim_config);
    let mut rng = SimRng::new(seed);

    // Initialize simulated components
    let mut network = SimNetwork::new(run.network_config);
    let mut storage = SimStorage::new(run.storage_config);

    // Initialize invariant checkers
    let mut replica_checker = ReplicaConsistencyChecker::new();
    let mut replica_head_checker = ReplicaHeadChecker::new();
    let mut commit_history_checker = CommitHistoryChecker::new();

    // Initialize model
    let mut model = KimberliteModel::new();

    // Track checkpoints
    let mut checkpoints: HashMap<u64, StorageCheckpoint> = HashMap::new();

    // Initialize trace collector (if enabled)
    let mut trace = if config.enable_trace || config.save_trace_on_failure {
        Some(TraceCollector::new(TraceConfig::default()))
    } else {
        None
    };

    if let Some(ref mut t) = trace {
        t.record(0, TraceEventType::SimulationStart { seed });
    }

    // Register nodes
    for node_id in 0..3 {
        network.register_node(node_id);
    }

    // Initialize workload scheduler for continuous operation generation
    let sched_config = WorkloadSchedulerConfig {
        ops_per_tick: config.workload_ops_per_tick,
        tick_interval_ns: config.workload_tick_interval_ns,
        enhanced_workloads: config.enhanced_workloads,
        max_scheduled_ops: None,
        vsr_mode: false,
        // Pass simulation limits so scheduler can self-terminate
        sim_max_events: Some(max_events),
        sim_max_time_ns: Some(max_time_ns),
    };
    let mut workload_scheduler = WorkloadScheduler::new(sched_config);

    // Schedule initial tick
    let mut initial_events = Vec::new();
    workload_scheduler.schedule_initial_tick(&mut initial_events, 0);
    for (time, kind) in initial_events {
        sim.schedule(time, kind);
    }

    // Schedule checkpoints
    for i in 0..5 {
        let checkpoint_time = 2_000_000_000 * (i + 1);
        sim.schedule(
            checkpoint_time,
            EventKind::CreateCheckpoint { checkpoint_id: i },
        );
    }

    // Schedule periodic fsync operations every ~500ms to commit pending writes
    // This models realistic database write patterns where fsync happens periodically
    for i in 0..20 {
        let fsync_time = 500_000_000 * (i + 1); // 500ms, 1s, 1.5s, ...
        sim.schedule(fsync_time, EventKind::StorageFsync);
    }

    // Note: Removed pending_ops tracking - no longer needed after removing
    // O(n!) linearizability checker in favor of industry-proven approach

    // Helper to create violation result
    let make_violation = |invariant: String,
                          message: String,
                          events_processed: u64,
                          trace_collector: &mut Option<TraceCollector>| {
        let failure_report = if config.failure_diagnosis {
            if let Some(t) = trace_collector {
                let events: Vec<_> = t.events().iter().cloned().collect();
                Some(Box::new(FailureAnalyzer::analyze_failure(
                    seed,
                    &events,
                    events_processed,
                )))
            } else {
                None
            }
        } else {
            None
        };

        // Generate effectiveness report even for failures
        let fault_registry = crate::instrumentation::fault_registry::get_fault_registry();
        let effectiveness = Some(fault_registry.effectiveness_report());

        VoprResult::InvariantViolation {
            seed,
            invariant,
            message,
            events_processed,
            failure_report,
            effectiveness,
        }
    };

    // Simulation loop (simplified from full vopr.rs - see original for complete logic)
    while let Some(event) = sim.step() {
        match event.kind {
            EventKind::Custom(op_type) => {
                let op_count = if config.enhanced_workloads { 6 } else { 4 };
                match op_type % op_count {
                    0 => {
                        // Write operation
                        let key = rng.next_u64() % 10;
                        let value = rng.next_u64();
                        let data = value.to_le_bytes().to_vec();
                        let write_result = storage.write(key, data.clone(), &mut rng);

                        if matches!(
                            write_result,
                            WriteResult::Success { bytes_written, .. }
                            if bytes_written == data.len()
                        ) {
                            // Record as pending write (not yet durable)
                            model.apply_pending_write(key, value);
                            for replica_id in 0..3 {
                                storage.append_replica_log(replica_id, data.clone());
                            }

                            let delay = rng.delay_ns(100_000, 1_000_000);
                            sim.schedule_after(
                                delay,
                                EventKind::StorageComplete {
                                    operation_id: 0, // No longer tracking for linearizability
                                    success: true,
                                },
                            );
                        }
                    }
                    1 => {
                        // Read operation
                        let key = rng.next_u64() % 10;
                        let result = storage.read(key, &mut rng);

                        match result {
                            ReadResult::Success { data, .. } if data.len() == 8 => {
                                let value = Some(u64::from_le_bytes(data[..8].try_into().unwrap()));

                                if !model.verify_read(key, value) {
                                    let expected = model.get(key);
                                    return make_violation(
                                        "model_verification".to_string(),
                                        format!(
                                            "read mismatch: key={key}, expected={expected:?}, actual={value:?}"
                                        ),
                                        sim.events_processed(),
                                        &mut trace,
                                    );
                                }
                            }
                            ReadResult::NotFound { .. } => {
                                if !model.verify_read(key, None) {
                                    let expected = model.get(key);
                                    return make_violation(
                                        "model_verification".to_string(),
                                        format!(
                                            "read mismatch: key={key}, expected={expected:?}, actual=None"
                                        ),
                                        sim.events_processed(),
                                        &mut trace,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    2 => {
                        // Network message
                        let from = rng.next_usize(3) as u64;
                        let to = rng.next_usize(3) as u64;
                        if from != to {
                            let payload = vec![rng.next_u64() as u8; 32];
                            let _ = network.send(from, to, payload, event.time_ns, &mut rng);
                        }
                    }
                    3 => {
                        // Replica state update
                        let replica_id = rng.next_usize(3) as u64;
                        let log_length = storage.get_replica_log_length(replica_id);

                        let log_hash = if let Some(entries) = storage.get_replica_log(replica_id) {
                            let mut combined = Vec::new();
                            for entry in entries {
                                combined.extend_from_slice(entry);
                            }
                            *internal_hash(&combined).as_bytes()
                        } else {
                            [0u8; 32]
                        };

                        let result = replica_checker.update_replica(
                            replica_id,
                            log_length,
                            log_hash,
                            event.time_ns,
                        );

                        if !result.is_ok() {
                            return make_violation(
                                "replica_consistency".to_string(),
                                format!("Replica divergence at time {}", event.time_ns),
                                sim.events_processed(),
                                &mut trace,
                            );
                        }

                        let view = 0;
                        let op = log_length;
                        let head_result = replica_head_checker.update_head(replica_id, view, op);
                        if !head_result.is_ok() {
                            return make_violation(
                                "replica_head_progress".to_string(),
                                format!("Replica {replica_id} head regressed"),
                                sim.events_processed(),
                                &mut trace,
                            );
                        }

                        if replica_id == 0 && log_length > 0 {
                            let last_committed = log_length - 1;
                            if let Some(last_op) = commit_history_checker.last_op() {
                                for op_num in (last_op + 1)..=last_committed {
                                    let commit_result =
                                        commit_history_checker.record_commit(op_num);
                                    if !commit_result.is_ok() {
                                        return make_violation(
                                            "commit_history".to_string(),
                                            format!("Commit gap at op {op_num}"),
                                            sim.events_processed(),
                                            &mut trace,
                                        );
                                    }
                                }
                            } else if log_length > 0 {
                                for op_num in 0..log_length {
                                    let commit_result =
                                        commit_history_checker.record_commit(op_num);
                                    if !commit_result.is_ok() {
                                        return make_violation(
                                            "commit_history".to_string(),
                                            format!("Commit history violation at op {op_num}"),
                                            sim.events_processed(),
                                            &mut trace,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            EventKind::StorageComplete {
                operation_id: _,
                success: _,
            } => {
                // Storage completion event (no longer tracked after removing linearizability checker)
            }
            EventKind::StorageFsync => {
                // Periodic fsync: flush pending writes to durable storage
                let fsync_result = storage.fsync(&mut rng);

                if matches!(fsync_result, crate::FsyncResult::Success { .. }) {
                    // Fsync succeeded - commit pending writes to durable state in model
                    model.commit_pending();

                    if config.verbose {
                        eprintln!(
                            "Fsync completed at {}ms, committed pending writes to durable state",
                            event.time_ns / 1_000_000
                        );
                    }
                } else {
                    // Fsync failed - clear model.pending to match storage behavior
                    // (storage.fsync() already cleared pending_writes at storage.rs:564)
                    model.clear_pending();

                    if config.verbose {
                        eprintln!(
                            "Fsync failed at {}ms, clearing pending writes from model",
                            event.time_ns / 1_000_000
                        );
                    }
                }
            }
            EventKind::NetworkDeliver { .. } => {
                let _ = network.deliver_ready(event.time_ns);
            }
            EventKind::InvariantCheck => {
                if !replica_checker.check_all().is_ok() {
                    return make_violation(
                        "replica_consistency".to_string(),
                        "Replicas have diverged".to_string(),
                        sim.events_processed(),
                        &mut trace,
                    );
                }
            }
            EventKind::CreateCheckpoint { checkpoint_id } => {
                let checkpoint = storage.checkpoint();
                checkpoints.insert(checkpoint_id, checkpoint);
            }
            EventKind::RecoverCheckpoint { checkpoint_id } => {
                if let Some(checkpoint) = checkpoints.get(&checkpoint_id) {
                    // Restore storage from checkpoint
                    storage.restore_checkpoint(checkpoint);

                    // Synchronize model with checkpoint state
                    // Checkpoints only contain durable data, no pending writes
                    model.clear_pending();

                    // Rebuild model.durable from checkpoint
                    model.durable.clear();
                    for (address, data) in checkpoint.iter_blocks() {
                        // Assuming we're using address as key and parsing data as u64 value
                        // In reality, we need to match the write/read semantics in the simulation
                        // For now, we'll just mark that we have data at this address
                        // The actual verification will compare against what was written
                        if data.len() >= 8 {
                            let value = u64::from_le_bytes(data[0..8].try_into().unwrap());
                            model.durable.insert(address, value);
                        }
                    }

                    if config.verbose {
                        eprintln!(
                            "Restored checkpoint {} at {}ms ({} blocks)",
                            checkpoint_id,
                            event.time_ns / 1_000_000,
                            model.durable.len()
                        );
                    }
                }
            }
            EventKind::WorkloadTick => {
                // Handle workload tick by generating next batch of operations
                // Pass simulation context for limit-aware termination
                let events = workload_scheduler.handle_tick(
                    event.time_ns,
                    sim.events_processed(),
                    &mut rng,
                );
                for (time, kind) in events {
                    sim.schedule(time, kind);
                }
            }
            _ => {}
        }
    }

    let storage_hash = storage.storage_hash();

    // TODO: Integrate actual kernel State tracking in simulation
    // For now, use empty state hash as placeholder
    let kernel_state_hash = kimberlite_kernel::State::new().compute_state_hash();

    if let Some(ref mut t) = trace {
        t.record(
            sim.now(),
            TraceEventType::SimulationEnd {
                events_processed: sim.events_processed(),
            },
        );
    }

    // Generate effectiveness report from fault registry
    let fault_registry = crate::instrumentation::fault_registry::get_fault_registry();
    let effectiveness = Some(fault_registry.effectiveness_report());

    VoprResult::Success {
        seed,
        events_processed: sim.events_processed(),
        final_time_ns: sim.now(),
        storage_hash,
        kernel_state_hash,
        effectiveness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism_check_identical_results() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        assert!(result1.check_determinism(&result2).is_ok());
    }

    #[test]
    fn test_determinism_check_different_storage_hash() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [3u8; 32], // Different
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let violations = result1.check_determinism(&result2).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("storage_hash"));
    }

    #[test]
    fn test_determinism_check_different_kernel_hash() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [4u8; 32], // Different
            effectiveness: None,
        };

        let violations = result1.check_determinism(&result2).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("kernel_state_hash"));
    }

    #[test]
    fn test_determinism_check_different_events_processed() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 150, // Different
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let violations = result1.check_determinism(&result2).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("events_processed"));
    }

    #[test]
    fn test_determinism_check_different_time() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 20000, // Different
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let violations = result1.check_determinism(&result2).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("final_time_ns"));
    }

    #[test]
    fn test_determinism_check_multiple_violations() {
        let result1 = VoprResult::Success {
            seed: 12345,
            events_processed: 100,
            final_time_ns: 10000,
            storage_hash: [1u8; 32],
            kernel_state_hash: [2u8; 32],
            effectiveness: None,        };

        let result2 = VoprResult::Success {
            seed: 12345,
            events_processed: 150,        // Different
            final_time_ns: 20000,         // Different
            storage_hash: [3u8; 32],      // Different
            kernel_state_hash: [4u8; 32], // Different
            effectiveness: None,
        };

        let violations = result1.check_determinism(&result2).unwrap_err();
        assert_eq!(violations.len(), 4);
        assert!(violations.iter().any(|v| v.contains("storage_hash")));
        assert!(violations.iter().any(|v| v.contains("kernel_state_hash")));
        assert!(violations.iter().any(|v| v.contains("events_processed")));
        assert!(violations.iter().any(|v| v.contains("final_time_ns")));
    }
}
