//! Global fault point registry for tracking coverage.
//!
//! Thread-local storage tracks which fault points have been executed
//! during simulation runs.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static FAULT_REGISTRY: RefCell<FaultRegistry> = RefCell::new(FaultRegistry::new());
}

/// Fault point with effect tracking.
///
/// Tracks the full lifecycle of a fault:
/// - **attempted**: Fault RNG check passed (decided to inject)
/// - **applied**: Fault was actually injected into the system
/// - **observed**: Effect of fault was detected (e.g., checksum failure, blocked delivery)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultPoint {
    /// Number of times fault was attempted (RNG said "yes")
    pub attempted: u64,
    /// Number of times fault was applied (actually injected)
    pub applied: u64,
    /// Number of times fault effect was observed (had impact)
    pub observed: u64,
}

/// Effectiveness report for fault types.
///
/// Shows what percentage of applied faults had observable effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivenessReport {
    /// Effectiveness of partition faults (% that blocked messages)
    pub partition: f64,
    /// Effectiveness of corruption faults (% that caused checksum failures)
    pub corruption: f64,
    /// Effectiveness of crash faults (% that lost data)
    pub crash: f64,
    /// Effectiveness of slow disk faults (% that delayed operations)
    pub slow_disk: f64,
    /// Effectiveness of drop faults (% that actually dropped messages)
    pub drop: f64,
    /// Effectiveness of delay faults (% that delayed messages)
    pub delay: f64,
}

/// Registry of fault injection points with effect tracking.
#[derive(Debug, Clone)]
pub struct FaultRegistry {
    /// Map from fault point key to fault tracking data
    fault_points: HashMap<String, FaultPoint>,
}

impl FaultRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            fault_points: HashMap::new(),
        }
    }

    /// Record that a fault was attempted (RNG check passed).
    fn record_attempted(&mut self, key: &str) {
        self.fault_points
            .entry(key.to_string())
            .or_default()
            .attempted += 1;
    }

    /// Record that a fault was applied (actually injected).
    fn record_applied(&mut self, key: &str) {
        self.fault_points
            .entry(key.to_string())
            .or_default()
            .applied += 1;
    }

    /// Record that a fault effect was observed (had impact).
    fn record_observed(&mut self, key: &str) {
        self.fault_points
            .entry(key.to_string())
            .or_default()
            .observed += 1;
    }

    /// Legacy method: Record that a fault point was reached.
    ///
    /// This increments the "attempted" counter for backwards compatibility.
    fn record(&mut self, key: &str) {
        self.record_attempted(key);
    }

    /// Get the hit count for a fault point (legacy, returns attempted count).
    pub fn get_hit_count(&self, key: &str) -> u64 {
        self.fault_points
            .get(key)
            .map(|fp| fp.attempted)
            .unwrap_or(0)
    }

    /// Get the attempted count for a fault point.
    pub fn get_attempted(&self, key: &str) -> u64 {
        self.fault_points
            .get(key)
            .map(|fp| fp.attempted)
            .unwrap_or(0)
    }

    /// Get the applied count for a fault point.
    pub fn get_applied(&self, key: &str) -> u64 {
        self.fault_points
            .get(key)
            .map(|fp| fp.applied)
            .unwrap_or(0)
    }

    /// Get the observed count for a fault point.
    pub fn get_observed(&self, key: &str) -> u64 {
        self.fault_points
            .get(key)
            .map(|fp| fp.observed)
            .unwrap_or(0)
    }

    /// Get the fault point data for a specific key.
    pub fn get_fault_point(&self, key: &str) -> Option<&FaultPoint> {
        self.fault_points.get(key)
    }

    /// Get all fault points and their tracking data.
    pub fn all_fault_points(&self) -> &HashMap<String, FaultPoint> {
        &self.fault_points
    }

    /// Calculate effectiveness ratio for a fault point (observed / applied).
    ///
    /// Returns percentage of applied faults that had observable effects.
    /// Returns 0.0 if no faults were applied.
    pub fn effectiveness(&self, key: &str) -> f64 {
        let point = match self.fault_points.get(key) {
            Some(p) => p,
            None => return 0.0,
        };

        if point.applied == 0 {
            return 0.0;
        }

        (point.observed as f64 / point.applied as f64) * 100.0
    }

    /// Calculate fault point coverage percentage.
    ///
    /// Returns (hit_count, total_count, coverage_percent)
    pub fn coverage(&self) -> (usize, usize, f64) {
        let total = self.fault_points.len();
        if total == 0 {
            return (0, 0, 100.0);
        }

        let hit = self
            .fault_points
            .values()
            .filter(|fp| fp.attempted > 0)
            .count();
        let coverage = (hit as f64 / total as f64) * 100.0;

        (hit, total, coverage)
    }

    /// Reset all counters to zero.
    pub fn reset(&mut self) {
        self.fault_points.clear();
    }

    /// Generate an effectiveness report for common fault types.
    ///
    /// Returns effectiveness percentages (observed / applied) for each fault type.
    pub fn effectiveness_report(&self) -> EffectivenessReport {
        EffectivenessReport {
            partition: self.effectiveness("network.partition"),
            corruption: self.effectiveness("storage.corruption"),
            crash: self.effectiveness("storage.crash"),
            slow_disk: self.effectiveness("storage.slow"),
            drop: self.effectiveness("network.drop"),
            delay: self.effectiveness("network.delay"),
        }
    }
}

