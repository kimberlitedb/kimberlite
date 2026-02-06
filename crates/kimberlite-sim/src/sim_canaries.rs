//! Simulator-level canary mutations for testing VOPR itself.
//!
//! This module contains intentional bugs in the **simulator** (not the system under test)
//! that should be caught by our verification techniques. While `canary.rs` tests Kimberlite
//! invariants, this module tests VOPR's correctness.
//!
//! # Philosophy
//!
//! A simulator is only trustworthy if we can prove it fails correctly. Sim canaries are
//! intentional bugs in VOPR's fault injection mechanisms that **must** be detected by
//! our verification layers (effect tracking, determinism checks, coverage audits).
//!
//! If a sim canary is not caught, either:
//! 1. The verification mechanism is missing
//! 2. The verification is not running
//! 3. The verification is too weak
//!
//! # Available Sim Canaries
//!
//! - `sim-canary-partition-leak`: Partition allows 1% of cross-group messages
//! - `sim-canary-time-leak`: Uses wall-clock time 0.1% of the time
//! - `sim-canary-drop-disabled`: Message drop fault does nothing
//! - `sim-canary-fsync-lies`: Fsync lies about failures (always succeeds)
//! - `sim-canary-rng-unseeded`: RNG uses entropy 0.1% of the time
//!
//! # Usage
//!
//! ```bash
//! # Run VOPR with a sim canary - should detect violation
//! cargo test -p kimberlite-sim --features sim-canary-drop-disabled -- --nocapture
//!
//! # In CI, verify all sim canaries are caught
//! ./scripts/test-sim-canaries.sh
//! ```
//!
//! # Detection Methods
//!
//! - **partition-leak**: Effect tracking (blocked_deliveries too low)
//! - **time-leak**: Determinism check fails (different runs diverge)
//! - **drop-disabled**: Fault coverage and effectiveness too low
//! - **fsync-lies**: Durability invariants fail, data loss not detected
//! - **rng-unseeded**: Determinism check fails (different seeds produce same outcome)

use crate::rng::SimRng;

/// Canary: Partition leaks 1% of cross-group messages.
///
/// **Expected Detection**: Effect tracking (partition effectiveness < 50%)
/// **Mechanism**: blocked_deliveries should be high, but leak reduces it
/// **Why it fails**: Partition should completely isolate groups
#[cfg(feature = "sim-canary-partition-leak")]
pub fn partition_should_leak_message(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.01) // 1% leak rate
}

#[cfg(not(feature = "sim-canary-partition-leak"))]
#[allow(unused_variables)]
pub fn partition_should_leak_message(rng: &mut SimRng) -> bool {
    false
}

/// Canary: Use wall-clock time 0.1% of the time instead of simulated time.
///
/// **Expected Detection**: Determinism check (same seed produces different outcomes)
/// **Mechanism**: Time advancement becomes nondeterministic
/// **Why it fails**: Breaks fundamental determinism guarantee
#[cfg(feature = "sim-canary-time-leak")]
pub fn should_use_wall_clock(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.001) // 0.1% chance
}

#[cfg(not(feature = "sim-canary-time-leak"))]
#[allow(unused_variables)]
pub fn should_use_wall_clock(rng: &mut SimRng) -> bool {
    false
}

/// Canary: Message drop fault is disabled (never drops).
///
/// **Expected Detection**: Fault coverage tracking (drop_applied = 0)
/// **Mechanism**: Statistics show drops not happening
/// **Why it fails**: Drop is a critical fault type for testing message loss
#[cfg(feature = "sim-canary-drop-disabled")]
pub fn should_actually_drop_message() -> bool {
    false // Always deliver, never drop
}

#[cfg(not(feature = "sim-canary-drop-disabled"))]
pub fn should_actually_drop_message() -> bool {
    true // Normal behavior: respect drop probability
}

/// Canary: Fsync always succeeds even when it should fail.
///
/// **Expected Detection**: Durability invariants fail (data loss not caught)
/// **Mechanism**: Lie about fsync failures (make them appear as successes)
/// **Why it fails**: System thinks data is durable when it's not
///
/// Returns true if we should lie about the failure (hide it).
#[cfg(feature = "sim-canary-fsync-lies")]
pub fn fsync_should_lie_about_failure(actual_failed: bool) -> bool {
    // If fsync actually failed, lie and make it succeed
    actual_failed // Return true when failed → causes lie
}

#[cfg(not(feature = "sim-canary-fsync-lies"))]
#[allow(unused_variables)]
pub fn fsync_should_lie_about_failure(actual_failed: bool) -> bool {
    false // Normal behavior: never lie
}

/// Canary: RNG uses system entropy 0.1% of the time.
///
/// **Expected Detection**: Determinism check (same seed produces different outcomes)
/// **Mechanism**: Injects true randomness into deterministic RNG stream
/// **Why it fails**: Breaks reproducibility from seed
#[cfg(feature = "sim-canary-rng-unseeded")]
pub fn should_inject_entropy(rng: &mut SimRng) -> bool {
    rng.next_bool_with_probability(0.001) // 0.1% chance
}

#[cfg(not(feature = "sim-canary-rng-unseeded"))]
#[allow(unused_variables)]
pub fn should_inject_entropy(rng: &mut SimRng) -> bool {
    false
}

