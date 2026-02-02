//! Global invariant execution tracker.
//!
//! Tracks how many times each invariant checker has run to ensure
//! all invariants are actually executed during simulation.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static INVARIANT_TRACKER: RefCell<InvariantTracker> = RefCell::new(InvariantTracker::new());
}

/// Tracker for invariant execution counts.
#[derive(Debug, Clone)]
pub struct InvariantTracker {
    /// Map from invariant name to run count
    run_counts: HashMap<String, u64>,
}

impl InvariantTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self {
            run_counts: HashMap::new(),
        }
    }
    
    /// Record that an invariant was executed.
    fn record(&mut self, invariant_name: &str) {
        *self.run_counts.entry(invariant_name.to_string()).or_insert(0) += 1;
    }
    
    /// Get the run count for an invariant.
    pub fn get_run_count(&self, invariant_name: &str) -> u64 {
        self.run_counts.get(invariant_name).copied().unwrap_or(0)
    }
    
    /// Get all invariant run counts.
    pub fn all_run_counts(&self) -> &HashMap<String, u64> {
        &self.run_counts
    }
    
    /// Reset all run counts to zero.
    pub fn reset(&mut self) {
        self.run_counts.clear();
    }
}

impl Default for InvariantTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Record that an invariant was executed (called by checkers).
pub fn record_invariant_execution(invariant_name: &str) {
    INVARIANT_TRACKER.with(|tracker| {
        tracker.borrow_mut().record(invariant_name);
    });
}

/// Get a snapshot of the current invariant tracker.
pub fn get_invariant_tracker() -> InvariantTracker {
    INVARIANT_TRACKER.with(|tracker| tracker.borrow().clone())
}

/// Reset the global invariant tracker.
pub fn reset_invariant_tracker() {
    INVARIANT_TRACKER.with(|tracker| tracker.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_invariant_tracking() {
        let mut tracker = InvariantTracker::new();
        
        tracker.record("linearizability");
        tracker.record("linearizability");
        tracker.record("hash_chain_integrity");
        
        assert_eq!(tracker.get_run_count("linearizability"), 2);
        assert_eq!(tracker.get_run_count("hash_chain_integrity"), 1);
        assert_eq!(tracker.get_run_count("unknown"), 0);
    }
    
    #[test]
    fn test_invariant_reset() {
        let mut tracker = InvariantTracker::new();
        
        tracker.record("test_invariant");
        assert_eq!(tracker.get_run_count("test_invariant"), 1);
        
        tracker.reset();
        assert_eq!(tracker.get_run_count("test_invariant"), 0);
        assert_eq!(tracker.all_run_counts().len(), 0);
    }
    
    #[test]
    fn test_global_invariant_tracker() {
        reset_invariant_tracker();
        
        record_invariant_execution("test1");
        record_invariant_execution("test1");
        record_invariant_execution("test2");
        
        let tracker = get_invariant_tracker();
        assert_eq!(tracker.get_run_count("test1"), 2);
        assert_eq!(tracker.get_run_count("test2"), 1);
        
        reset_invariant_tracker();
        let tracker = get_invariant_tracker();
        assert_eq!(tracker.get_run_count("test1"), 0);
    }
}
