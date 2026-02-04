//! Clock adapter trait for simulation vs production time sources.
//!
//! This module provides a trait-based abstraction for time sources, enabling:
//! - **Deterministic simulation**: Use `SimClock` with discrete time advancement
//! - **Production use**: Use `SystemClock` with real wall-clock time
//! - **Per-node clock skew**: Each replica can have independent clock skew
//!
//! # Performance
//!
//! The `Clock` trait is designed for hot-path use with generics:
//! - Methods are `#[inline]` for zero-cost abstraction
//! - Use `impl Clock` or `<C: Clock>` generic parameters, NOT `&dyn Clock`
//! - Monomorphization eliminates trait dispatch overhead
//!
//! # Example: Per-Node Clock Skew
//!
//! ```rust,ignore
//! let replica0 = VsrReplicaWrapper::new(
//!     0,
//!     SimClock::new(),                    // No skew
//!     /* ... */
//! );
//!
//! let replica1 = VsrReplicaWrapper::new(
//!     1,
//!     SimClock::with_skew(-5_000_000),   // 5ms behind
//!     /* ... */
//! );
//!
//! let replica2 = VsrReplicaWrapper::new(
//!     2,
//!     SimClock::with_skew(3_000_000),    // 3ms ahead
//!     /* ... */
//! );
//! ```

/// Trait for time sources (simulation or production).
///
/// Implementations must be `Send + Sync` for use in concurrent contexts.
pub trait Clock: Send + Sync {
    /// Returns current time in nanoseconds since epoch.
    ///
    /// For simulation clocks, this may include per-node skew.
    /// For production clocks, this returns wall-clock time.
    fn now(&self) -> u64;

    /// Advances time to the given value (simulation-only).
    ///
    /// Production implementations should make this a no-op.
    /// Simulation implementations update their internal time.
    ///
    /// # Panics
    ///
    /// May panic in debug builds if `time_ns < self.now()` (time going backwards).
    fn advance_to(&mut self, time_ns: u64);

    /// Returns current time in milliseconds (convenience method).
    #[inline]
    fn now_ms(&self) -> u64 {
        self.now() / 1_000_000
    }

    /// Advances the clock by a delta (simulation-only).
    ///
    /// Production implementations should make this a no-op.
    ///
    /// # Panics
    ///
    /// May panic in debug builds on overflow.
    #[inline]
    fn advance_by(&mut self, delta_ns: u64) {
        let new_time = self.now().checked_add(delta_ns).expect("clock overflow");
        self.advance_to(new_time);
    }
}

// ============================================================================
// Simulation Implementation
// ============================================================================

/// Deterministic clock with per-node skew support.
///
/// This clock provides nanosecond-precision simulated time that advances
/// only when explicitly requested. Each replica can have independent skew
/// to model clock drift in distributed systems.
///
/// # Per-Node Skew
///
/// Clock skew is applied when reading time via `now()`:
/// ```text
/// actual_time = base_time + skew_ns
/// ```
///
/// Skew can be positive (replica ahead) or negative (replica behind).
#[derive(Debug, Clone)]
pub struct SimClock {
    /// Base simulation time in nanoseconds.
    now_ns: u64,

    /// Per-node clock skew in nanoseconds (can be negative).
    ///
    /// - Positive: This node's clock is ahead of global time
    /// - Negative: This node's clock is behind global time
    /// - Zero: No skew (default)
    skew_ns: i64,
}

impl SimClock {
    /// Creates a new clock starting at time zero with no skew.
    pub fn new() -> Self {
        Self {
            now_ns: 0,
            skew_ns: 0,
        }
    }

    /// Creates a clock with the specified skew in nanoseconds.
    ///
    /// # Arguments
    ///
    /// * `skew_ns` - Clock skew relative to global time
    ///   - Positive: Clock is ahead (e.g., +5_000_000 = 5ms ahead)
    ///   - Negative: Clock is behind (e.g., -3_000_000 = 3ms behind)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let clock_ahead = SimClock::with_skew(5_000_000);  // 5ms ahead
    /// let clock_behind = SimClock::with_skew(-3_000_000); // 3ms behind
    /// ```
    pub fn with_skew(skew_ns: i64) -> Self {
        Self { now_ns: 0, skew_ns }
    }

    /// Creates a clock starting at the specified time with optional skew.
    pub fn at(now_ns: u64, skew_ns: i64) -> Self {
        Self { now_ns, skew_ns }
    }

    /// Returns the clock skew in nanoseconds.
    pub fn skew(&self) -> i64 {
        self.skew_ns
    }

