//! Counter Example: Simplest State Machine
//!
//! This example demonstrates the FCIS pattern with the simplest possible state machine:
//! a counter that can increment and decrement.
//!
//! Run with: `cargo run --example counter`

use pressurecraft::step1_pure_functions::{
    apply_counter_command, execute_counter_effect, CounterCommand, PureCounter,
};

fn main() {
    println!("=== Counter Example: FCIS Pattern ===\n");

    // Start with initial state
    let mut state = PureCounter::new();
    println!("Initial state: {:?}\n", state);

    // Apply commands
    let commands = vec![
        CounterCommand::Increment,
        CounterCommand::Increment,
        CounterCommand::Set(10),
        CounterCommand::Increment,
        CounterCommand::Decrement,
        CounterCommand::Reset,
    ];

    for cmd in commands {
        println!("Command: {:?}", cmd);

        // FUNCTIONAL CORE: Pure function returns new state + effects
        let (new_state, effects) = apply_counter_command(state, cmd);

        println!("  New state: {:?}", new_state);
        println!("  Effects:");
        for effect in &effects {
            println!("    - {:?}", effect);
        }

        // IMPERATIVE SHELL: Execute effects
        println!("  Executing effects:");
        for effect in effects {
            print!("    ");
            execute_counter_effect(effect);
        }

        println!();

        // Update state for next iteration
        state = new_state;
    }

    println!("Final state: {:?}", state);

    println!("\n=== Key Insights ===");
    println!("1. The functional core (apply_counter_command) is pure");
    println!("2. State transitions are deterministic");
    println!("3. Side effects are DESCRIBED (not executed) by the core");
    println!("4. The imperative shell (execute_counter_effect) does the IO");
}
