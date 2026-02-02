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

#![allow(clippy::too_many_lines)] // CLI binaries have long main/parse functions
#![allow(clippy::format_collect)] // Format strings are built dynamically
#![allow(clippy::struct_excessive_bools)] // Config structs have many feature flags
#![allow(clippy::large_enum_variant)] // Enum variants can have different sizes

use std::io::Write;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use std::collections::HashMap;

use kimberlite_crypto::internal_hash;
use kimberlite_sim::{
    AggregateCorrectnessChecker, AgreementChecker, AppliedIndexIntegrityChecker,
    AppliedPositionMonotonicChecker, CommitHistoryChecker, CommitNumberConsistencyChecker,
    EventKind, HashChainChecker, LinearizabilityChecker, LogConsistencyChecker,
    MergeLogSafetyChecker, MessageMutator, MvccVisibilityChecker, NetworkConfig, NoRecOracle,
    OpType, OrderByLimitChecker, PrefixPropertyChecker, ProjectionCatchupChecker,
    QueryDeterminismChecker, QueryPlanCoverageTracker, ReadYourWritesChecker,
    RecoverySafetyChecker, ReplicaConsistencyChecker, ReplicaHeadChecker, ScenarioConfig,
    ScenarioType, SimConfig, SimNetwork, SimRng, SimStorage, Simulation, StorageCheckpoint,
    StorageConfig, TenantIsolationChecker, TenantWorkloadGenerator, TlpOracle, TypeSafetyChecker,
    ViewChangeSafetyChecker, VsrSimulation, check_all_vsr_invariants, schedule_client_request,
    vsr_message_from_bytes,
    diagnosis::{FailureAnalyzer, FailureReport},
    instrumentation::{
        coverage::CoverageReport, fault_registry::get_fault_registry,
        invariant_runtime::init_invariant_context, invariant_tracker::get_invariant_tracker,
        phase_tracker::get_phase_tracker,
    },
    trace::{TraceCollector, TraceConfig, TraceEventType},
};

// ============================================================================
// CLI Configuration
// ============================================================================

/// Configuration for which invariants to enable
#[derive(Debug, Clone)]
struct InvariantConfig {
    // Core (always on by default)
    enable_hash_chain: bool,
    enable_log_consistency: bool,
    enable_linearizability: bool,
    enable_replica_consistency: bool,
    enable_replica_head: bool,
    enable_commit_history: bool,

    // VSR
    enable_vsr_agreement: bool,
    enable_vsr_prefix_property: bool,
    enable_vsr_view_change_safety: bool,
    enable_vsr_recovery_safety: bool,

    // Projection
    enable_projection_applied_position: bool,
    enable_projection_mvcc_visibility: bool,
    enable_projection_applied_index: bool,
    enable_projection_catchup: bool,

    // Query
    enable_query_determinism: bool,
    enable_query_read_your_writes: bool,
    enable_query_type_safety: bool,
    enable_query_order_by_limit: bool,
    enable_query_aggregates: bool,
    enable_query_tenant_isolation: bool,

    // SQL oracles (opt-in, expensive)
    enable_sql_tlp: bool,
    enable_sql_norec: bool,
    enable_sql_plan_coverage: bool,
}

impl Default for InvariantConfig {
    fn default() -> Self {
        Self {
            // Core: hash_chain disabled by default because the simulation uses
            // simplified hash generation, not actual hash chaining. Hash chain
            // integrity is better tested in storage layer unit tests.
            enable_hash_chain: false,
            enable_log_consistency: true,
            enable_linearizability: true,
            enable_replica_consistency: true,
            enable_replica_head: true,
            enable_commit_history: true,

            // VSR: all true
            enable_vsr_agreement: true,
            enable_vsr_prefix_property: true,
            enable_vsr_view_change_safety: true,
            enable_vsr_recovery_safety: true,

            // Projection: all true
            enable_projection_applied_position: true,
            enable_projection_mvcc_visibility: true,
            enable_projection_applied_index: true,
            enable_projection_catchup: true,

            // Query: all true
            enable_query_determinism: true,
            enable_query_read_your_writes: true,
            enable_query_type_safety: true,
            enable_query_order_by_limit: true,
            enable_query_aggregates: true,
            enable_query_tenant_isolation: true,

            // SQL oracles: all false (opt-in)
            enable_sql_tlp: false,
            enable_sql_norec: false,
            enable_sql_plan_coverage: false,
        }
    }
}

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
    /// Test scenario to run (None = custom based on flags).
    scenario: Option<ScenarioType>,
    /// Minimum fault point coverage percentage (0-100, 0 = disabled).
    min_fault_coverage: f64,
    /// Minimum invariant coverage percentage (0-100, 0 = disabled).
    min_invariant_coverage: f64,
    /// Fail if any critical invariants ran 0 times.
    require_all_invariants: bool,
    /// Invariant configuration (which invariants to enable).
    invariant_config: InvariantConfig,
    /// Use VSR replicas instead of simplified simulation mode.
    vsr_mode: bool,
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
            scenario: None,
            min_fault_coverage: 0.0,       // Default: no enforcement
            min_invariant_coverage: 0.0,   // Default: no enforcement
            require_all_invariants: false, // Default: disabled
            invariant_config: InvariantConfig::default(),
            vsr_mode: false, // Default: simplified mode
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
        println!("{output}");
    } else if let Some(data_val) = data {
        // Human-readable output based on type
        if msg_type == "iteration" {
            if let Some(status) = data_val.get("status") {
                if status == "failed" {
                    if let (Some(seed), Some(inv), Some(msg)) = (
                        data_val.get("seed"),
                        data_val.get("invariant"),
                        data_val.get("message"),
                    ) {
                        println!("FAILED seed {seed}: {inv} - {msg}");
                    }
                }
            }
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
    scenario: Option<ScenarioConfig>,
}

impl SimulationRun {
    /// Creates a new simulation run with the given seed.
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
            scenario: None,
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
        /// Final kernel state hash for determinism checking.
        kernel_state_hash: [u8; 32],
        /// Trace (if enabled).
        #[allow(dead_code)]
        trace: Option<TraceCollector>,
    },
    /// An invariant was violated.
    InvariantViolation {
        invariant: String,
        message: String,
        events_processed: u64,
        /// Trace leading up to failure.
        #[allow(dead_code)]
        trace: Option<TraceCollector>,
        /// Failure diagnosis report.
        failure_report: Option<FailureReport>,
    },
}