    /// Sets the clock skew (used for dynamic skew injection).
    pub fn set_skew(&mut self, skew_ns: i64) {
        self.skew_ns = skew_ns;
    }
}

impl Clock for SimClock {
    #[inline]
    fn now(&self) -> u64 {
        // Apply skew (saturating to prevent underflow)
        self.now_ns.saturating_add_signed(self.skew_ns)
    }

    fn advance_to(&mut self, time_ns: u64) {
        debug_assert!(
            time_ns >= self.now_ns,
            "time cannot go backwards: current={}, target={}",
            self.now_ns,
            time_ns
        );
        self.now_ns = time_ns;
    }
}

impl Default for SimClock {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// System clock using wall-clock time (production use).
///
/// This implementation uses `std::time::Instant` for monotonic time.
/// Time advancement methods are no-ops in production.
///
/// **Note**: This is a sketch for architectural demonstration.
/// Full implementation would be behind a `#[cfg(not(test))]` gate.
#[cfg(not(test))]
pub struct SystemClock {
    /// Reference point for time measurements.
    start: std::time::Instant,
}

#[cfg(not(test))]
impl SystemClock {
    /// Creates a new system clock anchored to the current instant.
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

#[cfg(not(test))]
impl Clock for SystemClock {
    #[inline]
    fn now(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }

    fn advance_to(&mut self, _time_ns: u64) {
        // No-op for production - time advances naturally
    }
}

#[cfg(not(test))]
impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_clock_starts_at_zero() {
        let clock = SimClock::new();
        assert_eq!(clock.now(), 0);
        assert_eq!(clock.skew(), 0);
    }

    #[test]
    fn sim_clock_advance_to() {
        let mut clock = SimClock::new();
        clock.advance_to(5_000_000);
        assert_eq!(clock.now(), 5_000_000);
    }

    #[test]
    fn sim_clock_advance_by() {
        let mut clock = SimClock::new();
        clock.advance_by(1_000_000);
        assert_eq!(clock.now(), 1_000_000);
        clock.advance_by(500_000);
        assert_eq!(clock.now(), 1_500_000);
    }

    #[test]
    fn sim_clock_with_positive_skew() {
        let clock = SimClock::with_skew(5_000_000); // 5ms ahead
        assert_eq!(clock.now(), 5_000_000); // Reports 5ms at time 0

        let mut clock = clock;
        clock.advance_to(1_000_000); // Base time = 1ms
        assert_eq!(clock.now(), 6_000_000); // Reports 6ms (1ms + 5ms skew)
    }

    #[test]
    fn sim_clock_with_negative_skew() {
        let clock = SimClock::with_skew(-3_000_000); // 3ms behind
        assert_eq!(clock.now(), 0); // Saturates to 0 at time 0

        let mut clock = clock;
        clock.advance_to(5_000_000); // Base time = 5ms
        assert_eq!(clock.now(), 2_000_000); // Reports 2ms (5ms - 3ms skew)
    }

    #[test]
    fn sim_clock_skew_saturates_at_zero() {
        let clock = SimClock::with_skew(-10_000_000); // 10ms behind
        // At base time 0, saturates to 0 (doesn't underflow)
        assert_eq!(clock.now(), 0);
    }

    #[test]
    fn sim_clock_set_skew_dynamically() {
        let mut clock = SimClock::new();
        clock.advance_to(10_000_000); // 10ms

        assert_eq!(clock.now(), 10_000_000);

        clock.set_skew(2_000_000); // 2ms ahead
        assert_eq!(clock.now(), 12_000_000);

        clock.set_skew(-5_000_000); // 5ms behind
        assert_eq!(clock.now(), 5_000_000);
    }

    #[test]
    fn sim_clock_now_ms() {
        let clock = SimClock::at(5_500_000, 0); // 5.5ms
        assert_eq!(clock.now_ms(), 5);
    }

    #[test]
    fn sim_clock_now_ms_with_skew() {
        let clock = SimClock::at(10_000_000, 5_000_000); // 10ms + 5ms skew
        assert_eq!(clock.now_ms(), 15);
    }

    #[test]
    #[should_panic(expected = "time cannot go backwards")]
    fn sim_clock_advance_to_past_panics() {
        let mut clock = SimClock::at(5_000_000, 0);
        clock.advance_to(1_000_000); // Should panic in debug builds
    }

    #[test]
    fn trait_object_works() {
        let mut clock: Box<dyn Clock> = Box::new(SimClock::new());
        assert_eq!(clock.now(), 0);
        clock.advance_to(1_000_000);
        assert_eq!(clock.now(), 1_000_000);
    }
}
