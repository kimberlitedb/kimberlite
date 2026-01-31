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

use kmb_sim::{
    EventKind, HashChainChecker, LinearizabilityChecker, LogConsistencyChecker, NetworkConfig,
    OpType, ReplicaConsistencyChecker, SimConfig, SimNetwork, SimRng, SimStorage, Simulation,
    StorageConfig,
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
// Simulation Execution
// ============================================================================

/// Result of a simulation run.
#[derive(Debug)]
enum SimulationResult {
    /// Simulation completed successfully.
    Success {
        events_processed: u64,
        final_time_ns: u64,
    },
    /// An invariant was violated.
    InvariantViolation {
        invariant: String,
        message: String,
        events_processed: u64,
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

    // Register simulated nodes
    for node_id in 0..3 {
        network.register_node(node_id);
    }

    // Schedule initial events
    for i in 0..10 {
        let delay = rng.delay_ns(1_000_000, 10_000_000);
        sim.schedule_after(delay, EventKind::Custom(i));
    }

    // Track operation state for linearizability
    let mut pending_ops: Vec<(u64, u64)> = Vec::new(); // (op_id, key)

    // Run simulation loop
    while let Some(event) = sim.step() {
        match event.kind {
            EventKind::Custom(op_type) => {
                // Simulate different operation types
                match op_type % 4 {
                    0 => {
                        // Write operation
                        let key = rng.next_u64() % 10;
                        let value = rng.next_u64();
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

                        // Write to storage
                        let data = value.to_le_bytes().to_vec();
                        let _ = storage.write(key, data, &mut rng);
                    }
                    1 => {
                        // Read operation
                        let key = rng.next_u64() % 10;
                        let result = storage.read(key, &mut rng);
                        let value = match result {
                            kmb_sim::ReadResult::Success { data, .. } => {
                                if data.len() >= 8 {
                                    Some(u64::from_le_bytes(data[..8].try_into().unwrap()))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };

                        let op_id = linearizability_checker.invoke(
                            0,
                            event.time_ns,
                            OpType::Read { key, value },
                        );
                        linearizability_checker.respond(op_id, event.time_ns + 1000);
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
                        let log_length = (event.time_ns / 1_000_000) % 1000;
                        let mut hash = [0u8; 32];
                        // Same log length should have same hash (deterministic)
                        hash[0..8].copy_from_slice(&log_length.to_le_bytes());

                        let result = replica_checker.update_replica(
                            replica_id,
                            log_length,
                            hash,
                            event.time_ns,
                        );

                        if !result.is_ok() {
                            return SimulationResult::InvariantViolation {
                                invariant: "replica_consistency".to_string(),
                                message: format!("Replica divergence at time {}", event.time_ns),
                                events_processed: sim.events_processed(),
                            };
                        }
                    }
                    _ => unreachable!(),
                }

                // Schedule more events to keep simulation running
                if sim.events().len() < 5 {
                    let delay = rng.delay_ns(1_000_000, 10_000_000);
                    let next_op = rng.next_u64() % 4;
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
                    return SimulationResult::InvariantViolation {
                        invariant: "linearizability".to_string(),
                        message: "History is not linearizable".to_string(),
                        events_processed: sim.events_processed(),
                    };
                }

                let replica_result = replica_checker.check_all();
                if !replica_result.is_ok() {
                    return SimulationResult::InvariantViolation {
                        invariant: "replica_consistency".to_string(),
                        message: "Replicas have diverged".to_string(),
                        events_processed: sim.events_processed(),
                    };
                }
            }
            _ => {
                // Handle other event types
            }
        }
    }

    // Final invariant check
    let lin_result = linearizability_checker.check();
    if !lin_result.is_ok() {
        return SimulationResult::InvariantViolation {
            invariant: "linearizability".to_string(),
            message: "Final history is not linearizable".to_string(),
            events_processed: sim.events_processed(),
        };
    }

    SimulationResult::Success {
        events_processed: sim.events_processed(),
        final_time_ns: sim.now(),
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
    -h, --help                  Print this help message

EXAMPLES:
    vopr --seed 12345           Run with specific seed
    vopr -n 1000 -v             Run 1000 iterations with verbose output
    vopr --faults network       Enable only network faults
    vopr --no-faults            Run without any fault injection
    vopr --json --checkpoint-file /var/lib/vopr/checkpoint.json
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

        match result {
            SimulationResult::Success {
                events_processed,
                final_time_ns,
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
                            "simulated_time_ms": final_time_ns as f64 / 1_000_000.0
                        })),
                    );
                } else if config.verbose {
                    println!(
                        "OK ({} events, {:.2}ms simulated)",
                        events_processed,
                        final_time_ns as f64 / 1_000_000.0
                    );
                }
            }
            SimulationResult::InvariantViolation {
                invariant,
                message,
                events_processed,
            } => {
                failures.push((seed, format!("{invariant}: {message}")));
                checkpoint.failed_seeds.push(seed);

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
