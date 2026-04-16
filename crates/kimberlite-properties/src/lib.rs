//! Antithesis-style property annotations for Deterministic Simulation Testing.
//!
//! This crate provides lightweight `macro_rules!` macros for annotating code with
//! temporal property assertions inspired by the [Antithesis SDK](https://antithesis.com/docs/properties_assertions/assertions/).
//!
//! # Property Types
//!
//! - **ALWAYS**: Condition must hold every time it is evaluated. A single `false` is a violation.
//! - **SOMETIMES**: Condition must be `true` at least once per simulation run. Guides exploration.
//! - **NEVER**: Condition must never be `true`. Equivalent to `always!(!condition)`.
//! - **REACHED**: Code path must be reached at least once per simulation run.
//! - **UNREACHABLE**: Code path must never be reached. Violation on first reach.
//!
//! # Zero-Cost in Production
//!
//! Without `feature = "sim"`, all macros compile to nothing. No runtime overhead.
//! With `feature = "sim"`, assertions record to thread-local registries for VOPR tracking.
//!
//! # Usage
//!
//! ```ignore
//! use kimberlite_properties::{always, sometimes, never, reached, unreachable_property};
//!
//! fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>)> {
//!     let old_offset = state.offset();
//!     let (new_state, effects) = process(state, cmd)?;
//!
//!     always!(
//!         new_state.offset() >= old_offset,
//!         "offset_monotonicity",
//!         "applied offset must never decrease"
//!     );
//!
//!     sometimes!(
//!         effects.len() > 3,
//!         "batch_effects",
//!         "simulation should sometimes produce large effect batches"
//!     );
//!
//!     Ok((new_state, effects))
//! }
//! ```

pub mod registry;

// ============================================================================
// Property Macros
// ============================================================================

/// Asserts that a condition is ALWAYS true when evaluated.
///
/// In simulation mode, records every evaluation. A single `false` evaluation
/// is reported as a property violation.
///
/// In production builds, compiles to nothing.
///
/// # Arguments
/// - `$cond` - Boolean condition to check
/// - `$id` - String literal property identifier (must be unique across codebase)
/// - `$msg` - Human-readable description of what this property means
#[macro_export]
macro_rules! always {
    ($cond:expr, $id:literal, $msg:literal) => {
        #[cfg(any(test, feature = "sim"))]
        {
            let condition = $cond;
            $crate::registry::record_always($id, condition, $msg);
            assert!(condition, concat!("ALWAYS property violated [", $id, "]: ", $msg));
        }
    };
}

/// Asserts that a condition is true at least once per simulation run.
///
/// This is a **coverage signal**, not a bug catcher. It tells the simulator
/// "go find a state where this is true." The simulator preferentially explores
/// paths toward triggering `sometimes` properties that haven't been satisfied yet.
///
/// In production builds, compiles to nothing.
///
/// # Arguments
/// - `$cond` - Boolean condition to check
/// - `$id` - String literal property identifier (must be unique across codebase)
/// - `$msg` - Human-readable description of what this property means
#[macro_export]
macro_rules! sometimes {
    ($cond:expr, $id:literal, $msg:literal) => {
        #[cfg(any(test, feature = "sim"))]
        {
            $crate::registry::record_sometimes($id, $cond, $msg);
        }
    };
}

/// Asserts that a condition is NEVER true when evaluated.
///
/// Equivalent to `always!(!condition, ...)` but reads more naturally for
/// expressing safety invariants (e.g., "two leaders NEVER exist in the same view").
///
/// In production builds, compiles to nothing.
///
/// # Arguments
/// - `$cond` - Boolean condition that must NOT be true
/// - `$id` - String literal property identifier (must be unique across codebase)
/// - `$msg` - Human-readable description of what this property means
#[macro_export]
macro_rules! never {
    ($cond:expr, $id:literal, $msg:literal) => {
        #[cfg(any(test, feature = "sim"))]
        {
            let condition = $cond;
            $crate::registry::record_never($id, condition, $msg);
            assert!(!condition, concat!("NEVER property violated [", $id, "]: ", $msg));
        }
    };
}

