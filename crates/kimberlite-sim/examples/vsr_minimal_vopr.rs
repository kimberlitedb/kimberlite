//! Minimal VOPR demonstration with VSR mode.
//!
//! This example shows a complete integration of `VsrSimulation` into a
//! VOPR-style event loop with invariant checking. It serves as a template
//! for the full VOPR integration.
//!
//! Run with:
//! ```bash
//! cargo run --example vsr_minimal_vopr
//! cargo run --example vsr_minimal_vopr -- --seed 42 --events 1000
//! ```

#![allow(clippy::uninlined_format_args)] // Example code, old-style format is fine

use std::env;

use kimberlite_sim::{
    AgreementChecker, CommitNumberConsistencyChecker, EventKind, EventQueue, InvariantResult,
    PrefixPropertyChecker, SimClock, SimConfig, SimRng, StorageConfig, VsrSimulation,
    check_all_vsr_invariants, schedule_client_request, schedule_vsr_messages,
    vsr_message_from_bytes,
};

// ============================================================================
// Configuration
// ============================================================================

struct MiniVoprConfig {
    seed: u64,
    max_events: u64,
    client_request_rate_ns: u64, // Mean time between client requests
    network_min_delay_ns: u64,
    network_max_delay_ns: u64,
}

impl Default for MiniVoprConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            max_events: 100,
            client_request_rate_ns: 10_000_000, // 10ms between requests
            network_min_delay_ns: 100_000,      // 100μs
            network_max_delay_ns: 1_000_000,    // 1ms
        }
    }
}

// ============================================================================
// Main Simulation
// ============================================================================

fn main() {
    // Parse command-line arguments
    let config = parse_args();

    println!("VSR Minimal VOPR");
    println!("================");
    println!("Seed: {}", config.seed);
    println!("Max Events: {}", config.max_events);
    println!();

    // Run simulation
    match run_simulation(&config) {
        SimResult::Success {
            events_processed,
            client_requests,
            messages_sent,
        } => {
            println!("\n✓ Simulation completed successfully");
            println!("  Events processed: {}", events_processed);
            println!("  Client requests: {}", client_requests);
            println!("  Messages sent: {}", messages_sent);
        }
        SimResult::InvariantViolation {
            invariant,
            message,
            events_processed,
        } => {
            eprintln!("\n✗ Invariant violation detected");
            eprintln!("  Invariant: {}", invariant);
            eprintln!("  Message: {}", message);
            eprintln!("  Events processed: {}", events_processed);
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_lines)] // Demo function showing full simulation workflow
fn run_simulation(config: &MiniVoprConfig) -> SimResult {
    // Initialize simulation components
    let _sim_config = SimConfig::default()
        .with_seed(config.seed)
        .with_max_events(config.max_events);

    let mut clock = SimClock::new();
    let mut queue = EventQueue::new();
    let mut rng = SimRng::new(config.seed);

    // Initialize VSR simulation
    let storage_config = StorageConfig::reliable();
    let mut vsr_sim = VsrSimulation::new(storage_config, config.seed);

    // Initialize invariant checkers
    let mut commit_checker = CommitNumberConsistencyChecker::new();
    let mut agreement_checker = AgreementChecker::new();
    let mut prefix_checker = PrefixPropertyChecker::new();

    // Statistics
    let mut events_processed = 0u64;
    let mut client_requests = 0u64;
    let mut messages_sent = 0u64;

    // Schedule initial client request
    schedule_client_request(
        &mut queue,
        clock.now(),
        config.client_request_rate_ns,
        0, // Leader replica
    );

    // Main event loop
    while let Some(event) = queue.pop() {
        // Check limits
        if events_processed >= config.max_events {
            break;
        }

        // Advance clock
        clock.advance_to(event.time_ns);
        events_processed += 1;

        if events_processed % 10 == 0 {
            print!(".");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }

        // Process event
        match event.kind {
            EventKind::VsrClientRequest { replica_id, .. } => {
                client_requests += 1;

                // Process client request through VSR
                let messages = vsr_sim.process_client_request(&mut rng);

                // Schedule message deliveries
                let count = schedule_vsr_messages(
                    &mut queue,
                    clock.now(),
                    &messages,
                    &mut rng,
                    config.network_min_delay_ns,
                    config.network_max_delay_ns,
                );
                messages_sent += count as u64;

                // Schedule next client request
                if client_requests < 10 {
                    // Only send 10 requests total for this demo
                    schedule_client_request(
                        &mut queue,
                        clock.now(),
                        config.client_request_rate_ns,
                        replica_id,
                    );
                }
            }

            EventKind::VsrMessage {
                to_replica,
                message_bytes,
            } => {
                // Deserialize and deliver message
                let msg = vsr_message_from_bytes(&message_bytes);
                let responses = vsr_sim.deliver_message(to_replica, msg, &mut rng);

                // Schedule response deliveries
                let count = schedule_vsr_messages(
                    &mut queue,
                    clock.now(),
                    &responses,
                    &mut rng,
                    config.network_min_delay_ns,
                    config.network_max_delay_ns,
                );
                messages_sent += count as u64;
            }

            _ => {
                // Other event types not handled in this minimal example
            }
        }

        // Check invariants every 10 events
        if events_processed % 10 == 0 {
            let snapshots = vsr_sim.extract_snapshots();

            let result = check_all_vsr_invariants(
                &mut commit_checker,
                &mut agreement_checker,
                &mut prefix_checker,
                &snapshots,
            );

            if !result.is_ok() {
                if let InvariantResult::Violated {
                    invariant, message, ..
                } = result
                {
                    return SimResult::InvariantViolation {
                        invariant,
                        message,
                        events_processed,
                    };
                }
            }
        }
    }

    // Final invariant check
    let snapshots = vsr_sim.extract_snapshots();
    let result = check_all_vsr_invariants(
        &mut commit_checker,
        &mut agreement_checker,
        &mut prefix_checker,
        &snapshots,
    );

    if !result.is_ok() {
        if let InvariantResult::Violated {
            invariant, message, ..
        } = result
        {
            return SimResult::InvariantViolation {
                invariant,
                message,
                events_processed,
            };
        }
    }

    SimResult::Success {
        events_processed,
        client_requests,
        messages_sent,
    }
}

// ============================================================================
// Results
// ============================================================================

enum SimResult {
    Success {
        events_processed: u64,
        client_requests: u64,
        messages_sent: u64,
    },
    InvariantViolation {
        invariant: String,
        message: String,
        events_processed: u64,
    },
}

// ============================================================================
// CLI Parsing
// ============================================================================

fn parse_args() -> MiniVoprConfig {
    let mut config = MiniVoprConfig::default();
    let args: Vec<String> = env::args().collect();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                if i + 1 < args.len() {
                    config.seed = args[i + 1].parse().unwrap_or(42);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--events" => {
                if i + 1 < args.len() {
                    config.max_events = args[i + 1].parse().unwrap_or(100);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    config
}
