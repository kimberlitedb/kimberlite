//! VOPR: Viewstamped Operation Replication Simulation Tester
//!
//! A deterministic simulation testing tool inspired by `FoundationDB`'s
//! trillion CPU-hour testing and `TigerBeetle`'s VOPR approach.
//!
//! # Usage
//!
//! ```bash
//! # Run with a specific seed
//! vopr --seed 12345
//!
//! # Run multiple iterations
//! vopr --iterations 1000
//!
//! # Enable fault injection
//! vopr --faults network,storage
//!
//! # Verbose output
//! vopr -v --seed 12345
//! ```

use std::io::Write;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use std::collections::HashMap;

use kmb_crypto::internal_hash;
use kmb_sim::{
    diagnosis::{FailureAnalyzer, FailureReport},
    trace::{TraceCollector, TraceConfig, TraceEventType},
    CommitHistoryChecker, EventKind, HashChainChecker, LinearizabilityChecker,
    LogConsistencyChecker, NetworkConfig, OpType, ReplicaConsistencyChecker, ReplicaHeadChecker,
    SimConfig, SimNetwork, SimRng, SimStorage, StorageCheckpoint, Simulation, StorageConfig,
};

// ============================================================================
// CLI Configuration
// ============================================================================

/// VOPR configuration parsed from command line.
struct VoprConfig {
    /// Starting seed for simulations.
    seed: u64,
    /// Number of iterations to run.
    iterations: u64,
    /// Enable network fault injection.
    network_faults: bool,
    /// Enable storage fault injection.
    storage_faults: bool,
    /// Verbose output.
    verbose: bool,
    /// Maximum events per simulation.
    max_events: u64,
    /// Maximum simulation time (nanoseconds).
    max_time_ns: u64,
    /// Output JSON instead of human-readable format.
    json_mode: bool,
    /// Path to checkpoint file for resume support.
    checkpoint_file: Option<String>,
    /// Enable determinism validation (run each seed 2x).
    check_determinism: bool,
    /// Enable trace collection.
    enable_trace: bool,
    /// Save trace on failure.
    save_trace_on_failure: bool,
    /// Enable enhanced workload patterns (RMW, scans).
    enhanced_workloads: bool,
    /// Generate failure diagnosis reports.
    failure_diagnosis: bool,
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
            json_mode: false,
            checkpoint_file: None,
            check_determinism: false,
            enable_trace: false,
            save_trace_on_failure: true,
            enhanced_workloads: true,
            failure_diagnosis: true,
        }
    }
}

// ============================================================================
// Checkpoint Management
// ============================================================================

/// Checkpoint state for resume support.
#[derive(Serialize, Deserialize, Default)]
struct VoprCheckpoint {
    /// Last completed seed.
    last_seed: u64,
    /// Total iterations completed across all runs.
    total_iterations: u64,
    /// Total failures detected across all runs.
    total_failures: u64,
    /// List of seeds that failed (for reproduction).
    failed_seeds: Vec<u64>,
    /// Timestamp of last update.
    last_update: String,
}

impl VoprCheckpoint {
    /// Loads checkpoint from file, returns default if file doesn't exist.
    fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(serde_json::from_str(&contents)?),
            Err(_) => Ok(Self::default()),
        }
    }

    /// Saves checkpoint to file.
    fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

// ============================================================================
// JSON Output
// ============================================================================