/// Asserts that a code path is reached at least once per simulation run.
///
/// Place this at interesting code locations (error handlers, rare branches,
/// fault recovery paths) to ensure simulation exercises them.
///
/// In production builds, compiles to nothing.
///
/// # Arguments
/// - `$id` - String literal property identifier (must be unique across codebase)
/// - `$msg` - Human-readable description of what this code path represents
#[macro_export]
macro_rules! reached {
    ($id:literal, $msg:literal) => {
        #[cfg(any(test, feature = "sim"))]
        {
            $crate::registry::record_reached($id, $msg);
        }
    };
}

/// Asserts that a code path is NEVER reached.
///
/// Place this at locations that should be dead code or impossible states.
/// Reaching this location is a property violation.
///
/// In production builds, compiles to nothing.
///
/// # Arguments
/// - `$id` - String literal property identifier (must be unique across codebase)
/// - `$msg` - Human-readable description of why this path is unreachable
#[macro_export]
macro_rules! unreachable_property {
    ($id:literal, $msg:literal) => {
        #[cfg(any(test, feature = "sim"))]
        {
            $crate::registry::record_unreachable($id, $msg);
            panic!(concat!("UNREACHABLE property violated [", $id, "]: ", $msg));
        }
    };
}

#[cfg(test)]
mod tests {
    // Macros should compile and work in test mode (which has cfg(test))

    #[test]
    fn test_always_passing() {
        always!(true, "test.always_pass", "always true in test");
    }

    #[test]
    #[should_panic(expected = "ALWAYS property violated")]
    fn test_always_failing() {
        always!(false, "test.always_fail", "should panic");
    }

    #[test]
    fn test_sometimes_records() {
        sometimes!(true, "test.sometimes_true", "recorded as satisfied");
        sometimes!(false, "test.sometimes_false", "recorded as unsatisfied");
    }

    #[test]
    fn test_never_passing() {
        never!(false, "test.never_pass", "never true in test");
    }

    #[test]
    #[should_panic(expected = "NEVER property violated")]
    fn test_never_failing() {
        never!(true, "test.never_fail", "should panic");
    }

    #[test]
    fn test_reached() {
        reached!("test.reached", "code path exercised");
    }

    #[test]
    #[should_panic(expected = "UNREACHABLE property violated")]
    fn test_unreachable() {
        unreachable_property!("test.unreachable", "should panic");
    }

    #[test]
    fn test_registry_snapshot() {
        use crate::registry;

        registry::reset();

        always!(true, "snap.always", "test");
        sometimes!(true, "snap.sometimes_hit", "test");
        sometimes!(false, "snap.sometimes_miss", "test");
        never!(false, "snap.never", "test");
        reached!("snap.reached", "test");

        let snapshot = registry::snapshot();

        // ALWAYS property evaluated once, never violated
        let always_prop = snapshot.get("snap.always").unwrap();
        assert_eq!(always_prop.evaluations, 1);
        assert_eq!(always_prop.violations, 0);

        // SOMETIMES satisfied
        let sometimes_hit = snapshot.get("snap.sometimes_hit").unwrap();
        assert!(sometimes_hit.satisfied);

        // SOMETIMES not satisfied
        let sometimes_miss = snapshot.get("snap.sometimes_miss").unwrap();
        assert!(!sometimes_miss.satisfied);

        // NEVER property evaluated once, never violated
        let never_prop = snapshot.get("snap.never").unwrap();
        assert_eq!(never_prop.evaluations, 1);
        assert_eq!(never_prop.violations, 0);

        // REACHED
        let reached_prop = snapshot.get("snap.reached").unwrap();
        assert!(reached_prop.satisfied);
    }
}