impl Default for FaultRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Record that a fault point was reached (legacy, called by macros).
///
/// This records a fault attempt for backwards compatibility.
pub fn record_fault_point(key: &str) {
    FAULT_REGISTRY.with(|registry| {
        registry.borrow_mut().record(key);
    });
}

/// Record that a fault was attempted (RNG check passed).
pub fn record_fault_attempted(key: &str) {
    FAULT_REGISTRY.with(|registry| {
        registry.borrow_mut().record_attempted(key);
    });
}

/// Record that a fault was applied (actually injected).
pub fn record_fault_applied(key: &str) {
    FAULT_REGISTRY.with(|registry| {
        registry.borrow_mut().record_applied(key);
    });
}

/// Record that a fault effect was observed (had impact).
pub fn record_fault_observed(key: &str) {
    FAULT_REGISTRY.with(|registry| {
        registry.borrow_mut().record_observed(key);
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

    #[test]
    fn test_effect_tracking() {
        let mut registry = FaultRegistry::new();

        // Simulate 10 attempts, 8 applied, 6 observed
        for _ in 0..10 {
            registry.record_attempted("network.partition");
        }
        for _ in 0..8 {
            registry.record_applied("network.partition");
        }
        for _ in 0..6 {
            registry.record_observed("network.partition");
        }

        assert_eq!(registry.get_attempted("network.partition"), 10);
        assert_eq!(registry.get_applied("network.partition"), 8);
        assert_eq!(registry.get_observed("network.partition"), 6);

        // Effectiveness = observed / applied = 6 / 8 = 75%
        let effectiveness = registry.effectiveness("network.partition");
        assert!((effectiveness - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_effectiveness_zero_applied() {
        let registry = FaultRegistry::new();

        // No faults applied, effectiveness should be 0
        let effectiveness = registry.effectiveness("nonexistent");
        assert_eq!(effectiveness, 0.0);
    }

    #[test]
    fn test_effectiveness_full_impact() {
        let mut registry = FaultRegistry::new();

        // All applied faults had observable effects
        registry.record_applied("storage.corruption");
        registry.record_applied("storage.corruption");
        registry.record_applied("storage.corruption");
        registry.record_observed("storage.corruption");
        registry.record_observed("storage.corruption");
        registry.record_observed("storage.corruption");

        let effectiveness = registry.effectiveness("storage.corruption");
        assert_eq!(effectiveness, 100.0);
    }

    #[test]
    fn test_effectiveness_no_impact() {
        let mut registry = FaultRegistry::new();

        // Faults applied but never observed
        registry.record_applied("network.drop");
        registry.record_applied("network.drop");

        let effectiveness = registry.effectiveness("network.drop");
        assert_eq!(effectiveness, 0.0);
    }

    #[test]
    fn test_effectiveness_report() {
        let mut registry = FaultRegistry::new();

        // Network partition: 50% effective
        registry.record_applied("network.partition");
        registry.record_applied("network.partition");
        registry.record_observed("network.partition");

        // Storage corruption: 100% effective
        registry.record_applied("storage.corruption");
        registry.record_observed("storage.corruption");

        let report = registry.effectiveness_report();

        assert_eq!(report.partition, 50.0);
        assert_eq!(report.corruption, 100.0);
        assert_eq!(report.crash, 0.0); // No crashes recorded
        assert_eq!(report.slow_disk, 0.0);
        assert_eq!(report.drop, 0.0);
        assert_eq!(report.delay, 0.0);
    }

    #[test]
    fn test_module_level_functions() {
        reset_fault_registry();

        record_fault_attempted("test.fault");
        record_fault_applied("test.fault");
        record_fault_observed("test.fault");

        let registry = get_fault_registry();
        assert_eq!(registry.get_attempted("test.fault"), 1);
        assert_eq!(registry.get_applied("test.fault"), 1);
        assert_eq!(registry.get_observed("test.fault"), 1);
        assert_eq!(registry.effectiveness("test.fault"), 100.0);
    }
}