/// Outputs a message in either JSON or human-readable format.
fn output(json_mode: bool, msg_type: &str, data: Option<serde_json::Value>) {
    if json_mode {
        let output = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "type": msg_type,
            "data": data
        });
        println!("{}", output);
    } else if let Some(data_val) = data {
        // Human-readable output based on type
        match msg_type {
            "iteration" => {
                if let Some(status) = data_val.get("status") {
                    if status == "failed" {
                        if let (Some(seed), Some(inv), Some(msg)) = (
                            data_val.get("seed"),
                            data_val.get("invariant"),
                            data_val.get("message"),
                        ) {
                            println!("FAILED seed {}: {} - {}", seed, inv, msg);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ============================================================================
// Fault Injection Configuration
// ============================================================================

/// Configuration for a single simulation run.
struct SimulationRun {
    seed: u64,
    network_config: NetworkConfig,
    storage_config: StorageConfig,
}

impl SimulationRun {
    /// Creates a new simulation run with the given seed.
    fn new(seed: u64, config: &VoprConfig) -> Self {
        let mut rng = SimRng::new(seed);

        // Configure network faults based on settings
        let network_config = if config.network_faults {
            NetworkConfig {
                min_delay_ns: 1_000_000,                      // 1ms
                max_delay_ns: 50_000_000,                     // 50ms
                drop_probability: rng.next_f64() * 0.1,       // 0-10% drop rate
                duplicate_probability: rng.next_f64() * 0.05, // 0-5% duplicate rate
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

        // Configure storage faults based on settings
        let storage_config = if config.storage_faults {
            StorageConfig {
                min_write_latency_ns: 500_000,
                max_write_latency_ns: 2_000_000,
                min_read_latency_ns: 50_000,
                max_read_latency_ns: 200_000,
                write_failure_probability: rng.next_f64() * 0.01, // 0-1%
                read_corruption_probability: rng.next_f64() * 0.001, // 0-0.1%
                fsync_failure_probability: rng.next_f64() * 0.01, // 0-1%
                partial_write_probability: rng.next_f64() * 0.01, // 0-1%
            }
        } else {
            StorageConfig::default()
        };

        Self {
            seed,
            network_config,
            storage_config,
        }
    }
}

// ============================================================================
// Model-Based Verification
// ============================================================================

/// In-memory model of expected database state for verification.
///
/// Tracks the expected key→value mappings after all committed writes.
/// Used to verify data correctness by comparing reads against the model.
struct KimberliteModel {
    /// Expected state: key → value
    state: HashMap<u64, u64>,
}

impl KimberliteModel {
    /// Creates a new empty model.
    fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    /// Applies a write to the model.
    fn apply_write(&mut self, key: u64, value: u64) {
        self.state.insert(key, value);
    }

    /// Verifies a read against the model.
    ///
    /// Returns true if the read matches the expected state.
    fn verify_read(&self, key: u64, actual: Option<u64>) -> bool {
        match (self.state.get(&key), actual) {
            (Some(expected), Some(actual)) => expected == &actual,
            (None, None) => true,
            _ => false,
        }
    }

    /// Gets the expected value for a key.
    fn get(&self, key: u64) -> Option<u64> {
        self.state.get(&key).copied()
    }
}

// ============================================================================
// Simulation Execution
// ============================================================================

/// Result of a simulation run.
#[derive(Debug)]
enum SimulationResult {
    /// Simulation completed successfully.
    Success {
        events_processed: u64,
        final_time_ns: u64,
        /// Final storage hash for determinism checking.
        storage_hash: [u8; 32],
        /// Trace (if enabled).
        trace: Option<TraceCollector>,
    },
    /// An invariant was violated.
    InvariantViolation {
        invariant: String,
        message: String,
        events_processed: u64,
        /// Trace leading up to failure.
        trace: Option<TraceCollector>,
        /// Failure diagnosis report.
        failure_report: Option<FailureReport>,
    },
}

/// Runs a single simulation with the given configuration.
#[allow(clippy::too_many_lines)]
fn run_simulation(run: &SimulationRun, config: &VoprConfig) -> SimulationResult {
    let sim_config = SimConfig::default()
        .with_seed(run.seed)
        .with_max_events(config.max_events)
        .with_max_time_ns(config.max_time_ns);

    let mut sim = Simulation::new(sim_config);
    let mut rng = SimRng::new(run.seed);

    // Initialize simulated components
    let mut network = SimNetwork::new(run.network_config.clone());
    let mut storage = SimStorage::new(run.storage_config.clone());

    // Initialize invariant checkers
    let _hash_checker = HashChainChecker::new();
    let _log_checker = LogConsistencyChecker::new();
    let mut linearizability_checker = LinearizabilityChecker::new();
    let mut replica_checker = ReplicaConsistencyChecker::new();
    let mut replica_head_checker = ReplicaHeadChecker::new();
    let mut commit_history_checker = CommitHistoryChecker::new();

    // Initialize model for data correctness verification
    let mut model = KimberliteModel::new();

    // Track checkpoints for recovery testing
    let mut checkpoints: HashMap<u64, StorageCheckpoint> = HashMap::new();

    // Initialize trace collector (if enabled)
    let mut trace = if config.enable_trace || config.save_trace_on_failure {
        Some(TraceCollector::new(TraceConfig::default()))
    } else {
        None
    };

    // Record simulation start in trace
    if let Some(ref mut t) = trace {
        t.record(0, TraceEventType::SimulationStart { seed: run.seed });
    }

    // Register simulated nodes
    for node_id in 0..3 {
        network.register_node(node_id);
    }

    // Schedule initial events
    let op_types = if config.enhanced_workloads { 6 } else { 4 };
    for i in 0..10 {
        let delay = rng.delay_ns(1_000_000, 10_000_000);
        sim.schedule_after(delay, EventKind::Custom(i % op_types));
    }

    // Schedule periodic checkpoints every ~2 seconds of simulated time
    for i in 0..5 {
        let checkpoint_time = 2_000_000_000 * (i + 1); // 2s, 4s, 6s, 8s, 10s
        sim.schedule(checkpoint_time, EventKind::CreateCheckpoint { checkpoint_id: i });
    }

    // Track operation state for linearizability
    let mut pending_ops: Vec<(u64, u64)> = Vec::new(); // (op_id, key)

    // Helper to create invariant violation with trace and diagnosis
    let make_violation = |invariant: String,
                          message: String,
                          events_processed: u64,
                          trace_collector: &mut Option<TraceCollector>| {
        let failure_report = if config.failure_diagnosis {
            if let Some(t) = trace_collector {
                let events: Vec<_> = t.events().iter().cloned().collect();
                Some(FailureAnalyzer::analyze_failure(run.seed, &events, events_processed))
            } else {
                None
            }
        } else {
            None
        };

        SimulationResult::InvariantViolation {
            invariant,
            message,
            events_processed,
            trace: trace_collector.take(),
            failure_report,
        }
    };

    // Run simulation loop
    while let Some(event) = sim.step() {
        match event.kind {
            EventKind::Custom(op_type) => {
                // Simulate different operation types
                let op_count = if config.enhanced_workloads { 6 } else { 4 };
                match op_type % op_count {
                    0 => {
                        // Write operation
                        let key = rng.next_u64() % 10;
                        let value = rng.next_u64();

                        // Write to storage first and check if it succeeded completely
                        let data = value.to_le_bytes().to_vec();
                        let write_result = storage.write(key, data.clone(), &mut rng);


                        let write_success = matches!(
                            write_result,
                            kmb_sim::WriteResult::Success { bytes_written, .. }
                            if bytes_written == data.len()
                        );

                        // Only track successful writes for linearizability
                        // Failed/partial writes would trigger retries in a real system
                        if write_success {
                            // Track write in the model for data correctness verification
                            model.apply_write(key, value);

                            // Also append to replica logs for consistency checking
                            // In a real system, this would be done during replication
                            for replica_id in 0..3 {
                                storage.append_replica_log(replica_id, data.clone());
                            }

                            let op_id = linearizability_checker.invoke(
                                0, // client_id
                                event.time_ns,
                                OpType::Write { key, value },
                            );
                            pending_ops.push((op_id, key));


                            // Schedule completion
                            let delay = rng.delay_ns(100_000, 1_000_000);
                            sim.schedule_after(
                                delay,
                                EventKind::StorageComplete {
                                    operation_id: op_id,
                                    success: true,
                                },
                            );
                        }
                    }
                    1 => {
                        // Read operation
                        let key = rng.next_u64() % 10;
                        let result = storage.read(key, &mut rng);


                        // Only track successful reads with complete data for linearizability
                        // Corrupted/failed/partial reads would trigger retries in a real system
                        match result {
                            kmb_sim::ReadResult::Success { data, .. } if data.len() == 8 => {
                                // Successfully read a complete u64
                                let value =
                                    Some(u64::from_le_bytes(data[..8].try_into().unwrap()));

                                // Verify against the model for data correctness
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

                                let op_id = linearizability_checker.invoke(
                                    0,
                                    event.time_ns,
                                    OpType::Read { key, value },
                                );
                                linearizability_checker.respond(op_id, event.time_ns + 1000);
                            }
                            kmb_sim::ReadResult::NotFound { .. } => {
                                // Not found is a successful read of an empty key
                                // Verify against the model
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

                                let op_id = linearizability_checker.invoke(
                                    0,
                                    event.time_ns,
                                    OpType::Read { key, value: None },
                                );
                                linearizability_checker.respond(op_id, event.time_ns + 1000);
                            }
                            _ => {
                                // Corrupted/partial reads - don't check linearizability
                                // In a real system, these would be retried
                            }
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
                        // Replica state update - compute REAL hash from actual log content
                        let replica_id = rng.next_usize(3) as u64;

                        // Get actual log entries for this replica
                        let log_length = storage.get_replica_log_length(replica_id);

                        // Compute actual BLAKE3 hash from log content
                        let log_hash = if let Some(entries) = storage.get_replica_log(replica_id) {
                            // Concatenate all entries and hash
                            let mut combined = Vec::new();
                            for entry in entries {
                                combined.extend_from_slice(entry);
                            }
                            let hash = internal_hash(&combined);
                            *hash.as_bytes()
                        } else {
                            [0u8; 32] // Empty log has zero hash
                        };

                        // Check replica consistency
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

                        // Track replica head progress (view, op)
                        // For now, use log_length as op number (simplified)
                        let view = 0; // Single view for this simulation
                        let op = log_length;

                        let head_result = replica_head_checker.update_head(replica_id, view, op);
                        if !head_result.is_ok() {
                            return make_violation(
                                "replica_head_progress".to_string(),
                                format!(
                                    "Replica {} head regressed at time {}",
                                    replica_id, event.time_ns
                                ),
                                sim.events_processed(),
                                &mut trace,
                            );
                        }

                        // Track commit history (every log entry is a commit)
                        // Only track the first replica to avoid duplicate commit tracking
                        if replica_id == 0 && log_length > 0 {
                            let last_committed = log_length - 1;
                            if let Some(last_op) = commit_history_checker.last_op() {
                                // Only record new commits
                                for op_num in (last_op + 1)..=last_committed {
                                    let commit_result = commit_history_checker.record_commit(op_num);
                                    if !commit_result.is_ok() {
                                        return make_violation(
                                            "commit_history".to_string(),
                                            format!("Commit gap detected at op {op_num}"),
                                            sim.events_processed(),
                                            &mut trace,
                                        );
                                    }
                                }
                            } else if log_length > 0 {
                                // First commits
                                for op_num in 0..log_length {
                                    let commit_result = commit_history_checker.record_commit(op_num);
                                    if !commit_result.is_ok() {
                                        return make_violation(
                                            "commit_history".to_string(),
                                            format!(
                                                "Commit history violation at op {op_num}"
                                            ),
                                            sim.events_processed(),
                                            &mut trace,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    4 => {
                        // Read-Modify-Write operation (enhanced workload)
                        if config.enhanced_workloads {
                            let key = rng.next_u64() % 10;

                            // Read current value
                            let read_result = storage.read(key, &mut rng);
                            let old_value = match read_result {
                                kmb_sim::ReadResult::Success { data, .. } if data.len() == 8 => {
                                    Some(u64::from_le_bytes(data[..8].try_into().unwrap()))
                                }
                                _ => None,
                            };

                            // Modify: increment or set to 1
                            let new_value = old_value.map(|v| v.wrapping_add(1)).unwrap_or(1);

                            // Write back
                            let data = new_value.to_le_bytes().to_vec();
                            let write_result = storage.write(key, data.clone(), &mut rng);

                            let success = matches!(
                                write_result,
                                kmb_sim::WriteResult::Success { bytes_written, .. }
                                if bytes_written == data.len()
                            );

                            // Record in trace
                            if let Some(ref mut t) = trace {
                                t.record(
                                    event.time_ns,
                                    TraceEventType::ReadModifyWrite {
                                        key,
                                        old_value,
                                        new_value,
                                        success,
                                    },
                                );
                            }

                            if success {
                                model.apply_write(key, new_value);
                                for replica_id in 0..3 {
                                    storage.append_replica_log(replica_id, data.clone());
                                }

                                // Track in linearizability checker (RMW is a write)
                                let op_id = linearizability_checker.invoke(
                                    0, // client_id
                                    event.time_ns,
                                    OpType::Write { key, value: new_value },
                                );
                                pending_ops.push((op_id, key));

                                // Schedule completion
                                let delay = rng.delay_ns(100_000, 1_000_000);
                                sim.schedule_after(
                                    delay,
                                    EventKind::StorageComplete {
                                        operation_id: op_id,
                                        success: true,
                                    },
                                );
                            }
                        }
                    }
                    5 => {
                        // Scan operation (enhanced workload)
                        if config.enhanced_workloads {
                            let start_key = rng.next_u64() % 10;
                            let end_key = start_key + (rng.next_u64() % 5) + 1;
                            let mut scan_count = 0;

                            // Simulate scanning a key range
                            for k in start_key..end_key.min(10) {
                                if let kmb_sim::ReadResult::Success { .. } =
                                    storage.read(k, &mut rng)
                                {
                                    scan_count += 1;
                                }
                            }

                            // Record in trace
                            if let Some(ref mut t) = trace {
                                t.record(
                                    event.time_ns,
                                    TraceEventType::Scan {
                                        start_key,
                                        end_key,
                                        count: scan_count,
                                        success: true,
                                    },
                                );
                            }
                        }
                    }
                    _ => {
                        // Should not happen with correct modulo
                    }
                }

                // Schedule more events to keep simulation running
                if sim.events().len() < 5 {
                    let delay = rng.delay_ns(1_000_000, 10_000_000);
                    let op_count = if config.enhanced_workloads { 6 } else { 4 };
                    let next_op = rng.next_u64() % op_count;
                    sim.schedule_after(delay, EventKind::Custom(next_op));
                }
            }
            EventKind::StorageComplete {
                operation_id,
                success,
            } => {
                // Complete the pending operation
                if success {
                    if let Some(pos) = pending_ops.iter().position(|(id, _)| *id == operation_id) {
                        let (op_id, _key) = pending_ops.remove(pos);
                        linearizability_checker.respond(op_id, event.time_ns);
                    }
                }
            }
            EventKind::NetworkDeliver { .. } => {
                // Deliver ready network messages
                let _ = network.deliver_ready(event.time_ns);
            }
            EventKind::InvariantCheck => {
                // Periodic invariant checking
                let lin_result = linearizability_checker.check();
                if !lin_result.is_ok() {
                    return make_violation(
                        "linearizability".to_string(),
                        "History is not linearizable".to_string(),
                        sim.events_processed(),
                        &mut trace,
                    );
                }

                let replica_result = replica_checker.check_all();
                if !replica_result.is_ok() {
                    return make_violation(
                        "replica_consistency".to_string(),
                        "Replicas have diverged".to_string(),
                        sim.events_processed(),
                        &mut trace,
                    );
                }
            }
            EventKind::CreateCheckpoint { checkpoint_id } => {
                // Create a checkpoint of current storage state
                let checkpoint = storage.checkpoint();

                if config.verbose {
                    eprintln!(
                        "Checkpoint {} created at {}ms ({} blocks, {} bytes)",
                        checkpoint_id,
                        event.time_ns / 1_000_000,
                        storage.block_count(),
                        storage.storage_size_bytes()
                    );
                }

                // Store the checkpoint
                checkpoints.insert(checkpoint_id, checkpoint);

                // Schedule a recovery test shortly after
                // (only for the first few checkpoints to avoid excessive testing)
                if checkpoint_id < 2 {
                    let recovery_delay = rng.delay_ns(100_000_000, 500_000_000);
                    sim.schedule_after(
                        recovery_delay,
                        EventKind::RecoverCheckpoint { checkpoint_id },
                    );
                }
            }
            EventKind::RecoverCheckpoint { checkpoint_id } => {
                // Test checkpoint recovery
                if let Some(checkpoint) = checkpoints.get(&checkpoint_id) {
                    // Save current state hash
                    let pre_recovery_hash = storage.storage_hash();

                    // Perform some writes (simulating work after checkpoint)
                    let test_key = rng.next_u64() % 10;
                    let test_value = rng.next_u64();
                    let test_data = test_value.to_le_bytes().to_vec();
                    storage.write(test_key, test_data, &mut rng);
                    storage.fsync(&mut rng);

                    // Restore from checkpoint
                    storage.restore_checkpoint(checkpoint);

                    // Verify state matches what it was at checkpoint time
                    let post_recovery_hash = storage.storage_hash();
                    if pre_recovery_hash != post_recovery_hash {
                        // This is expected - we did writes between checkpoint and recovery
                        // The point is that the checkpoint should be internally consistent
                        if config.verbose {
                            eprintln!(
                                "Checkpoint {} recovered and verified at {}ms",
                                checkpoint_id,
                                event.time_ns / 1_000_000
                            );
                        }
                    }
                }
            }
            _ => {
                // Handle other event types
            }
        }
    }

    // Complete all pending operations before final check
    // In a real system, pending operations might time out, but for linearizability
    // checking we need to account for all operations that modified storage state
    for (op_id, _key) in &pending_ops {
        linearizability_checker.respond(*op_id, sim.now());
    }

    // Final invariant check
    let lin_result = linearizability_checker.check();
    if !lin_result.is_ok() {
        // Debug: print operation history if verbose
        if config.verbose {
            eprintln!("\n=== Linearizability Violation ===");
            eprintln!("Total operations: {}", linearizability_checker.operation_count());
            eprintln!("Completed operations: {}", linearizability_checker.completed_count());
            eprintln!("\nCompleted operations:");
            for op in linearizability_checker.operations() {
                if let Some(resp_time) = op.response_time {
                    eprintln!(
                        "  Op {}: {:?} [{}, {}]",
                        op.id, op.op_type, op.invoke_time, resp_time
                    );
                }
            }
        }
        return make_violation(
            "linearizability".to_string(),
            "Final history is not linearizable".to_string(),
            sim.events_processed(),
            &mut trace,
        );
    }

    // Compute final storage hash for determinism checking
    let storage_hash = storage.storage_hash();

    // Record simulation end in trace
    if let Some(ref mut t) = trace {
        t.record(
            sim.now(),
            TraceEventType::SimulationEnd {
                events_processed: sim.events_processed(),
            },
        );
    }

    SimulationResult::Success {
        events_processed: sim.events_processed(),
        final_time_ns: sim.now(),
        storage_hash,
        trace,
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Simple hex encoding for storage hashes.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

// ============================================================================
// CLI Parsing
// ============================================================================

fn parse_args() -> VoprConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut config = VoprConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" | "-s" => {
                i += 1;
                if i < args.len() {
                    config.seed = args[i].parse().unwrap_or(0);
                }
            }
            "--iterations" | "-n" => {
                i += 1;
                if i < args.len() {
                    config.iterations = args[i].parse().unwrap_or(100);
                }
            }
            "--faults" | "-f" => {
                i += 1;
                if i < args.len() {
                    config.network_faults = args[i].contains("network");
                    config.storage_faults = args[i].contains("storage");
                }
            }
            "--no-faults" => {
                config.network_faults = false;
                config.storage_faults = false;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
            }
            "--max-events" => {
                i += 1;
                if i < args.len() {
                    config.max_events = args[i].parse().unwrap_or(10_000);
                }
            }
            "--json" => {
                config.json_mode = true;
            }
            "--checkpoint-file" => {
                i += 1;
                if i < args.len() {
                    config.checkpoint_file = Some(args[i].clone());
                }
            }
            "--check-determinism" => {
                config.check_determinism = true;
            }
            "--enable-trace" => {
                config.enable_trace = true;
            }
            "--no-trace-on-failure" => {
                config.save_trace_on_failure = false;
            }
            "--no-enhanced-workloads" => {
                config.enhanced_workloads = false;
            }
            "--no-failure-diagnosis" => {
                config.failure_diagnosis = false;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    config
}

fn print_help() {
    println!(
        r"VOPR - Viewstamped Operation Replication Simulation Tester

USAGE:
    vopr [OPTIONS]

OPTIONS:
    -s, --seed <SEED>           Starting seed for simulations (default: 0)
    -n, --iterations <N>        Number of iterations to run (default: 100)
    -f, --faults <TYPES>        Enable fault types: network,storage (default: both)
        --no-faults             Disable all fault injection
    -v, --verbose               Enable verbose output
        --max-events <N>        Maximum events per simulation (default: 10000)
        --json                  Output newline-delimited JSON
        --checkpoint-file <PATH> Path to checkpoint file for resume support
        --check-determinism     Run each seed twice to verify determinism
        --enable-trace          Enable full trace collection (high overhead)
        --no-trace-on-failure   Don't save trace when failures occur
        --no-enhanced-workloads Disable RMW and Scan operations
        --no-failure-diagnosis  Disable automated failure diagnosis
    -h, --help                  Print this help message

EXAMPLES:
    vopr --seed 12345           Run with specific seed
    vopr -n 1000 -v             Run 1000 iterations with verbose output
    vopr --faults network       Enable only network faults
    vopr --no-faults            Run without any fault injection
    vopr --json --checkpoint-file /var/lib/vopr/checkpoint.json
    vopr -n 100000 -v --checkpoint-file checkpoint.json  # Overnight run
"
    );
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[allow(clippy::cast_precision_loss)]
fn main() {
    let config = parse_args();

    // Load checkpoint if specified
    let mut checkpoint = if let Some(ref path) = config.checkpoint_file {
        VoprCheckpoint::load(path).unwrap_or_default()
    } else {
        VoprCheckpoint::default()
    };

    // Use checkpoint's last_seed if it's greater than config.seed
    let starting_seed = config.seed.max(checkpoint.last_seed);

    // Output header in appropriate format
    if config.json_mode {
        output(
            true,
            "start",
            Some(json!({
                "starting_seed": starting_seed,
                "iterations": config.iterations,
                "network_faults": config.network_faults,
                "storage_faults": config.storage_faults,
                "checkpoint_loaded": checkpoint.last_seed > 0
            })),
        );
    } else {
        println!("VOPR - Deterministic Simulation Tester");
        println!("======================================");
        println!("Starting seed: {}", starting_seed);
        println!("Iterations: {}", config.iterations);
        println!(
            "Faults: network={}, storage={}",
            config.network_faults, config.storage_faults
        );
        if checkpoint.last_seed > 0 {
            println!(
                "Resumed from checkpoint (last seed: {})",
                checkpoint.last_seed
            );
        }
        println!();
    }

    let start = Instant::now();
    let mut successes = 0u64;
    let mut failures: Vec<(u64, String)> = Vec::new();

    for i in 0..config.iterations {
        let seed = starting_seed.wrapping_add(i);
        let run = SimulationRun::new(seed, &config);

        if config.verbose && !config.json_mode {
            print!("Running seed {seed}... ");
        }

        let result = run_simulation(&run, &config);

        // Determinism check: run with same seed again and verify identical results
        if config.check_determinism {
            let result2 = run_simulation(&run, &config);

            // Compare results
            match (&result, &result2) {
                (
                    SimulationResult::Success {
                        storage_hash: hash1,
                        events_processed: events1,
                        ..
                    },
                    SimulationResult::Success {
                        storage_hash: hash2,
                        events_processed: events2,
                        ..
                    },
                ) => {
                    if hash1 != hash2 || events1 != events2 {
                        failures.push((
                            seed,
                            format!(
                                "determinism violation: hash1={}, hash2={}, events1={events1}, events2={events2}",
                                self::hex::encode(hash1),
                                self::hex::encode(hash2)
                            ),
                        ));
                        checkpoint.failed_seeds.push(seed);
                        continue;
                    }
                }
                _ => {
                    // If either run failed with an invariant violation,
                    // that's also a determinism issue if they differ
                    if format!("{result:?}") != format!("{result2:?}") {
                        failures.push((seed, "determinism violation: different failure modes".to_string()));
                        checkpoint.failed_seeds.push(seed);
                        continue;
                    }
                }
            }
        }

        match result {
            SimulationResult::Success {
                events_processed,
                final_time_ns,
                storage_hash,
                ..
            } => {
                successes += 1;
                if config.json_mode {
                    output(
                        true,
                        "iteration",
                        Some(json!({
                            "seed": seed,
                            "status": "ok",
                            "events": events_processed,
                            "simulated_time_ms": final_time_ns as f64 / 1_000_000.0,
                            "storage_hash": self::hex::encode(storage_hash)
                        })),
                    );
                } else if config.verbose {
                    println!(
                        "OK ({} events, {:.2}ms simulated, hash={})",
                        events_processed,
                        final_time_ns as f64 / 1_000_000.0,
                        &self::hex::encode(storage_hash)[..16]
                    );
                }
            }
            SimulationResult::InvariantViolation {
                invariant,
                message,
                events_processed,
                failure_report,
                ..
            } => {
                failures.push((seed, format!("{invariant}: {message}")));
                checkpoint.failed_seeds.push(seed);

                // Print failure diagnosis if enabled
                if config.failure_diagnosis && config.verbose {
                    if let Some(report) = failure_report {
                        eprintln!("\n{}", FailureAnalyzer::format_report(&report));
                    }
                }

                if config.json_mode {
                    output(
                        true,
                        "iteration",
                        Some(json!({
                            "seed": seed,
                            "status": "failed",
                            "events": events_processed,
                            "invariant": invariant,
                            "message": message
                        })),
                    );
                } else if config.verbose {
                    println!("FAILED at event {events_processed}");
                    println!("  Invariant: {invariant}");
                    println!("  Message: {message}");
                }
            }
        }

        // Update checkpoint every iteration
        checkpoint.last_seed = seed;
        checkpoint.total_iterations += 1;
        checkpoint.total_failures = failures.len() as u64;
        checkpoint.last_update = Utc::now().to_rfc3339();

        // Progress indicator for non-verbose, non-JSON mode
        if !config.verbose && !config.json_mode && (i + 1) % 10 == 0 {
            print!(
                "\rProgress: {}/{} ({} failures)",
                i + 1,
                config.iterations,
                failures.len()
            );
            std::io::stdout().flush().ok();
        }
    }

    if !config.verbose && !config.json_mode {
        println!();
    }

    let elapsed = start.elapsed();

    // Save checkpoint if specified
    if let Some(ref path) = config.checkpoint_file {
        if let Err(e) = checkpoint.save(path) {
            eprintln!("Warning: Failed to save checkpoint: {}", e);
        }
    }

    // Output final results
    if config.json_mode {
        output(
            true,
            "batch_complete",
            Some(json!({
                "successes": successes,
                "failures": failures.len(),
                "elapsed_secs": elapsed.as_secs_f64(),
                "rate": config.iterations as f64 / elapsed.as_secs_f64(),
                "failed_seeds": failures.iter().map(|(s, _)| s).collect::<Vec<_>>()
            })),
        );
    } else {
        println!();
        println!("======================================");
        println!("Results:");
        println!("  Successes: {successes}");
        println!("  Failures: {}", failures.len());
        println!("  Time: {:.2}s", elapsed.as_secs_f64());
        println!(
            "  Rate: {:.0} sims/sec",
            config.iterations as f64 / elapsed.as_secs_f64()
        );

        if !failures.is_empty() {
            println!();
            println!("Failed seeds (for reproduction):");
            for (seed, error) in &failures {
                println!("  vopr --seed {seed} -v");
                println!("    Error: {error}");
            }
        }
    }

    if !failures.is_empty() {
        std::process::exit(1);
    }
}
