//! Deterministic sampling runtime for "sometimes assertions".
//!
//! Provides deterministic decision logic for whether to run expensive
//! invariant checks based on global seed and current step count.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

thread_local! {
    static INVARIANT_CONTEXT: RefCell<InvariantContext> = RefCell::new(InvariantContext::new());
}

/// Context for deterministic invariant sampling.
#[derive(Debug, Clone)]
struct InvariantContext {
    seed: u64,
    step: u64,
}

impl InvariantContext {
    fn new() -> Self {
        Self {
            seed: 0,
            step: 0,
        }
    }
}

/// Initialize the invariant sampling context with a seed.
pub fn init_invariant_context(seed: u64) {
    INVARIANT_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        ctx.seed = seed;
        ctx.step = 0;
    });
}

/// Increment the global step counter.
pub fn increment_step() {
    INVARIANT_CONTEXT.with(|ctx| {
        ctx.borrow_mut().step += 1;
    });
}

/// Get the current step count.
pub fn get_step() -> u64 {
    INVARIANT_CONTEXT.with(|ctx| ctx.borrow().step)
}

/// Deterministically decide whether to check an invariant.
///
/// Uses hash(seed ^ hash(key) ^ step) % rate == 0 for determinism.
/// Same seed + step + key always produces the same decision.
pub fn should_check_invariant(key: &str, rate: u64) -> bool {
    if rate == 0 {
        return true; // Always check if rate is 0
    }
    
    INVARIANT_CONTEXT.with(|ctx| {
        let ctx = ctx.borrow();
        
        // Hash the key
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let key_hash = hasher.finish();
        
        // Combine with seed and step
        let mut hasher = DefaultHasher::new();
        (ctx.seed ^ key_hash ^ ctx.step).hash(&mut hasher);
        let combined_hash = hasher.finish();
        
        // Deterministic sampling
        combined_hash % rate == 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_deterministic_sampling() {
        init_invariant_context(12345);
        
        // Same key + step should give same result
        let result1 = should_check_invariant("test_key", 10);
        let result2 = should_check_invariant("test_key", 10);
        assert_eq!(result1, result2);
    }
    
    #[test]
    fn test_different_steps_different_results() {
        init_invariant_context(12345);
        
        let mut results = Vec::new();
        for _ in 0..100 {
            results.push(should_check_invariant("test_key", 10));
            increment_step();
        }
        
        // Should have some true and some false (not all same)
        let true_count = results.iter().filter(|&&x| x).count();
        assert!(true_count > 0);
        assert!(true_count < 100);
    }
    
    #[test]
    fn test_different_seeds_different_patterns() {
        // Pattern with seed 1
        init_invariant_context(1);
        let mut pattern1 = Vec::new();
        for _ in 0..100 {
            pattern1.push(should_check_invariant("test_key", 3));
            increment_step();
        }

        // Pattern with seed 2
        init_invariant_context(2);
        let mut pattern2 = Vec::new();
        for _ in 0..100 {
            pattern2.push(should_check_invariant("test_key", 3));
            increment_step();
        }

        // Count the number of matches - with different seeds, we shouldn't have
        // exactly the same pattern (very unlikely with 100 samples)
        let matches = pattern1.iter().zip(&pattern2).filter(|(a, b)| a == b).count();

        // Even with same distribution, random patterns should differ in many places
        // With 100 samples and 1/3 probability, we expect ~33 true values
        // Exact match would be extremely unlikely (< 0.01% chance)
        assert!(matches < 100, "Patterns should not be identical");
    }
    
    #[test]
    fn test_rate_zero_always_checks() {
        init_invariant_context(12345);
        
        for _ in 0..10 {
            assert!(should_check_invariant("test_key", 0));
            increment_step();
        }
    }
}
