//! Mini Database Example: Complete FCIS System
//!
//! This example demonstrates a complete mini database using the FCIS pattern.
//! It shows the kernel (functional core) and runtime (imperative shell) working together.
//!
//! Run with: `cargo run --example mini_database`

use bytes::Bytes;
use pressurecraft::step2_commands_effects::{DataClass, Offset, StreamId};
use pressurecraft::step4_mini_kernel::{apply, execute_effect, Command, State};

fn main() {
    println!("=== Mini Database Example ===\n");

    // Initial state (empty)
    let mut state = State::new();
    println!("Initial state: empty database\n");

    // Simulate a sequence of database operations
    let operations = vec![
        (
            "Create stream 'user_events'",
            Command::create_stream(
                StreamId::new(1),
                "user_events".to_string(),
                DataClass::Internal,
            ),
        ),
        (
            "Create stream 'orders'",
            Command::create_stream(
                StreamId::new(2),
                "orders".to_string(),
                DataClass::Confidential,
            ),
        ),
        (
            "Append user events",
            Command::append_batch(
                StreamId::new(1),
                vec![
                    Bytes::from(r#"{"user":"alice","action":"login"}"#),
                    Bytes::from(r#"{"user":"bob","action":"login"}"#),
                ],
                Offset::ZERO,
            ),
        ),
        (
            "Append order",
            Command::append_batch(
                StreamId::new(2),
                vec![Bytes::from(
                    r#"{"order_id":1001,"user":"alice","total":99.99}"#,
                )],
                Offset::ZERO,
            ),
        ),
        (
            "Append more user events",
            Command::append_batch(
                StreamId::new(1),
                vec![Bytes::from(r#"{"user":"alice","action":"logout"}"#)],
                Offset::new(2),
            ),
        ),
    ];

    let mut had_error = false;

    for (description, command) in operations {
        println!("▶ {description}");
        println!("  Command: {:?}", command);

        // FUNCTIONAL CORE: Apply command to get new state + effects
        match apply(state, command) {
            Ok((new_state, effects)) => {
                println!("  ✓ Success");
                println!("  Effects to execute ({}):", effects.len());
                for (i, effect) in effects.iter().enumerate() {
                    println!("    {}. {:?}", i + 1, effect);
                }

                // IMPERATIVE SHELL: Execute effects
                println!("  Executing effects:");
                for effect in effects {
                    print!("    ");
                    execute_effect(effect);
                }

                // Update state for next operation
                state = new_state;
            }
            Err(err) => {
                println!("  ✗ Error: {:?}", err);
                had_error = true;
                // State was moved but not reassigned - must recreate it
                state = State::new();
                break;
            }
        }

        println!();
    }

    // Show final state (only if no error)
    if !had_error {
        println!("=== Final State ===");
        if let Some(stream) = state.get_stream(&StreamId::new(1)) {
            println!(
                "Stream 1 (user_events): offset = {:?}",
                stream.current_offset
            );
        }
        if let Some(stream) = state.get_stream(&StreamId::new(2)) {
            println!("Stream 2 (orders): offset = {:?}", stream.current_offset);
        }
    }

    println!("\n=== Key Insights ===");
    println!("1. The kernel (apply) is PURE - no IO, deterministic");
    println!("2. State transitions are validated before applying");
    println!("3. Effects are DESCRIBED by kernel, EXECUTED by runtime");
    println!("4. This pattern enables:");
    println!("   - Testing (test kernel without real storage)");
    println!("   - Replication (replay commands on replicas)");
    println!("   - Time-travel debugging (replay commands)");
    println!("   - Audit logging (record all commands)");

    println!("\n=== Try It ===");
    println!("1. Modify operations to trigger errors (wrong offset, duplicate stream)");
    println!("2. Add your own commands");
    println!("3. Notice how errors are detected BEFORE effects execute");
}
