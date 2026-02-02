//! # Step 1: Pure vs. Impure Functions
//!
//! **Learning objective:** Understand the difference between pure and impure functions.
//!
//! ## Key Concepts
//!
//! A **pure function**:
//! - Given the same inputs, ALWAYS returns the same output
//! - Has no side effects (no IO, no mutation of external state)
//! - Deterministic and predictable
//!
//! An **impure function**:
//! - May return different outputs for the same inputs
//! - Has side effects (IO, randomness, global state, clocks)
//! - Non-deterministic and unpredictable
//!
//! ## Why This Matters
//!
//! Pure functions are:
//! - **Testable**: Write a test once, it passes forever
//! - **Composable**: Combine pure functions without worrying about order
//! - **Parallelizable**: No shared state means safe concurrent execution
//! - **Reproducible**: Critical for database replication
//!
//! ## The Refactoring Pattern
//!
//! When you have impure code:
//! 1. Extract the pure logic into a pure function
//! 2. Move side effects to the "shell" (caller)
//! 3. Pass impure data as function parameters
//!
//! This is the foundation of FCIS (Functional Core, Imperative Shell).

use std::time::SystemTime;

// ============================================================================
// Anti-Pattern: Impure Functions (DO NOT USE IN PRODUCTION)
// ============================================================================

/// IMPURE: Uses randomness internally.
///
/// Problem: Can't test this reliably. Every call returns different output.
#[allow(dead_code)]
fn generate_id_impure() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};

    let random_state = RandomState::new();
    let mut hasher = random_state.build_hasher();
    SystemTime::now().hash(&mut hasher);
    hasher.finish()
}

/// IMPURE: Reads current time internally.
///
/// Problem: Returns different values at different times. Not reproducible.
#[allow(dead_code)]
fn seconds_since_epoch_impure() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs()
}

/// IMPURE: Both takes input AND modifies it.
///
/// Problem: Caller must track state, can't reason about function in isolation.
#[allow(dead_code)]
fn increment_impure(counter: &mut u64) {
    *counter += 1;
}

// ============================================================================
// Good Pattern: Pure Functions
// ============================================================================

/// PURE: Takes random bytes as input, returns deterministic output.
///
/// Key insight: Move the randomness to the CALLER. The function itself is pure.
pub fn generate_id_pure(random_bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    random_bytes.hash(&mut hasher);
    hasher.finish()
}

/// PURE: Takes timestamp as input instead of reading clock.
///
/// Key insight: The caller provides the time, the function computes with it.
pub fn seconds_since_epoch_pure(now: SystemTime) -> u64 {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs()
}

/// PURE: Takes value, returns new value. No mutation.
///
/// Key insight: Return a new value instead of modifying input.
pub fn increment_pure(counter: u64) -> u64 {
    counter + 1
}

/// PURE: More complex example - increment with overflow check.
///
/// Returns `None` if incrementing would overflow.
pub fn increment_checked(counter: u64) -> Option<u64> {
    counter.checked_add(1)
}

// ============================================================================
// Comparison: Pure vs Impure Counter
// ============================================================================

/// Impure counter using mutable state.
pub struct ImpureCounter {
    value: u64,
}

impl ImpureCounter {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// IMPURE: Mutates self, no return value.
    pub fn increment(&mut self) {
        self.value += 1;
    }

    /// IMPURE: Returns different value each time increment() was called.
    pub fn get(&self) -> u64 {
        self.value
    }
}

impl Default for ImpureCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure counter using functional style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PureCounter {
    value: u64,
}

impl PureCounter {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// PURE: Takes self, returns new counter. Original unchanged.
    pub fn increment(self) -> Self {
        Self {
            value: self.value + 1,
        }
    }

    /// PURE: Always returns the same value for the same counter.
    pub fn get(&self) -> u64 {
        self.value
    }
}

impl Default for PureCounter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// The FCIS Pattern Applied to Counter
// ============================================================================

/// Commands represent requests to change state (functional core input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterCommand {
    Increment,
    Decrement,
    Reset,
    Set(u64),
}

/// Effects represent side effects to execute (imperative shell output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterEffect {
    LogChange { old: u64, new: u64 },
    NotifyObservers(u64),
}

/// PURE: The functional core.
///
/// This is the pattern Kimberlite uses:
/// - Take current state + command
/// - Return new state + effects
/// - NO side effects in this function
pub fn apply_counter_command(
    state: PureCounter,
    cmd: CounterCommand,
) -> (PureCounter, Vec<CounterEffect>) {
    let old_value = state.get();

    let new_state = match cmd {
        CounterCommand::Increment => state.increment(),
        CounterCommand::Decrement => PureCounter {
            value: state.value.saturating_sub(1),
        },
        CounterCommand::Reset => PureCounter::new(),
        CounterCommand::Set(val) => PureCounter { value: val },
    };

    let effects = vec![
        CounterEffect::LogChange {
            old: old_value,
            new: new_state.get(),
        },
        CounterEffect::NotifyObservers(new_state.get()),
    ];

    (new_state, effects)
}

/// IMPURE: The imperative shell.
///
/// This is where side effects actually happen:
/// - Executes effects produced by the functional core
/// - Does IO, logging, notifications, etc.
pub fn execute_counter_effect(effect: CounterEffect) {
    match effect {
        CounterEffect::LogChange { old, new } => {
            // This is impure: writes to stdout
            println!("Counter changed: {} -> {}", old, new);
        }
        CounterEffect::NotifyObservers(value) => {
            // This is impure: would notify external observers
            println!("Notifying observers: {}", value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_functions_are_deterministic() {
        let random_bytes = b"fixed seed";

        // Call the function 100 times with same input
        let results: Vec<u64> = (0..100)
            .map(|_| generate_id_pure(random_bytes))
            .collect();

        // All results should be identical
        assert!(results.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn pure_counter_is_immutable() {
        let counter = PureCounter::new();
        let incremented = counter.increment();

        // Original counter unchanged
        assert_eq!(counter.get(), 0);
        // New counter has new value
        assert_eq!(incremented.get(), 1);
    }

    #[test]
    fn fcis_pattern_produces_effects() {
        let state = PureCounter::new();
        let cmd = CounterCommand::Set(42);

        let (new_state, effects) = apply_counter_command(state, cmd);

        assert_eq!(new_state.get(), 42);
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], CounterEffect::LogChange { old: 0, new: 42 }));
    }

    #[test]
    fn same_command_same_result() {
        let state = PureCounter { value: 10 };
        let cmd = CounterCommand::Increment;

        // Apply command twice to same state
        let (result1, effects1) = apply_counter_command(state, cmd);
        let (result2, effects2) = apply_counter_command(state, cmd);

        // Results must be identical (determinism)
        assert_eq!(result1, result2);
        assert_eq!(effects1, effects2);
    }
}