/// Returns true if any sim canary feature is enabled.
///
/// Useful for CI to verify that at least one sim canary runs in each test.
pub fn any_sim_canary_enabled() -> bool {
    cfg!(feature = "sim-canary-partition-leak")
        || cfg!(feature = "sim-canary-time-leak")
        || cfg!(feature = "sim-canary-drop-disabled")
        || cfg!(feature = "sim-canary-fsync-lies")
        || cfg!(feature = "sim-canary-rng-unseeded")
}

/// Returns a list of all enabled sim canaries.
pub fn enabled_sim_canaries() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut canaries = Vec::new();

    #[cfg(feature = "sim-canary-partition-leak")]
    canaries.push("sim-canary-partition-leak");

    #[cfg(feature = "sim-canary-time-leak")]
    canaries.push("sim-canary-time-leak");

    #[cfg(feature = "sim-canary-drop-disabled")]
    canaries.push("sim-canary-drop-disabled");

    #[cfg(feature = "sim-canary-fsync-lies")]
    canaries.push("sim-canary-fsync-lies");

    #[cfg(feature = "sim-canary-rng-unseeded")]
    canaries.push("sim-canary-rng-unseeded");

    canaries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "sim-canary-partition-leak")]
    fn test_partition_leak_canary_enabled() {
        let mut rng = SimRng::new(12345);

        // With enough iterations, should leak at least once
        let mut leaked = false;
        for _ in 0..10_000 {
            if partition_should_leak_message(&mut rng) {
                leaked = true;
                break;
            }
        }

        assert!(
            leaked,
            "sim-canary-partition-leak should trigger within 10k iterations"
        );
        println!(
            "Partition leak canary is active - effect tracking should detect low effectiveness"
        );
    }

    #[test]
    #[cfg(not(feature = "sim-canary-partition-leak"))]
    fn test_partition_leak_canary_disabled() {
        let mut rng = SimRng::new(12345);

        // Should never leak when canary is disabled
        for _ in 0..1_000 {
            assert!(!partition_should_leak_message(&mut rng));
        }
    }

    #[test]
    #[cfg(feature = "sim-canary-time-leak")]
    fn test_time_leak_canary_enabled() {
        let mut rng = SimRng::new(99999);

        // With enough iterations, should use wall clock at least once
        let mut used_wall_clock = false;
        for _ in 0..10_000 {
            if should_use_wall_clock(&mut rng) {
                used_wall_clock = true;
                break;
            }
        }

        assert!(
            used_wall_clock,
            "sim-canary-time-leak should trigger within 10k iterations"
        );
        println!("Time leak canary is active - determinism check should fail");
    }

    #[test]
    #[cfg(not(feature = "sim-canary-time-leak"))]
    fn test_time_leak_canary_disabled() {
        let mut rng = SimRng::new(99999);

        // Should never use wall clock when canary is disabled
        for _ in 0..1_000 {
            assert!(!should_use_wall_clock(&mut rng));
        }
    }

    #[test]
    #[cfg(feature = "sim-canary-drop-disabled")]
    fn test_drop_disabled_canary_enabled() {
        // When enabled, should never actually drop
        assert!(
            !should_actually_drop_message(),
            "drop should be disabled when canary is enabled"
        );
        println!("Drop disabled canary is active - fault coverage should be 0");
    }

    #[test]
    #[cfg(not(feature = "sim-canary-drop-disabled"))]
    fn test_drop_disabled_canary_disabled() {
        // When disabled, should allow drops
        assert!(
            should_actually_drop_message(),
            "drop should be enabled when canary is disabled"
        );
    }

    #[test]
    #[cfg(feature = "sim-canary-fsync-lies")]
    fn test_fsync_lies_canary_enabled() {
        // Should invert failure to success
        assert!(
            !fsync_should_lie_about_failure(true),
            "should lie about failure (return success)"
        );
        assert!(
            !fsync_should_lie_about_failure(false),
            "should keep success as success"
        );
        println!("Fsync lies canary is active - durability invariants should fail");
    }

    #[test]
    #[cfg(not(feature = "sim-canary-fsync-lies"))]
    fn test_fsync_lies_canary_disabled() {
        // Should never lie (return false in both cases)
        assert!(
            !fsync_should_lie_about_failure(true),
            "should not lie about failure"
        );
        assert!(
            !fsync_should_lie_about_failure(false),
            "should not lie about success"
        );
    }

    #[test]
    #[cfg(feature = "sim-canary-rng-unseeded")]
    fn test_rng_unseeded_canary_enabled() {
        let mut rng = SimRng::new(55555);

        // With enough iterations, should inject entropy at least once
        let mut injected = false;
        for _ in 0..10_000 {
            if should_inject_entropy(&mut rng) {
                injected = true;
                break;
            }
        }

        assert!(
            injected,
            "sim-canary-rng-unseeded should trigger within 10k iterations"
        );
        println!("RNG unseeded canary is active - determinism check should fail");
    }

    #[test]
    #[cfg(not(feature = "sim-canary-rng-unseeded"))]
    fn test_rng_unseeded_canary_disabled() {
        let mut rng = SimRng::new(55555);

        // Should never inject entropy when canary is disabled
        for _ in 0..1_000 {
            assert!(!should_inject_entropy(&mut rng));
        }
    }

    #[test]
    fn test_sim_canary_detection() {
        let canaries = enabled_sim_canaries();

        if any_sim_canary_enabled() {
            assert!(
                !canaries.is_empty(),
                "should report which sim canaries are enabled"
            );
            println!("Enabled sim canaries: {:?}", canaries);
        } else {
            assert!(
                canaries.is_empty(),
                "should report no sim canaries when all disabled"
            );
        }
    }
}
