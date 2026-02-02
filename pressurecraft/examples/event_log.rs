//! Event Log Example: Append-Only Structure
//!
//! This example demonstrates commands and effects with a simple append-only log.
//!
//! Run with: `cargo run --example event_log`

use bytes::Bytes;
use pressurecraft::step2_commands_effects::{
    command_to_effects, Command, DataClass, Offset, StreamId,
};

fn main() {
    println!("=== Event Log Example: Commands & Effects ===\n");

    // Commands represent operations
    let commands = vec![
        Command::create_stream(
            StreamId::new(1),
            "user_events".to_string(),
            DataClass::Internal,
        ),
        Command::append_batch(
            StreamId::new(1),
            vec![
                Bytes::from(r#"{"type":"login","user":"alice"}"#),
                Bytes::from(r#"{"type":"purchase","user":"alice","amount":99}"#),
            ],
            Offset::ZERO,
        ),
        Command::append_batch(
            StreamId::new(1),
            vec![Bytes::from(r#"{"type":"logout","user":"alice"}"#)],
            Offset::new(2),
        ),
        Command::read_stream(StreamId::new(1), Offset::ZERO, 10),
    ];

    let timestamp = 1000;

    for cmd in commands {
        println!("Command: {:?}", cmd);

        // Transform command into effects
        let effects = command_to_effects(cmd, timestamp);

        println!("  Effects generated:");
        for effect in effects {
            println!("    - {:?}", effect);
        }

        println!();
    }

    println!("=== Key Insights ===");
    println!("1. Commands are data structures (can be serialized)");
    println!("2. Effects describe side effects without executing them");
    println!("3. Same command → same effects (deterministic)");
    println!("4. The kernel transforms commands into effects");
}
