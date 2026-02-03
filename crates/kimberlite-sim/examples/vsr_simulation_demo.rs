//! Demo of VSR simulation mode.
//!
//! This example demonstrates how to use the VSR simulation mode to test
//! the actual VSR protocol with Byzantine resistance.
//!
//! Run with:
//! ```bash
//! cargo run --example vsr_simulation_demo
//! ```

#![allow(clippy::uninlined_format_args)] // Example code, old-style format is fine

use kimberlite_sim::{CommitNumberConsistencyChecker, SimRng, StorageConfig, VsrSimulation};

#[allow(clippy::too_many_lines)] // Demo example showing full workflow
fn main() {
    println!("VSR Simulation Demo");
    println!("===================\n");

    // Create simulation with 3 replicas
    let storage_config = StorageConfig::reliable();
    let mut sim = VsrSimulation::new(storage_config, 42);
    let mut rng = SimRng::new(42);

    println!("Initial state:");
    for i in 0..3 {
        let replica = sim.replica(i);
        println!(
            "  Replica {}: view={}, op={}, commit={}",
            i,
            replica.view().as_u64(),
            replica.op_number().as_u64(),
            replica.commit_number().as_u64()
        );
    }
    println!();

    // Step 1: Submit a client request
    println!("Step 1: Submit client request to leader (replica 0)");
    let prepare_messages = sim.process_client_request(&mut rng);
    println!("  Leader generated {} Prepare messages", prepare_messages.len());

    let leader = sim.replica(0);
    println!(
        "  Leader state: op={}, commit={}",
        leader.op_number().as_u64(),
        leader.commit_number().as_u64()
    );
    println!();

    // Step 2: Deliver Prepare to backups
    println!("Step 2: Deliver Prepare messages to backups");
    let mut prepare_ok_messages = Vec::new();

    for (i, prepare_msg) in prepare_messages.iter().enumerate() {
        let backup_id = (i + 1) as u8; // Backups are replicas 1 and 2
        println!("  Delivering Prepare to replica {}", backup_id);

        let responses = sim.deliver_message(backup_id, prepare_msg.clone(), &mut rng);
        println!("    Replica {} generated {} responses", backup_id, responses.len());

        prepare_ok_messages.extend(responses);
    }
    println!();

    // Step 3: Deliver PrepareOK to leader
    println!("Step 3: Deliver PrepareOK messages to leader");
    for prepare_ok in &prepare_ok_messages {
        let responses = sim.deliver_message(0, prepare_ok.clone(), &mut rng);
        println!("  Leader processed PrepareOK, generated {} responses", responses.len());
    }
    println!();

    // Step 4: Check final state
    println!("Step 4: Final state");
    for i in 0..3 {
        let replica = sim.replica(i);
        println!(
            "  Replica {}: view={}, op={}, commit={}",
            i,
            replica.view().as_u64(),
            replica.op_number().as_u64(),
            replica.commit_number().as_u64()
        );
    }
    println!();

    // Step 5: Run invariant checks
    println!("Step 5: Invariant checks");
    let snapshots = sim.extract_snapshots();

    // Check commit_number <= op_number for each replica
    let mut checker = CommitNumberConsistencyChecker::new();
    for snapshot in &snapshots {
        let result = checker.check_consistency(
            snapshot.replica_id,
            snapshot.op_number,
            snapshot.commit_number,
        );
        println!(
            "  Replica {}: commit_number ({}) <= op_number ({}) - {}",
            snapshot.replica_id.as_u8(),
            snapshot.commit_number.as_u64(),
            snapshot.op_number.as_u64(),
            if result.is_ok() { "✓" } else { "✗" }
        );
    }
    println!();

    // Check agreement: all replicas at same op should have same log entries
    println!("Step 6: Agreement check");
    let min_op = snapshots
        .iter()
        .map(|s| s.op_number.as_u64())
        .min()
        .unwrap_or(0);

    if min_op > 0 {
        // Check if all replicas have the same log entry at op=1
        let log0 = &snapshots[0].log;
        let log1 = &snapshots[1].log;
        let log2 = &snapshots[2].log;

        if !log0.is_empty() && !log1.is_empty() && !log2.is_empty() {
            let entry0 = &log0[0];
            let entry1 = &log1[0];
            let entry2 = &log2[0];

            let agreement = entry0.op_number == entry1.op_number
                && entry1.op_number == entry2.op_number
                && entry0.view == entry1.view
                && entry1.view == entry2.view;

            println!(
                "  All replicas agree on op={}: {}",
                entry0.op_number.as_u64(),
                if agreement { "✓" } else { "✗" }
            );
        } else {
            println!("  Not all replicas have log entries yet");
        }
    } else {
        println!("  No operations committed yet");
    }
    println!();

    println!("Demo complete!");
}
