//! Global fault point registry for tracking coverage.
//!
//! Thread-local storage tracks which fault points have been executed
//! during simulation runs.

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static FAULT_REGISTRY: RefCell<FaultRegistry> = RefCell::new(FaultRegistry::new());
}

/// Registry of fault injection points with hit counts.
#[derive(Debug, Clone)]
pub struct FaultRegistry {
    /// Map from fault point key to hit count
    fault_points: HashMap<String, u64>,
}

impl FaultRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            fault_points: HashMap::new(),
        }
    }
    
    /// Record that a fault point was reached.
    fn record(&mut self, key: &str) {
        *self.fault_points.entry(key.to_string()).or_insert(0) += 1;
    }
    
    /// Get the hit count for a fault point.
    pub fn get_hit_count(&self, key: &str) -> u64 {
        self.fault_points.get(key).copied().unwrap_or(0)
    }
    
    /// Get all fault points and their hit counts.
    pub fn all_fault_points(&self) -> &HashMap<String, u64> {
        &self.fault_points
    }
    
    /// Calculate fault point coverage percentage.
    ///
    /// Returns (hit_count, total_count, coverage_percent)
    pub fn coverage(&self) -> (usize, usize, f64) {
        let total = self.fault_points.len();
        if total == 0 {
            return (0, 0, 100.0);
        }
        
        let hit = self.fault_points.values().filter(|&&count| count > 0).count();
        let coverage = (hit as f64 / total as f64) * 100.0;
        
        (hit, total, coverage)
    }
    
    /// Reset all hit counts to zero.
    pub fn reset(&mut self) {
        self.fault_points.clear();
    }
}

impl Default for FaultRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Record that a fault point was reached (called by macros).
pub fn record_fault_point(key: &str) {
    FAULT_REGISTRY.with(|registry| {
        registry.borrow_mut().record(key);
    });
}

/// Check if a fault should be injected at this point (called by macros).
///
/// Currently always returns false - actual fault injection logic will be
/// integrated with SimFaultInjector in future tasks.
pub fn should_inject_fault(_key: &str) -> bool {
    // TODO: Integrate with SimFaultInjector from vopr.rs
    // For now, never inject faults (this is just coverage tracking)
    false
}

/// Get a snapshot of the current fault registry.
pub fn get_fault_registry() -> FaultRegistry {
    FAULT_REGISTRY.with(|registry| registry.borrow().clone())
}

/// Reset the global fault registry.
pub fn reset_fault_registry() {
    FAULT_REGISTRY.with(|registry| registry.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fault_registry_tracking() {
        let mut registry = FaultRegistry::new();
        
        registry.record("storage.fsync");
        registry.record("storage.fsync");
        registry.record("network.send");
        
        assert_eq!(registry.get_hit_count("storage.fsync"), 2);
        assert_eq!(registry.get_hit_count("network.send"), 1);
        assert_eq!(registry.get_hit_count("unknown"), 0);
    }
    
    #[test]
    fn test_fault_registry_coverage() {
        let mut registry = FaultRegistry::new();
        
        // Empty registry
        let (hit, total, coverage) = registry.coverage();
        assert_eq!(hit, 0);
        assert_eq!(total, 0);
        assert_eq!(coverage, 100.0);
        
        // Some fault points hit
        registry.record("fault1");
        registry.record("fault2");
        registry.record("fault3");
        
        let (hit, total, coverage) = registry.coverage();
        assert_eq!(hit, 3);
        assert_eq!(total, 3);
        assert_eq!(coverage, 100.0);
    }
    
    #[test]
    fn test_fault_registry_reset() {
        let mut registry = FaultRegistry::new();
        
        registry.record("fault1");
        assert_eq!(registry.get_hit_count("fault1"), 1);
        
        registry.reset();
        assert_eq!(registry.get_hit_count("fault1"), 0);
        assert_eq!(registry.all_fault_points().len(), 0);
    }
}
