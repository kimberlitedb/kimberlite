//! RNG adapter trait for simulation vs production randomness.
//!
//! This module provides a trait-based abstraction for random number generation:
//! - **Deterministic simulation**: Use `SimRng` with seedable, reproducible randomness
//! - **Production use**: Use `OsRng` wrapper for cryptographic randomness
//! - **Per-node isolation**: Fork RNGs for independent per-replica randomness streams
//!
//! # Performance
//!
//! The `Rng` trait is designed for hot-path use with generics:
//! - Methods are `#[inline]` for zero-cost abstraction
//! - Use `impl Rng` or `<R: Rng>` generic parameters, NOT `&dyn Rng`
//! - Monomorphization eliminates trait dispatch overhead
//!
//! # Example: Per-Node RNG Forking
//!
//! ```rust,ignore
//! let mut master_rng = SimRng::new(seed);
//!
//! let replica0_rng = master_rng.fork(); // Independent stream
//! let replica1_rng = master_rng.fork(); // Independent stream
//! let replica2_rng = master_rng.fork(); // Independent stream
//!
//! // Each replica has deterministic but independent randomness
//! ```

// Re-export the existing SimRng from the parent module
pub use crate::SimRng;

/// Trait for random number generation (simulation or production).
///
/// Implementations provide deterministic (simulation) or cryptographic (production)
/// random number generation.
pub trait Rng {
    /// Generates a random `u64`.
    fn next_u64(&mut self) -> u64;

    /// Generates a random `u32`.
    fn next_u32(&mut self) -> u32;

    /// Generates a random `bool`.
    fn next_bool(&mut self) -> bool;

    /// Generates a random `f64` in the range `[0.0, 1.0)`.
    fn next_f64(&mut self) -> f64;

    /// Generates a random `bool` with the given probability of being `true`.
    #[inline]
    fn next_bool_with_probability(&mut self, probability: f64) -> bool {
        self.next_f64() < probability
    }

    /// Generates a random `usize` in the range `[0, max)`.
    fn next_usize(&mut self, max: usize) -> usize;

    /// Generates a random `u64` in the range `[min, max)`.
    #[inline]
    fn next_u64_range(&mut self, min: u64, max: u64) -> u64 {
        debug_assert!(min < max, "min must be < max");
        min + (self.next_u64() % (max - min))
    }

    /// Generates a random delay in nanoseconds within the given range.
    ///
    /// Useful for simulating network latency, disk I/O, etc.
    #[inline]
    fn delay_ns(&mut self, min_ns: u64, max_ns: u64) -> u64 {
        self.next_u64_range(min_ns, max_ns)
    }

    /// Fills a byte slice with random bytes.
    fn fill_bytes(&mut self, dest: &mut [u8]);

    /// Forks a new RNG with derived seed (for per-node RNGs).
    ///
    /// This creates an independent random number stream that is still
    /// deterministically derived from the parent RNG.
    ///
    /// # Use Case
    ///
    /// Fork is used to give each replica its own independent RNG:
    /// ```text
    /// master_rng (seed 12345)
    ///   ├─> replica0_rng (forked with seed X)
    ///   ├─> replica1_rng (forked with seed Y)
    ///   └─> replica2_rng (forked with seed Z)
    /// ```
    ///
    /// Each replica's RNG is deterministic and independent.
    fn fork(&mut self) -> Box<dyn Rng>;
}

// ============================================================================
// Simulation Implementation
// ============================================================================

impl Rng for SimRng {
    #[inline]
    fn next_u64(&mut self) -> u64 {
        // Call the inherent method on SimRng via UFCS
        SimRng::next_u64(self)
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        SimRng::next_u32(self)
    }

    #[inline]
    fn next_bool(&mut self) -> bool {
        SimRng::next_bool(self)
    }

    #[inline]
    fn next_f64(&mut self) -> f64 {
        SimRng::next_f64(self)
    }

    #[inline]
    fn next_usize(&mut self, max: usize) -> usize {
        SimRng::next_usize(self, max)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        SimRng::fill_bytes(self, dest);
    }

    fn fork(&mut self) -> Box<dyn Rng> {
        Box::new(SimRng::fork(self))
    }
}

// ============================================================================
// Production Implementation (Sketch)
// ============================================================================