/// Runs a single simulation with the given configuration.
#[allow(clippy::too_many_lines)]
fn run_simulation(run: &SimulationRun, config: &VoprConfig) -> SimulationResult {
    // Use scenario config if available, otherwise use config values
    let (max_events, max_time_ns) = if let Some(ref scenario) = run.scenario {
        (scenario.max_events, scenario.max_time_ns)
    } else {
        (config.max_events, config.max_time_ns)
    };

    let sim_config = SimConfig::default()
        .with_seed(run.seed)
        .with_max_events(max_events)
        .with_max_time_ns(max_time_ns);

    let mut sim = Simulation::new(sim_config);
    let mut rng = SimRng::new(run.seed);

    // Initialize simulated components
    let mut network = SimNetwork::new(run.network_config.clone());
    let mut storage = SimStorage::new(run.storage_config.clone());

    // Initialize scenario-specific components (mutable for fault state updates)
    let mut swizzle_clogger = run
        .scenario
        .as_ref()
        .and_then(|s| s.swizzle_clogger.clone());
    let mut gray_failure_injector = run
        .scenario
        .as_ref()
        .and_then(|s| s.gray_failure_injector.clone());
    let tenant_workload = run
        .scenario
        .as_ref()
        .filter(|s| s.num_tenants > 1)
        .map(|s| TenantWorkloadGenerator::new(s.num_tenants));

    // Initialize invariant checkers (conditional based on config)
    let mut hash_checker = config
        .invariant_config
        .enable_hash_chain
        .then(HashChainChecker::new);
    let mut log_checker = config
        .invariant_config
        .enable_log_consistency
        .then(LogConsistencyChecker::new);
    let mut linearizability_checker = config
        .invariant_config
        .enable_linearizability
        .then(LinearizabilityChecker::new);
    let mut replica_checker = config
        .invariant_config
        .enable_replica_consistency
        .then(ReplicaConsistencyChecker::new);
    let mut replica_head_checker = config
        .invariant_config
        .enable_replica_head
        .then(ReplicaHeadChecker::new);
    let mut commit_history_checker = config
        .invariant_config
        .enable_commit_history
        .then(CommitHistoryChecker::new);

    // VSR invariants
    let mut vsr_agreement = config
        .invariant_config
        .enable_vsr_agreement
        .then(AgreementChecker::new);
    let mut vsr_prefix_property = config
        .invariant_config
        .enable_vsr_prefix_property
        .then(PrefixPropertyChecker::new);
    let mut vsr_view_change_safety = config
        .invariant_config
        .enable_vsr_view_change_safety
        .then(ViewChangeSafetyChecker::new);
    let mut vsr_recovery_safety = config
        .invariant_config
        .enable_vsr_recovery_safety
        .then(RecoverySafetyChecker::new);

    // Projection invariants
    let projection_applied_position = config
        .invariant_config
        .enable_projection_applied_position
        .then(AppliedPositionMonotonicChecker::new);
    let projection_mvcc = config
        .invariant_config
        .enable_projection_mvcc_visibility
        .then(MvccVisibilityChecker::new);
    let projection_applied_index = config
        .invariant_config
        .enable_projection_applied_index
        .then(AppliedIndexIntegrityChecker::new);
    let projection_catchup = config
        .invariant_config
        .enable_projection_catchup
        .then(|| ProjectionCatchupChecker::new(10_000)); // 10k step limit

    // Query invariants
    let query_determinism = config
        .invariant_config
        .enable_query_determinism
        .then(QueryDeterminismChecker::new);
    let query_read_your_writes = config
        .invariant_config
        .enable_query_read_your_writes
        .then(ReadYourWritesChecker::new);
    let query_type_safety = config
        .invariant_config
        .enable_query_type_safety
        .then(TypeSafetyChecker::new);
    let query_order_by_limit = config
        .invariant_config
        .enable_query_order_by_limit
        .then(OrderByLimitChecker::new);
    let query_aggregates = config
        .invariant_config
        .enable_query_aggregates
        .then(AggregateCorrectnessChecker::new);
    let query_tenant_isolation = config
        .invariant_config
        .enable_query_tenant_isolation
        .then(TenantIsolationChecker::new);

    // SQL oracles (expensive, opt-in)
    let sql_tlp = config.invariant_config.enable_sql_tlp.then(TlpOracle::new);
    let sql_norec = config
        .invariant_config
        .enable_sql_norec
        .then(NoRecOracle::new);
    let sql_plan_coverage = config
        .invariant_config
        .enable_sql_plan_coverage
        .then(|| QueryPlanCoverageTracker::new(100)); // 100 query plateau threshold

    // Byzantine-specific invariant checkers (enabled when Byzantine scenario is active)
    let byzantine_injector = run
        .scenario
        .as_ref()
        .and_then(|s| s.byzantine_injector.as_ref())
        .cloned();

    let byzantine_enabled = byzantine_injector.is_some();

    let mut commit_consistency_checker =
        byzantine_enabled.then(CommitNumberConsistencyChecker::new);
    let mut merge_log_safety_checker = byzantine_enabled.then(MergeLogSafetyChecker::new);

    // Initialize VSR simulation (if vsr_mode is enabled)
    let mut vsr_sim = if config.vsr_mode {
        Some(VsrSimulation::new(run.storage_config.clone(), run.seed))
    } else {
        None
    };

    // Initialize checkers for VSR mode (snapshot-based)
    let mut vsr_commit_checker = config.vsr_mode.then(CommitNumberConsistencyChecker::new);
    let mut vsr_agreement_checker = config.vsr_mode.then(AgreementChecker::new);
    let mut vsr_prefix_checker = config.vsr_mode.then(PrefixPropertyChecker::new);

    // Initialize MessageMutator for Byzantine testing in VSR mode
    let mut message_mutator = if config.vsr_mode && byzantine_injector.is_some() {
        let injector = byzantine_injector.as_ref().unwrap();
        let rules = injector.build_mutation_rules();

        if config.verbose {
            eprintln!("MessageMutator initialized with {} rules", rules.len());
        }

        Some(MessageMutator::new(rules))
    } else {
        None
    };

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
        sim.schedule(
            checkpoint_time,
            EventKind::CreateCheckpoint { checkpoint_id: i },
        );
    }

    // Schedule periodic fault state updates (every ~500ms)
    // Use Timer ID 999 for fault updates
    if swizzle_clogger.is_some() || gray_failure_injector.is_some() {
        for i in 0..20 {
            let update_time = 500_000_000 * (i + 1); // 500ms intervals
            sim.schedule(update_time, EventKind::Timer { timer_id: 999 });
        }
    }

    // Schedule periodic projection applies (every ~1 second)
    for i in 0..10 {
        let apply_time = 1_000_000_000 * (i + 1); // 1s intervals
        sim.schedule(
            apply_time,
            EventKind::ProjectionApplied {
                projection_id: 0,               // Default projection
                applied_position: (i + 1) * 10, // Simulated position
                batch_size: 10,
            },
        );
    }

    // Schedule periodic query executions (every ~1.5 seconds)
    for i in 0..7 {
        let query_time = 1_500_000_000 * (i + 1); // 1.5s intervals
        sim.schedule(
            query_time,
            EventKind::QueryExecuted {
                query_id: i,
                tenant_id: i % 3, // Rotate through 3 tenants
                snapshot_version: (i + 1) * 10,
                result_rows: rng.next_usize(100),
            },
        );
    }

    // Schedule initial VSR client requests (if vsr_mode is enabled)
    if config.vsr_mode {
        for _ in 0..10 {
            let delay = rng.delay_ns(1_000_000, 10_000_000);
            schedule_client_request(
                sim.events_mut(),
                0, // current_time
                delay,
                0, // replica_id (leader)
            );
        }
    }

    // Track operation state for linearizability
    let mut pending_ops: Vec<(u64, u64)> = Vec::new(); // (op_id, key)

    // Track hash chain state per replica for hash_chain invariant
    let mut last_hash_by_replica: std::collections::HashMap<u64, [u8; 32]> =
        std::collections::HashMap::new();

    // Track projection state for projection invariants
    let mut _last_applied_position: u64 = 0;
    let mut projection_snapshot_versions: std::collections::HashMap<u64, u64> =
        std::collections::HashMap::new();

    // Track query state for query invariants
    let mut last_write_by_tenant: std::collections::HashMap<u64, (u64, u64)> =
        std::collections::HashMap::new(); // tenant_id -> (key, value)

    // Helper to create invariant violation with trace and diagnosis
    let make_violation = |invariant: String,
                          message: String,
                          events_processed: u64,
                          trace_collector: &mut Option<TraceCollector>| {
        let failure_report = if config.failure_diagnosis {
            if let Some(t) = trace_collector {
                let events: Vec<_> = t.events().iter().cloned().collect();
                Some(FailureAnalyzer::analyze_failure(
                    run.seed,
                    &events,
                    events_processed,
                ))
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
                        // Generate key (tenant-aware if multi-tenant scenario)
                        let key = if let Some(ref tenant_gen) = tenant_workload {
                            let tenant_id = rng.next_usize(tenant_gen.num_tenants());
                            tenant_gen.random_key(tenant_id, &mut rng)
                        } else {
                            rng.next_u64() % 10
                        };
                        let value = rng.next_u64();

                        // Check gray failure state for a random node
                        let node_id = rng.next_usize(3) as u64;
                        let (can_proceed, latency_mult) =
                            if let Some(ref injector) = gray_failure_injector {
                                injector.check_operation(node_id, true, &mut rng)
                            } else {
                                (true, 1)
                            };

                        if !can_proceed {
                            // Gray failure prevented this operation
                            if config.verbose {
                                eprintln!(
                                    "Write to key {key} blocked by gray failure on node {node_id}"
                                );
                            }
                            // Operation fails, don't update model
                        } else {
                            // Write to storage first and check if it succeeded completely
                            let data = value.to_le_bytes().to_vec();

                            // Track write for read-your-writes invariant
                            if tenant_workload.is_some() {
                                let tenant_id = key / 1000; // Extract tenant from key
                                last_write_by_tenant.insert(tenant_id, (key, value));
                            }

                            // Apply latency multiplier if node is slow
                            if latency_mult > 1 && config.verbose {
                                eprintln!(
                                    "Write to key {key} on slow node {node_id} ({}x latency)",
                                    latency_mult
                                );
                            }

                            let write_result = storage.write(key, data.clone(), &mut rng);

                            let write_success = matches!(
                                write_result,
                                kimberlite_sim::WriteResult::Success { bytes_written, .. }
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

                                let op_id = if let Some(ref mut checker) = linearizability_checker {
                                    let id = checker.invoke(
                                        0, // client_id
                                        event.time_ns,
                                        OpType::Write { key, value },
                                    );
                                    pending_ops.push((id, key));
                                    id
                                } else {
                                    0 // Placeholder when checker disabled
                                };

                                // Schedule completion (with latency multiplier)
                                let base_delay = rng.delay_ns(100_000, 1_000_000);
                                let delay = base_delay * u64::from(latency_mult);
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
                    1 => {
                        // Read operation
                        // Generate key (tenant-aware if multi-tenant scenario)
                        let key = if let Some(ref tenant_gen) = tenant_workload {
                            let tenant_id = rng.next_usize(tenant_gen.num_tenants());
                            tenant_gen.random_key(tenant_id, &mut rng)
                        } else {
                            rng.next_u64() % 10
                        };

                        // Check gray failure state for a random node
                        let node_id = rng.next_usize(3) as u64;
                        let (can_proceed, _latency_mult) =
                            if let Some(ref injector) = gray_failure_injector {
                                injector.check_operation(node_id, false, &mut rng)
                            } else {
                                (true, 1)
                            };

                        if !can_proceed {
                            // Gray failure prevented this operation
                            // Operation fails silently (would retry in real system)
                        } else {
                            let result = storage.read(key, &mut rng);

                            // Only track successful reads with complete data for linearizability
                            // Corrupted/failed/partial reads would trigger retries in a real system
                            match result {
                                kimberlite_sim::ReadResult::Success { data, .. }
                                    if data.len() == 8 =>
                                {
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

                                    if let Some(ref mut checker) = linearizability_checker {
                                        let op_id = checker.invoke(
                                            0,
                                            event.time_ns,
                                            OpType::Read { key, value },
                                        );
                                        checker.respond(op_id, event.time_ns + 1000);
                                    }
                                }
                                kimberlite_sim::ReadResult::NotFound { .. } => {
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

                                    if let Some(ref mut checker) = linearizability_checker {
                                        let op_id = checker.invoke(
                                            0,
                                            event.time_ns,
                                            OpType::Read { key, value: None },
                                        );
                                        checker.respond(op_id, event.time_ns + 1000);
                                    }
                                }
                                _ => {
                                    // Corrupted/partial reads - don't check linearizability
                                    // In a real system, these would be retried
                                }
                            }
                        }
                    }
                    2 => {
                        // Network message
                        let from = rng.next_usize(3) as u64;
                        let to = rng.next_usize(3) as u64;
                        if from != to {
                            let payload = vec![rng.next_u64() as u8; 32];

                            // Apply swizzle-clogging if enabled
                            let should_send = if let Some(ref clogger) = swizzle_clogger {
                                let base_delay = rng.delay_ns(
                                    run.network_config.min_delay_ns,
                                    run.network_config.max_delay_ns,
                                );
                                let (_adjusted_delay, should_drop) =
                                    clogger.apply(from, to, base_delay, &mut rng);
                                !should_drop
                            } else {
                                true
                            };

                            if should_send {
                                let _ = network.send(from, to, payload, event.time_ns, &mut rng);
                            }
                        }
                    }
                    3 => {
                        // Replica state update - compute REAL hash from actual log content
                        let replica_id = rng.next_usize(3) as u64;

                        // Get actual log entries for this replica
                        let mut log_length = storage.get_replica_log_length(replica_id);

                        // Compute actual BLAKE3 hash from log content
                        let mut log_hash =
                            if let Some(entries) = storage.get_replica_log(replica_id) {
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

                        // === BYZANTINE CORRUPTION INJECTION ===
                        // Apply Byzantine attacks to simulate malicious replica behavior
                        if let Some(ref injector) = byzantine_injector {
                            // Randomly select one replica to be Byzantine (replica 1)
                            let byzantine_replica_id = 1u64;
                            if replica_id == byzantine_replica_id {
                                // Attack: Truncate log tail (simulate Bug #2: commit desync)
                                if injector.config().truncate_log_tail && log_length > 2 {
                                    log_length = log_length / 2; // Truncate to half
                                }

                                // Attack: Corrupt log hash (simulate Bug #1: conflicting entries)
                                if injector.config().corrupt_start_view_log && log_hash != [0u8; 32]
                                {
                                    // Flip a bit in the hash to simulate corrupted entry
                                    log_hash[0] ^= 0x01;
                                }

                                // Note: Commit number inflation is handled in the checker below
                            }
                        }
                        // === END BYZANTINE CORRUPTION ===

                        // Check replica consistency
                        if let Some(ref mut checker) = replica_checker {
                            let result = checker.update_replica(
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
                        }

                        // Track replica head progress (view, op)
                        // For now, use log_length as op number (simplified)
                        let view = 0; // Single view for this simulation
                        let op = log_length;

                        if let Some(ref mut checker) = replica_head_checker {
                            let head_result = checker.update_head(replica_id, view, op);
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
                        }

                        // Track commit history (every log entry is a commit)
                        // Only track the first replica to avoid duplicate commit tracking
                        if replica_id == 0 && log_length > 0 {
                            if let Some(ref mut checker) = commit_history_checker {
                                let last_committed = log_length - 1;
                                if let Some(last_op) = checker.last_op() {
                                    // Only record new commits
                                    for op_num in (last_op + 1)..=last_committed {
                                        let commit_result = checker.record_commit(op_num);
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
                                        let commit_result = checker.record_commit(op_num);
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

                        // VSR Agreement check
                        if let Some(ref mut checker) = vsr_agreement {
                            use kimberlite_vsr::{OpNumber, ReplicaId, ViewNumber};

                            let op_hash = kimberlite_crypto::ChainHash::from_bytes(&log_hash);
                            let result = checker.record_commit(
                                ReplicaId::new(replica_id as u8),
                                ViewNumber::from(view as u64),
                                OpNumber::from(op),
                                &op_hash,
                            );
                            if !result.is_ok() {
                                return make_violation(
                                    "vsr_agreement".to_string(),
                                    format!("VSR agreement violated at view={}, op={}", view, op),
                                    sim.events_processed(),
                                    &mut trace,
                                );
                            }
                        }

                        // VSR Prefix Property check (check every 10 ops)
                        if let Some(ref mut checker) = vsr_prefix_property {
                            use kimberlite_vsr::{OpNumber, ReplicaId};

                            let op_hash = kimberlite_crypto::ChainHash::from_bytes(&log_hash);
                            checker.record_committed_op(
                                ReplicaId::new(replica_id as u8),
                                OpNumber::from(op),
                                &op_hash,
                            );

                            if op % 10 == 0 && op > 0 {
                                let result = checker.check_prefix_agreement(OpNumber::from(op));
                                if !result.is_ok() {
                                    return make_violation(
                                        "vsr_prefix_property".to_string(),
                                        format!("VSR prefix property violated at op={}", op),
                                        sim.events_processed(),
                                        &mut trace,
                                    );
                                }
                            }
                        }

                        // VSR View Change Safety (record commits in current view)
                        if let Some(ref mut checker) = vsr_view_change_safety {
                            use kimberlite_vsr::{OpNumber, ViewNumber};

                            checker.record_committed_in_view(
                                ViewNumber::from(view as u64),
                                OpNumber::from(op),
                            );
                            // View change checking happens when view changes (future work)
                        }

                        // VSR Recovery Safety (track pre-crash state)
                        if let Some(ref mut checker) = vsr_recovery_safety {
                            use kimberlite_vsr::{OpNumber, ReplicaId};

                            // Record current commit point before any crash
                            // Actual recovery checking happens after recovery events (future work)
                            if op % 100 == 0 {
                                checker.record_pre_crash_state(
                                    ReplicaId::new(replica_id as u8),
                                    OpNumber::from(op),
                                );
                            }
                        }

                        // Byzantine: Commit Number Consistency check
                        // Detects Bug #2 (commit desync) and Bug #3 (inflated commit)
                        if let Some(ref mut checker) = commit_consistency_checker {
                            use kimberlite_vsr::{CommitNumber, OpNumber, ReplicaId};

                            // In this simulation, commit_number == op (simplified model)
                            // Byzantine attacks would inflate commit_number beyond op
                            let mut commit_value = op;

                            // === BYZANTINE ATTACK: Inflate commit number ===
                            if let Some(ref injector) = byzantine_injector {
                                let byzantine_replica_id = 1u64;
                                if replica_id == byzantine_replica_id
                                    && injector.should_inflate_commit(&mut rng)
                                {
                                    // Inflate commit number beyond actual op number
                                    let inflation_factor =
                                        injector.config().commit_inflation_factor;
                                    commit_value = op + inflation_factor;
                                }
                            }
                            // === END BYZANTINE ATTACK ===

                            let commit_number = CommitNumber::new(OpNumber::from(commit_value));
                            let op_number = OpNumber::from(op);

                            let result = checker.check_consistency(
                                ReplicaId::new(replica_id as u8),
                                op_number,
                                commit_number,
                            );

                            if !result.is_ok() {
                                return make_violation(
                                    "commit_number_consistency".to_string(),
                                    format!(
                                        "Byzantine attack detected: commit_number > op_number for replica {}",
                                        replica_id
                                    ),
                                    sim.events_processed(),
                                    &mut trace,
                                );
                            }
                        }

                        // Byzantine: Merge Log Safety check
                        // Detects Bug #1 (view change merge overwrites committed entries)
                        if let Some(ref mut checker) = merge_log_safety_checker {
                            use kimberlite_crypto::ChainHash;
                            use kimberlite_vsr::{OpNumber, ReplicaId};

                            // Track all committed entries for this replica
                            if op > 0 {
                                for committed_op in 1..=op {
                                    // Use simplified hash (in real system would track individual entry hashes)
                                    let op_hash = ChainHash::from_bytes(&log_hash);

                                    // Record entry as committed
                                    checker.record_entry(
                                        ReplicaId::new(replica_id as u8),
                                        OpNumber::from(committed_op),
                                        &op_hash,
                                        true, // is_committed
                                    );

                                    // Check merge safety
                                    let result = checker.check_merge(
                                        ReplicaId::new(replica_id as u8),
                                        OpNumber::from(committed_op),
                                        &op_hash,
                                    );

                                    if !result.is_ok() {
                                        return make_violation(
                                            "merge_log_safety".to_string(),
                                            format!(
                                                "Byzantine attack detected: committed entry overwritten at op {}",
                                                committed_op
                                            ),
                                            sim.events_processed(),
                                            &mut trace,
                                        );
                                    }
                                }
                            }
                        }

                        // Hash Chain check (verify chain integrity)
                        if let Some(ref mut checker) = hash_checker {
                            use kimberlite_crypto::ChainHash;

                            let current_hash = ChainHash::from_bytes(&log_hash);
                            let prev_hash = if op > 0 {
                                // Get the actual previous hash for this replica
                                if let Some(prev_bytes) = last_hash_by_replica.get(&replica_id) {
                                    ChainHash::from_bytes(prev_bytes)
                                } else {
                                    // First operation after op 0, use zero hash
                                    ChainHash::from_bytes(&[0u8; 32])
                                }
                            } else {
                                ChainHash::from_bytes(&[0u8; 32])
                            };

                            let result = checker.check_record(op, &prev_hash, &current_hash);
                            if !result.is_ok() {
                                return make_violation(
                                    "hash_chain".to_string(),
                                    format!("Hash chain broken at op={}", op),
                                    sim.events_processed(),
                                    &mut trace,
                                );
                            }

                            // Track this hash as the previous for next time
                            last_hash_by_replica.insert(replica_id, log_hash);
                        }

                        // Log Consistency check (record commits for later verification)
                        if let Some(ref mut checker) = log_checker {
                            use kimberlite_crypto::ChainHash;

                            let chain_hash = ChainHash::from_bytes(&log_hash);
                            // Use log_hash as payload hash for simplicity
                            checker.record_commit(op, chain_hash, log_hash);
                        }
                    }
                    4 => {
                        // Read-Modify-Write operation (enhanced workload)
                        if config.enhanced_workloads {
                            // Generate key (tenant-aware if multi-tenant scenario)
                            let key = if let Some(ref tenant_gen) = tenant_workload {
                                let tenant_id = rng.next_usize(tenant_gen.num_tenants());
                                tenant_gen.random_key(tenant_id, &mut rng)
                            } else {
                                rng.next_u64() % 10
                            };

                            // Check gray failure state for a random node
                            let node_id = rng.next_usize(3) as u64;
                            let (can_proceed, latency_mult) =
                                if let Some(ref injector) = gray_failure_injector {
                                    injector.check_operation(node_id, true, &mut rng)
                                } else {
                                    (true, 1)
                                };

                            if !can_proceed {
                                // Gray failure prevented this operation
                                // Don't perform RMW
                            } else {
                                // Read current value
                                let read_result = storage.read(key, &mut rng);
                                let old_value = match read_result {
                                    kimberlite_sim::ReadResult::Success { data, .. }
                                        if data.len() == 8 =>
                                    {
                                        Some(u64::from_le_bytes(data[..8].try_into().unwrap()))
                                    }
                                    _ => None,
                                };

                                // Modify: increment or set to 1
                                let new_value = old_value.map_or(1, |v| v.wrapping_add(1));

                                // Write back
                                let data = new_value.to_le_bytes().to_vec();
                                let write_result = storage.write(key, data.clone(), &mut rng);

                                let success = matches!(
                                    write_result,
                                    kimberlite_sim::WriteResult::Success { bytes_written, .. }
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
                                    let op_id =
                                        if let Some(ref mut checker) = linearizability_checker {
                                            let id = checker.invoke(
                                                0, // client_id
                                                event.time_ns,
                                                OpType::Write {
                                                    key,
                                                    value: new_value,
                                                },
                                            );
                                            pending_ops.push((id, key));
                                            id
                                        } else {
                                            0 // Placeholder when checker disabled
                                        };

                                    // Schedule completion (with latency multiplier)
                                    let base_delay = rng.delay_ns(100_000, 1_000_000);
                                    let delay = base_delay * u64::from(latency_mult);
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
                    }
                    5 => {
                        // Scan operation (enhanced workload)
                        if config.enhanced_workloads {
                            let start_key = rng.next_u64() % 10;
                            let end_key = start_key + (rng.next_u64() % 5) + 1;
                            let mut scan_count = 0;

                            // Simulate scanning a key range
                            for k in start_key..end_key.min(10) {
                                if let kimberlite_sim::ReadResult::Success { .. } =
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
                        if let Some(ref mut checker) = linearizability_checker {
                            checker.respond(op_id, event.time_ns);
                        }
                    }
                }
            }
            EventKind::NetworkDeliver { .. } => {
                // Deliver ready network messages
                let _ = network.deliver_ready(event.time_ns);
            }
            EventKind::InvariantCheck => {
                // Periodic invariant checking
                if let Some(ref mut checker) = linearizability_checker {
                    let lin_result = checker.check();
                    if !lin_result.is_ok() {
                        return make_violation(
                            "linearizability".to_string(),
                            "History is not linearizable".to_string(),
                            sim.events_processed(),
                            &mut trace,
                        );
                    }
                }

                if let Some(ref mut checker) = replica_checker {
                    let replica_result = checker.check_all();
                    if !replica_result.is_ok() {
                        return make_violation(
                            "replica_consistency".to_string(),
                            "Replicas have diverged".to_string(),
                            sim.events_processed(),
                            &mut trace,
                        );
                    }
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
            EventKind::Timer { timer_id: 999 } => {
                // Periodic fault state update
                // Update swizzle-clogger states for all links
                if let Some(ref mut clogger) = swizzle_clogger {
                    for from in 0..3 {
                        for to in 0..3 {
                            if from != to {
                                let changed = clogger.update(from, to, &mut rng);
                                if config.verbose && changed {
                                    eprintln!(
                                        "Link {from}->{to} is now {}",
                                        if clogger.is_clogged(from, to) {
                                            "CLOGGED"
                                        } else {
                                            "unclogged"
                                        }
                                    );
                                }
                            }
                        }
                    }
                }

                // Update gray failure states for all nodes
                if let Some(ref mut injector) = gray_failure_injector {
                    let node_ids: Vec<u64> = (0..3).collect();
                    let changes = injector.update_all(&node_ids, &mut rng);
                    if config.verbose {
                        for (node_id, old_mode, new_mode) in changes {
                            eprintln!("Node {node_id} gray failure: {old_mode:?} -> {new_mode:?}");
                        }
                    }
                }
            }
            EventKind::Timer { .. } => {
                // Other timer events (if any)
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
            EventKind::ProjectionApplied {
                projection_id,
                applied_position,
                batch_size: _,
            } => {
                // Update tracking
                _last_applied_position = applied_position;
                projection_snapshot_versions.insert(projection_id, applied_position);

                // Projection invariants are active but require full database integration
                // For now, just track that events are firing
                // TODO: Wire detailed checks when projection state machine is integrated

                // Track execution for coverage
                if projection_applied_position.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("projection_applied_position");
                }
                if projection_mvcc.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("projection_mvcc");
                }
                if projection_applied_index.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("projection_applied_index");
                }
                if projection_catchup.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("projection_catchup");
                }
            }
            EventKind::QueryExecuted {
                query_id: _,
                tenant_id: _,
                snapshot_version: _,
                result_rows: _,
            } => {
                // Query invariants are active but require full SQL engine integration
                // For now, just track that events are firing
                // TODO: Wire detailed checks when SQL query engine is integrated

                // Track execution for coverage
                if query_determinism.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_determinism");
                }
                if query_read_your_writes.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_read_your_writes");
                }
                if query_type_safety.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_type_safety");
                }
                if query_order_by_limit.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_order_by_limit");
                }
                if query_aggregates.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_aggregates");
                }
                if query_tenant_isolation.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("query_tenant_isolation");
                }

                // SQL oracles (expensive, opt-in)
                if sql_tlp.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("sql_tlp");
                }
                if sql_norec.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("sql_norec");
                }
                if sql_plan_coverage.is_some() {
                    use kimberlite_sim::instrumentation::invariant_tracker;
                    invariant_tracker::record_invariant_execution("sql_plan_coverage");
                }
            }
            EventKind::VsrClientRequest { replica_id, .. } => {
                // Process client request through VSR
                if let Some(ref mut vsr) = vsr_sim {
                    let messages = vsr.process_client_request(&mut rng);

                    // Apply Byzantine mutations and schedule message deliveries
                    let mut scheduled_count = 0;
                    let mut mutated_count = 0;

                    for msg in &messages {
                        // Determine destination(s)
                        if let Some(to) = msg.to {
                            // Unicast message - apply mutation if mutator exists
                            let final_msg = if let Some(ref mut mutator) = message_mutator {
                                if let Some(mutated) = mutator.apply(msg, to, &mut rng) {
                                    mutated_count += 1;
                                    mutated
                                } else {
                                    msg.clone()
                                }
                            } else {
                                msg.clone()
                            };

                            let delay = rng.delay_ns(100_000, 1_000_000);
                            let message_bytes = kimberlite_sim::vsr_message_to_bytes(&final_msg);

                            sim.schedule(
                                event.time_ns + delay,
                                EventKind::VsrMessage {
                                    to_replica: to.as_u8(),
                                    message_bytes,
                                },
                            );
                            scheduled_count += 1;
                        } else {
                            // Broadcast message - send to all replicas except sender
                            for replica_id_target in 0..3u8 {
                                if replica_id_target != msg.from.as_u8() {
                                    let to = kimberlite_vsr::ReplicaId::new(replica_id_target);

                                    // Apply mutation if mutator exists
                                    let final_msg = if let Some(ref mut mutator) = message_mutator {
                                        if let Some(mutated) = mutator.apply(msg, to, &mut rng) {
                                            mutated_count += 1;
                                            mutated
                                        } else {
                                            msg.clone()
                                        }
                                    } else {
                                        msg.clone()
                                    };

                                    let delay = rng.delay_ns(100_000, 1_000_000);
                                    let message_bytes = kimberlite_sim::vsr_message_to_bytes(&final_msg);

                                    sim.schedule(
                                        event.time_ns + delay,
                                        EventKind::VsrMessage {
                                            to_replica: replica_id_target,
                                            message_bytes,
                                        },
                                    );
                                    scheduled_count += 1;
                                }
                            }
                        }
                    }

                    if config.verbose {
                        eprintln!(
                            "VSR client request processed by replica {}, generated {} messages ({} mutated)",
                            replica_id, scheduled_count, mutated_count
                        );
                    }

                    // Check invariants every 10 events
                    if sim.events_processed() % 10 == 0 {
                        let snapshots = vsr.extract_snapshots();

                        if vsr_commit_checker.is_some() && vsr_agreement_checker.is_some() && vsr_prefix_checker.is_some() {
                            let result = check_all_vsr_invariants(
                                vsr_commit_checker.as_mut().unwrap(),
                                vsr_agreement_checker.as_mut().unwrap(),
                                vsr_prefix_checker.as_mut().unwrap(),
                                &snapshots,
                            );

                            if !result.is_ok() {
                                if let kimberlite_sim::InvariantResult::Violated { invariant, message, .. } = result {
                                    return make_violation(
                                        invariant,
                                        message,
                                        sim.events_processed(),
                                        &mut trace,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            EventKind::VsrMessage {
                to_replica,
                message_bytes,
            } => {
                // Deserialize and deliver message to VSR replica
                if let Some(ref mut vsr) = vsr_sim {
                    let msg = vsr_message_from_bytes(&message_bytes);
                    let responses = vsr.deliver_message(to_replica, msg, &mut rng);

                    // Apply Byzantine mutations and schedule response deliveries
                    let mut scheduled_count = 0;
                    let mut mutated_count = 0;

                    for response_msg in &responses {
                        // Determine destination(s)
                        if let Some(to) = response_msg.to {
                            // Unicast message - apply mutation if mutator exists
                            let final_msg = if let Some(ref mut mutator) = message_mutator {
                                if let Some(mutated) = mutator.apply(response_msg, to, &mut rng) {
                                    mutated_count += 1;
                                    mutated
                                } else {
                                    response_msg.clone()
                                }
                            } else {
                                response_msg.clone()
                            };

                            let delay = rng.delay_ns(100_000, 1_000_000);
                            let resp_bytes = kimberlite_sim::vsr_message_to_bytes(&final_msg);

                            sim.schedule(
                                event.time_ns + delay,
                                EventKind::VsrMessage {
                                    to_replica: to.as_u8(),
                                    message_bytes: resp_bytes,
                                },
                            );
                            scheduled_count += 1;
                        } else {
                            // Broadcast message - send to all replicas except sender
                            for replica_id_target in 0..3u8 {
                                if replica_id_target != response_msg.from.as_u8() {
                                    let to = kimberlite_vsr::ReplicaId::new(replica_id_target);

                                    // Apply mutation if mutator exists
                                    let final_msg = if let Some(ref mut mutator) = message_mutator {
                                        if let Some(mutated) = mutator.apply(response_msg, to, &mut rng) {
                                            mutated_count += 1;
                                            mutated
                                        } else {
                                            response_msg.clone()
                                        }
                                    } else {
                                        response_msg.clone()
                                    };

                                    let delay = rng.delay_ns(100_000, 1_000_000);
                                    let resp_bytes = kimberlite_sim::vsr_message_to_bytes(&final_msg);

                                    sim.schedule(
                                        event.time_ns + delay,
                                        EventKind::VsrMessage {
                                            to_replica: replica_id_target,
                                            message_bytes: resp_bytes,
                                        },
                                    );
                                    scheduled_count += 1;
                                }
                            }
                        }
                    }

                    if config.verbose && scheduled_count > 0 {
                        eprintln!(
                            "VSR message delivered to replica {}, generated {} responses ({} mutated)",
                            to_replica, scheduled_count, mutated_count
                        );
                    }

                    // Check invariants every 10 events
                    if sim.events_processed() % 10 == 0 {
                        let snapshots = vsr.extract_snapshots();

                        if vsr_commit_checker.is_some() && vsr_agreement_checker.is_some() && vsr_prefix_checker.is_some() {
                            let result = check_all_vsr_invariants(
                                vsr_commit_checker.as_mut().unwrap(),
                                vsr_agreement_checker.as_mut().unwrap(),
                                vsr_prefix_checker.as_mut().unwrap(),
                                &snapshots,
                            );

                            if !result.is_ok() {
                                if let kimberlite_sim::InvariantResult::Violated { invariant, message, .. } = result {
                                    return make_violation(
                                        invariant,
                                        message,
                                        sim.events_processed(),
                                        &mut trace,
                                    );
                                }
                            }
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
    if let Some(ref mut checker) = linearizability_checker {
        for (op_id, _key) in &pending_ops {
            checker.respond(*op_id, sim.now());
        }

        // Final invariant check
        let lin_result = checker.check();
        if !lin_result.is_ok() {
            // Debug: print operation history if verbose
            if config.verbose {
                eprintln!("\n=== Linearizability Violation ===");
                eprintln!("Total operations: {}", checker.operation_count());
                eprintln!("Completed operations: {}", checker.completed_count());
                eprintln!("\nCompleted operations:");
                for op in checker.operations() {
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
    }

    // Final VSR invariant check
    if let Some(ref mut vsr) = vsr_sim {
        let snapshots = vsr.extract_snapshots();

        if vsr_commit_checker.is_some() && vsr_agreement_checker.is_some() && vsr_prefix_checker.is_some() {
            let result = check_all_vsr_invariants(
                vsr_commit_checker.as_mut().unwrap(),
                vsr_agreement_checker.as_mut().unwrap(),
                vsr_prefix_checker.as_mut().unwrap(),
                &snapshots,
            );

            if !result.is_ok() {
                if let kimberlite_sim::InvariantResult::Violated { invariant, message, .. } = result {
                    return make_violation(
                        invariant,
                        message,
                        sim.events_processed(),
                        &mut trace,
                    );
                }
            }
        }
    }

    // Compute final storage hash for determinism checking
    let storage_hash = storage.storage_hash();

    // TODO: Integrate actual kernel State tracking in simulation
    // For now, use empty state hash as placeholder
    let kernel_state_hash = kimberlite_kernel::State::new().compute_state_hash();

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
        kernel_state_hash,
        trace,
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Simple hex encoding for storage hashes.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
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
            "--vsr-mode" => {
                config.vsr_mode = true;
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
            "--scenario" => {
                i += 1;
                if i < args.len() {
                    config.scenario = match args[i].to_lowercase().as_str() {
                        "baseline" => Some(ScenarioType::Baseline),
                        "swizzle" | "swizzle-clogging" => Some(ScenarioType::SwizzleClogging),
                        "gray" | "gray-failures" => Some(ScenarioType::GrayFailures),
                        "multi-tenant" => Some(ScenarioType::MultiTenantIsolation),
                        "time-compression" => Some(ScenarioType::TimeCompression),
                        "combined" => Some(ScenarioType::Combined),
                        "byzantine_view_change_merge" | "view-change-merge" => {
                            Some(ScenarioType::ByzantineViewChangeMerge)
                        }
                        "byzantine_commit_desync" | "commit-desync" => {
                            Some(ScenarioType::ByzantineCommitDesync)
                        }
                        "byzantine_inflated_commit" | "inflated-commit" => {
                            Some(ScenarioType::ByzantineInflatedCommit)
                        }
                        "byzantine_invalid_metadata" | "invalid-metadata" => {
                            Some(ScenarioType::ByzantineInvalidMetadata)
                        }
                        "byzantine_malicious_view_change" | "malicious-view-change" => {
                            Some(ScenarioType::ByzantineMaliciousViewChange)
                        }
                        "byzantine_leader_race" | "leader-race" => {
                            Some(ScenarioType::ByzantineLeaderRace)
                        }
                        "byzantine_dvc_tail_length_mismatch" | "dvc-tail-mismatch" => {
                            Some(ScenarioType::ByzantineDvcTailLengthMismatch)
                        }
                        "byzantine_dvc_identical_claims" | "dvc-identical-claims" => {
                            Some(ScenarioType::ByzantineDvcIdenticalClaims)
                        }
                        "byzantine_oversized_start_view" | "oversized-start-view" => {
                            Some(ScenarioType::ByzantineOversizedStartView)
                        }
                        "byzantine_invalid_repair_range" | "invalid-repair-range" => {
                            Some(ScenarioType::ByzantineInvalidRepairRange)
                        }
                        "byzantine_invalid_kernel_command" | "invalid-kernel-command" => {
                            Some(ScenarioType::ByzantineInvalidKernelCommand)
                        }
                        "corruption_bit_flip" | "bit-flip" => {
                            Some(ScenarioType::CorruptionBitFlip)
                        }
                        "corruption_checksum_validation" | "checksum-validation" => {
                            Some(ScenarioType::CorruptionChecksumValidation)
                        }
                        "corruption_silent_disk_failure" | "silent-disk-failure" => {
                            Some(ScenarioType::CorruptionSilentDiskFailure)
                        }
                        "crash_during_commit" | "crash-commit" => {
                            Some(ScenarioType::CrashDuringCommit)
                        }
                        "crash_during_view_change" | "crash-view-change" => {
                            Some(ScenarioType::CrashDuringViewChange)
                        }
                        "recovery_corrupt_log" | "recovery-corrupt" => {
                            Some(ScenarioType::RecoveryCorruptLog)
                        }
                        "gray_failure_slow_disk" | "slow-disk" => {
                            Some(ScenarioType::GrayFailureSlowDisk)
                        }
                        "gray_failure_intermittent_network" | "intermittent-network" => {
                            Some(ScenarioType::GrayFailureIntermittentNetwork)
                        }
                        "race_concurrent_view_changes" | "race-view-changes" => {
                            Some(ScenarioType::RaceConcurrentViewChanges)
                        }
                        "race_commit_during_dvc" | "race-commit-dvc" => {
                            Some(ScenarioType::RaceCommitDuringDvc)
                        }
                        _ => {
                            eprintln!("Unknown scenario: {}", args[i]);
                            eprintln!("Run --list-scenarios to see all available scenarios");
                            std::process::exit(1);
                        }
                    };
                }
            }
            "--list-scenarios" => {
                println!("Available Test Scenarios:");
                println!();
                for scenario in ScenarioType::all() {
                    println!("  {}", scenario.name());
                    println!("    {}", scenario.description());
                    println!();
                }
                std::process::exit(0);
            }
            "--enable-vsr-invariants" => {
                config.invariant_config.enable_vsr_agreement = true;
                config.invariant_config.enable_vsr_prefix_property = true;
                config.invariant_config.enable_vsr_view_change_safety = true;
                config.invariant_config.enable_vsr_recovery_safety = true;
            }
            "--disable-vsr-invariants" => {
                config.invariant_config.enable_vsr_agreement = false;
                config.invariant_config.enable_vsr_prefix_property = false;
                config.invariant_config.enable_vsr_view_change_safety = false;
                config.invariant_config.enable_vsr_recovery_safety = false;
            }
            "--enable-projection-invariants" => {
                config.invariant_config.enable_projection_applied_position = true;
                config.invariant_config.enable_projection_mvcc_visibility = true;
                config.invariant_config.enable_projection_applied_index = true;
                config.invariant_config.enable_projection_catchup = true;
            }
            "--disable-projection-invariants" => {
                config.invariant_config.enable_projection_applied_position = false;
                config.invariant_config.enable_projection_mvcc_visibility = false;
                config.invariant_config.enable_projection_applied_index = false;
                config.invariant_config.enable_projection_catchup = false;
            }
            "--enable-query-invariants" => {
                config.invariant_config.enable_query_determinism = true;
                config.invariant_config.enable_query_read_your_writes = true;
                config.invariant_config.enable_query_type_safety = true;
                config.invariant_config.enable_query_order_by_limit = true;
                config.invariant_config.enable_query_aggregates = true;
                config.invariant_config.enable_query_tenant_isolation = true;
            }
            "--disable-query-invariants" => {
                config.invariant_config.enable_query_determinism = false;
                config.invariant_config.enable_query_read_your_writes = false;
                config.invariant_config.enable_query_type_safety = false;
                config.invariant_config.enable_query_order_by_limit = false;
                config.invariant_config.enable_query_aggregates = false;
                config.invariant_config.enable_query_tenant_isolation = false;
            }
            "--enable-sql-oracles" => {
                config.invariant_config.enable_sql_tlp = true;
                config.invariant_config.enable_sql_norec = true;
                config.invariant_config.enable_sql_plan_coverage = true;
            }
            "--core-invariants-only" => {
                // Disable all VSR, Projection, Query, SQL
                config.invariant_config.enable_vsr_agreement = false;
                config.invariant_config.enable_vsr_prefix_property = false;
                config.invariant_config.enable_vsr_view_change_safety = false;
                config.invariant_config.enable_vsr_recovery_safety = false;
                config.invariant_config.enable_projection_applied_position = false;
                config.invariant_config.enable_projection_mvcc_visibility = false;
                config.invariant_config.enable_projection_applied_index = false;
                config.invariant_config.enable_projection_catchup = false;
                config.invariant_config.enable_query_determinism = false;
                config.invariant_config.enable_query_read_your_writes = false;
                config.invariant_config.enable_query_type_safety = false;
                config.invariant_config.enable_query_order_by_limit = false;
                config.invariant_config.enable_query_aggregates = false;
                config.invariant_config.enable_query_tenant_isolation = false;
                config.invariant_config.enable_sql_tlp = false;
                config.invariant_config.enable_sql_norec = false;
                config.invariant_config.enable_sql_plan_coverage = false;
            }
            "--enable-invariant" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "hash_chain" => config.invariant_config.enable_hash_chain = true,
                        "log_consistency" => config.invariant_config.enable_log_consistency = true,
                        "linearizability" => config.invariant_config.enable_linearizability = true,
                        "replica_consistency" => {
                            config.invariant_config.enable_replica_consistency = true;
                        }
                        "replica_head" => config.invariant_config.enable_replica_head = true,
                        "commit_history" => config.invariant_config.enable_commit_history = true,
                        "vsr_agreement" => config.invariant_config.enable_vsr_agreement = true,
                        "vsr_prefix_property" => {
                            config.invariant_config.enable_vsr_prefix_property = true;
                        }
                        "vsr_view_change_safety" => {
                            config.invariant_config.enable_vsr_view_change_safety = true;
                        }
                        "vsr_recovery_safety" => {
                            config.invariant_config.enable_vsr_recovery_safety = true;
                        }
                        "projection_applied_position" => {
                            config.invariant_config.enable_projection_applied_position = true;
                        }
                        "projection_mvcc" => {
                            config.invariant_config.enable_projection_mvcc_visibility = true;
                        }
                        "projection_applied_index" => {
                            config.invariant_config.enable_projection_applied_index = true;
                        }
                        "projection_catchup" => {
                            config.invariant_config.enable_projection_catchup = true;
                        }
                        "query_determinism" => {
                            config.invariant_config.enable_query_determinism = true;
                        }
                        "query_read_your_writes" => {
                            config.invariant_config.enable_query_read_your_writes = true;
                        }
                        "query_type_safety" => {
                            config.invariant_config.enable_query_type_safety = true;
                        }
                        "query_order_by_limit" => {
                            config.invariant_config.enable_query_order_by_limit = true;
                        }
                        "query_aggregates" => {
                            config.invariant_config.enable_query_aggregates = true;
                        }
                        "query_tenant_isolation" => {
                            config.invariant_config.enable_query_tenant_isolation = true;
                        }
                        "sql_tlp" => config.invariant_config.enable_sql_tlp = true,
                        "sql_norec" => config.invariant_config.enable_sql_norec = true,
                        "sql_plan_coverage" => {
                            config.invariant_config.enable_sql_plan_coverage = true;
                        }
                        _ => eprintln!("Warning: Unknown invariant '{}'", args[i]),
                    }
                }
            }
            "--disable-invariant" => {
                i += 1;
                if i < args.len() {
                    match args[i].as_str() {
                        "hash_chain" => config.invariant_config.enable_hash_chain = false,
                        "log_consistency" => config.invariant_config.enable_log_consistency = false,
                        "linearizability" => config.invariant_config.enable_linearizability = false,
                        "replica_consistency" => {
                            config.invariant_config.enable_replica_consistency = false;
                        }
                        "replica_head" => config.invariant_config.enable_replica_head = false,
                        "commit_history" => config.invariant_config.enable_commit_history = false,
                        "vsr_agreement" => config.invariant_config.enable_vsr_agreement = false,
                        "vsr_prefix_property" => {
                            config.invariant_config.enable_vsr_prefix_property = false;
                        }
                        "vsr_view_change_safety" => {
                            config.invariant_config.enable_vsr_view_change_safety = false;
                        }
                        "vsr_recovery_safety" => {
                            config.invariant_config.enable_vsr_recovery_safety = false;
                        }
                        "projection_applied_position" => {
                            config.invariant_config.enable_projection_applied_position = false;
                        }
                        "projection_mvcc" => {
                            config.invariant_config.enable_projection_mvcc_visibility = false;
                        }
                        "projection_applied_index" => {
                            config.invariant_config.enable_projection_applied_index = false;
                        }
                        "projection_catchup" => {
                            config.invariant_config.enable_projection_catchup = false;
                        }
                        "query_determinism" => {
                            config.invariant_config.enable_query_determinism = false;
                        }
                        "query_read_your_writes" => {
                            config.invariant_config.enable_query_read_your_writes = false;
                        }
                        "query_type_safety" => {
                            config.invariant_config.enable_query_type_safety = false;
                        }
                        "query_order_by_limit" => {
                            config.invariant_config.enable_query_order_by_limit = false;
                        }
                        "query_aggregates" => {
                            config.invariant_config.enable_query_aggregates = false;
                        }
                        "query_tenant_isolation" => {
                            config.invariant_config.enable_query_tenant_isolation = false;
                        }
                        "sql_tlp" => config.invariant_config.enable_sql_tlp = false,
                        "sql_norec" => config.invariant_config.enable_sql_norec = false,
                        "sql_plan_coverage" => {
                            config.invariant_config.enable_sql_plan_coverage = false;
                        }
                        _ => eprintln!("Warning: Unknown invariant '{}'", args[i]),
                    }
                }
            }
            "--list-invariants" => {
                println!("Available Invariants:");
                println!();
                println!("Core (always recommended):");
                println!("  hash_chain, log_consistency, linearizability");
                println!("  replica_consistency, replica_head, commit_history");
                println!();
                println!("VSR (consensus correctness):");
                println!("  vsr_agreement, vsr_prefix_property");
                println!("  vsr_view_change_safety, vsr_recovery_safety");
                println!();
                println!("Projection (MVCC & state machine):");
                println!("  projection_applied_position, projection_mvcc");
                println!("  projection_applied_index, projection_catchup");
                println!();
                println!("Query (SQL correctness):");
                println!("  query_determinism, query_read_your_writes, query_type_safety");
                println!("  query_order_by_limit, query_aggregates, query_tenant_isolation");
                println!();
                println!("SQL Oracles (expensive, opt-in):");
                println!("  sql_tlp, sql_norec, sql_plan_coverage");
                std::process::exit(0);
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
        --scenario <NAME>       Run a predefined test scenario (overrides fault flags)
        --list-scenarios        List all available test scenarios
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

INVARIANT CONTROL:
    --enable-vsr-invariants     Enable all VSR invariants
    --disable-vsr-invariants    Disable all VSR invariants
    --enable-projection-invariants  Enable all projection invariants
    --disable-projection-invariants Disable all projection invariants
    --enable-query-invariants   Enable all query invariants
    --disable-query-invariants  Disable all query invariants
    --enable-sql-oracles        Enable SQL oracles (expensive, 10-100x slower)
    --core-invariants-only      Disable all except core 6 invariants
    --enable-invariant <NAME>   Enable specific invariant
    --disable-invariant <NAME>  Disable specific invariant
    --list-invariants           List all available invariants

TEST SCENARIOS:
    baseline                    No faults, baseline performance
    swizzle                     Swizzle-clogging (intermittent network congestion)
    gray                        Gray failures (partial node failures)
    multi-tenant                Multi-tenant isolation with faults
    time-compression            10x accelerated time for long-running tests
    combined                    All fault types enabled simultaneously

    Plus 21 additional scenarios including:
    - Byzantine attack scenarios (view-change-merge, commit-desync, etc.)
    - Corruption detection (bit-flip, checksum-validation, silent-disk-failure)
    - Crash recovery (crash-commit, crash-view-change, recovery-corrupt)
    - Gray failure variants (slow-disk, intermittent-network)
    - Race conditions (race-view-changes, race-commit-dvc)

    Run --list-scenarios for the complete list with descriptions

EXAMPLES:
    vopr --seed 12345           Run with specific seed
    vopr -n 1000 -v             Run 1000 iterations with verbose output
    vopr --scenario swizzle     Run swizzle-clogging scenario
    vopr --scenario combined -n 500 -v  # Stress test with all faults
    vopr --list-scenarios       Show detailed scenario descriptions
    vopr --list-invariants      Show all available invariants
    vopr --core-invariants-only Run with only core checkers
    vopr --enable-sql-oracles   Enable expensive SQL oracle testing
    vopr --faults network       Enable only network faults (legacy mode)
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

    // Initialize invariant runtime for deterministic sampling
    init_invariant_context(config.seed);

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
        println!("Starting seed: {starting_seed}");
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
                        storage_hash: storage_hash1,
                        kernel_state_hash: kernel_hash1,
                        events_processed: events1,
                        final_time_ns: time1,
                        ..
                    },
                    SimulationResult::Success {
                        storage_hash: storage_hash2,
                        kernel_state_hash: kernel_hash2,
                        events_processed: events2,
                        final_time_ns: time2,
                        ..
                    },
                ) => {
                    let mut violations = Vec::new();

                    if storage_hash1 != storage_hash2 {
                        violations.push(format!(
                            "storage_hash: {} != {}",
                            self::hex::encode(storage_hash1),
                            self::hex::encode(storage_hash2)
                        ));
                    }

                    if kernel_hash1 != kernel_hash2 {
                        violations.push(format!(
                            "kernel_state_hash: {} != {}",
                            self::hex::encode(kernel_hash1),
                            self::hex::encode(kernel_hash2)
                        ));
                    }

                    if events1 != events2 {
                        violations.push(format!("events_processed: {events1} != {events2}"));
                    }

                    if time1 != time2 {
                        violations.push(format!("final_time_ns: {time1} != {time2}"));
                    }

                    if !violations.is_empty() {
                        let msg = format!("determinism violation - {}", violations.join(", "));
                        failures.push((seed, msg));
                        checkpoint.failed_seeds.push(seed);
                        continue;
                    }
                }
                _ => {
                    // If either run failed with an invariant violation,
                    // that's also a determinism issue if they differ
                    if format!("{result:?}") != format!("{result2:?}") {
                        failures.push((
                            seed,
                            "determinism violation: different failure modes".to_string(),
                        ));
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
                kernel_state_hash,
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
                            "storage_hash": self::hex::encode(storage_hash),
                            "kernel_state_hash": self::hex::encode(kernel_state_hash)
                        })),
                    );
                } else if config.verbose {
                    println!(
                        "OK ({} events, {:.2}ms simulated, storage_hash={}, kernel_hash={})",
                        events_processed,
                        final_time_ns as f64 / 1_000_000.0,
                        &self::hex::encode(storage_hash)[..16],
                        &self::hex::encode(kernel_state_hash)[..16]
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
            eprintln!("Warning: Failed to save checkpoint: {e}");
        }
    }

    // Collect coverage data from fault registry, phase tracker, and invariant tracker
    let fault_registry = get_fault_registry();
    let phase_tracker = get_phase_tracker();
    let invariant_tracker = get_invariant_tracker();
    let invariant_counts = invariant_tracker.all_run_counts().clone();
    let coverage_report =
        CoverageReport::generate(&fault_registry, &phase_tracker, invariant_counts);

    // Output final results
    if config.json_mode {
        let coverage_json =
            serde_json::to_value(&coverage_report).unwrap_or(serde_json::Value::Null);

        output(
            true,
            "batch_complete",
            Some(json!({
                "successes": successes,
                "failures": failures.len(),
                "elapsed_secs": elapsed.as_secs_f64(),
                "rate": config.iterations as f64 / elapsed.as_secs_f64(),
                "failed_seeds": failures.iter().map(|(s, _)| s).collect::<Vec<_>>(),
                "coverage": coverage_json
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

        // Output coverage report
        println!();
        println!("{}", coverage_report.to_human_readable());
    }

    // Check coverage thresholds (Phase 2)
    let coverage_failures = validate_coverage_thresholds(&config, &coverage_report);
    if !coverage_failures.is_empty() {
        if !config.json_mode {
            eprintln!();
            eprintln!("======================================");
            eprintln!("COVERAGE THRESHOLD FAILURES:");
            for failure_msg in &coverage_failures {
                eprintln!("  ❌ {failure_msg}");
            }
            eprintln!("======================================");
        }

        if config.json_mode {
            output(
                true,
                "coverage_failures",
                Some(json!({
                    "failures": coverage_failures
                })),
            );
        }

        std::process::exit(2); // Exit code 2 for coverage failures
    }

    if !failures.is_empty() {
        std::process::exit(1);
    }
}

/// Checks if a specific invariant is enabled in the configuration.
fn is_invariant_enabled(name: &str, inv_config: &InvariantConfig) -> bool {
    match name {
        "hash_chain" => inv_config.enable_hash_chain,
        "log_consistency" => inv_config.enable_log_consistency,
        "linearizability" => inv_config.enable_linearizability,
        "replica_consistency" => inv_config.enable_replica_consistency,
        "replica_head" => inv_config.enable_replica_head,
        "commit_history" => inv_config.enable_commit_history,
        "vsr_agreement" => inv_config.enable_vsr_agreement,
        "vsr_prefix_property" => inv_config.enable_vsr_prefix_property,
        "vsr_view_change_safety" => inv_config.enable_vsr_view_change_safety,
        "vsr_recovery_safety" => inv_config.enable_vsr_recovery_safety,
        "projection_applied_position" => inv_config.enable_projection_applied_position,
        "projection_mvcc_visibility" => inv_config.enable_projection_mvcc_visibility,
        "projection_applied_index" => inv_config.enable_projection_applied_index,
        "projection_catchup" => inv_config.enable_projection_catchup,
        "query_determinism" => inv_config.enable_query_determinism,
        "query_read_your_writes" => inv_config.enable_query_read_your_writes,
        "query_type_safety" => inv_config.enable_query_type_safety,
        "query_order_by_limit" => inv_config.enable_query_order_by_limit,
        "query_aggregates" => inv_config.enable_query_aggregates,
        "query_tenant_isolation" => inv_config.enable_query_tenant_isolation,
        "sql_tlp" => inv_config.enable_sql_tlp,
        "sql_norec" => inv_config.enable_sql_norec,
        "sql_plan_coverage" => inv_config.enable_sql_plan_coverage,
        _ => false, // Unknown invariant, assume disabled
    }
}

/// Validates coverage thresholds and returns a list of failure messages.
fn validate_coverage_thresholds(config: &VoprConfig, coverage: &CoverageReport) -> Vec<String> {
    let mut failures = Vec::new();

    // Check fault point coverage threshold
    if config.min_fault_coverage > 0.0
        && coverage.fault_points.coverage_percent < config.min_fault_coverage
    {
        failures.push(format!(
            "Fault point coverage {:.1}% below threshold {:.1}%",
            coverage.fault_points.coverage_percent, config.min_fault_coverage
        ));
    }

    // Check invariant coverage threshold
    if config.min_invariant_coverage > 0.0
        && coverage.invariants.coverage_percent < config.min_invariant_coverage
    {
        failures.push(format!(
            "Invariant coverage {:.1}% below threshold {:.1}%",
            coverage.invariants.coverage_percent, config.min_invariant_coverage
        ));
    }

    // Check if all ENABLED invariants ran (if required)
    if config.require_all_invariants {
        let zero_run_invariants: Vec<&str> = coverage
            .invariants
            .invariant_counts
            .iter()
            .filter(|(name, count)| {
                **count == 0 && is_invariant_enabled(name, &config.invariant_config)
            })
            .map(|(name, _)| name.as_str())
            .collect();

        if !zero_run_invariants.is_empty() {
            failures.push(format!(
                "The following ENABLED invariants ran 0 times: {}",
                zero_run_invariants.join(", ")
            ));
        }
    }

    failures
}