/// Wrapper around `OsRng` for production use (cryptographic randomness).
///
/// **Note**: This is a sketch for architectural demonstration.
/// Full implementation would use proper cryptographic RNG.
#[cfg(not(test))]
pub struct OsRngWrapper {
    inner: rand::rngs::OsRng,
}

#[cfg(not(test))]
impl Default for OsRngWrapper {
    fn default() -> Self {
        Self {
            inner: rand::rngs::OsRng,
        }
    }
}

#[cfg(not(test))]
impl OsRngWrapper {
    /// Creates a new OS-backed RNG.
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(not(test))]
impl Rng for OsRngWrapper {
    fn next_u64(&mut self) -> u64 {
        use rand::Rng as _;
        self.inner.r#gen()
    }

    fn next_u32(&mut self) -> u32 {
        use rand::Rng as _;
        self.inner.r#gen()
    }

    fn next_bool(&mut self) -> bool {
        use rand::Rng as _;
        self.inner.r#gen()
    }

    fn next_f64(&mut self) -> f64 {
        use rand::Rng as _;
        self.inner.r#gen()
    }

    fn next_usize(&mut self, max: usize) -> usize {
        use rand::Rng as _;
        self.inner.gen_range(0..max)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use rand::RngCore;
        self.inner.fill_bytes(dest);
    }

    fn fork(&mut self) -> Box<dyn Rng> {
        // For production, forking creates a new independent OS RNG
        Box::new(OsRngWrapper::new())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_rng_trait_impl() {
        let mut rng: Box<dyn Rng> = Box::new(SimRng::new(12345));

        let _a = rng.next_u64();
        let _b = rng.next_u32();
        let _c = rng.next_bool();
        let _d = rng.next_f64();
        let _e = rng.next_usize(100);
        let _delay = rng.delay_ns(1000, 2000);
    }

    #[test]
    fn sim_rng_fork_via_trait() {
        let mut rng: Box<dyn Rng> = Box::new(SimRng::new(12345));

        let val1 = rng.next_u64();

        let mut forked = rng.fork();
        let val2 = forked.next_u64();

        // Forked RNG should produce different values than parent
        assert_ne!(val1, val2);
    }

    #[test]
    fn sim_rng_deterministic_via_trait() {
        let mut rng1: Box<dyn Rng> = Box::new(SimRng::new(12345));
        let mut rng2: Box<dyn Rng> = Box::new(SimRng::new(12345));

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn sim_rng_fill_bytes() {
        let mut rng: Box<dyn Rng> = Box::new(SimRng::new(12345));

        let mut buf1 = [0u8; 32];
        let mut buf2 = [0u8; 32];

        rng.fill_bytes(&mut buf1);
        rng.fill_bytes(&mut buf2);

        // Should produce different bytes
        assert_ne!(buf1, buf2);
    }

    #[test]
    fn sim_rng_next_bool_with_probability() {
        let mut rng: Box<dyn Rng> = Box::new(SimRng::new(12345));

        // Probability 0.0 should always be false
        for _ in 0..10 {
            assert!(!rng.next_bool_with_probability(0.0));
        }

        // Probability 1.0 should always be true
        for _ in 0..10 {
            assert!(rng.next_bool_with_probability(1.0));
        }
    }

    #[test]
    fn sim_rng_generic_usage() {
        fn use_rng<R: Rng>(rng: &mut R) -> u64 {
            rng.next_u64()
        }

        let mut rng = SimRng::new(12345);
        let val1 = use_rng(&mut rng);
        let val2 = use_rng(&mut rng);

        assert_ne!(val1, val2);
    }

    #[test]
    fn fork_produces_independent_streams() {
        let mut master = SimRng::new(12345);

        let val1 = master.next_u64();

        let mut child1 = master.fork();
        let mut child2 = master.fork();

        // Children should be independent
        let c1_val1 = child1.next_u64();
        let c2_val1 = child2.next_u64();

        // Children's first values should differ from master's first value
        assert_ne!(c1_val1, val1);
        assert_ne!(c2_val1, val1);

        // Children should be independent of each other
        // (might be equal by chance, but sequence should diverge)
        let c1_val2 = child1.next_u64();
        let c2_val2 = child2.next_u64();

        // At least one pair should be different (very high probability)
        assert!(c1_val1 != c2_val1 || c1_val2 != c2_val2);
    }
}
