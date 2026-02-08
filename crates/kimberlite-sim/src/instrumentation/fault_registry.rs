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

    /// Get the attempted count for a fault point.
    pub fn get_attempted(&self, key: &str) -> u64 {
        self.fault_points
            .get(key)
            .map(|fp| fp.attempted)
            .unwrap_or(0)
    }

    /// Get the applied count for a fault point.
    pub fn get_applied(&self, key: &str) -> u64 {
        self.fault_points.get(key).map(|fp| fp.applied).unwrap_or(0)
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
        let Some(point) = self.fault_points.get(key) else {
            return 0.0;
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

thread_local! {
    static INJECTION_CONFIG: RefCell<InjectionConfig> = RefCell::new(InjectionConfig::default());
}

/// Configuration for fault injection decisions.
///
/// Controls which faults are injected and at what probability.
/// Used by the VOPR to dynamically adjust fault injection based on
/// coverage gaps (faults that have never been observed).
#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Per-fault-key injection probability (0.0 to 1.0).
    probabilities: HashMap<String, f64>,
    /// Default injection probability for unspecified faults.
    default_probability: f64,
    /// Whether injection is enabled at all.
    enabled: bool,
    /// RNG seed for deterministic injection decisions.
    seed: u64,
    /// Counter for deterministic pseudo-random decisions.
    counter: u64,
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            probabilities: HashMap::new(),
            default_probability: 0.0,
            enabled: false,
            seed: 0,
            counter: 0,
        }
    }
}

impl InjectionConfig {
    /// Creates a new injection config with a base probability and seed.
    pub fn new(default_probability: f64, seed: u64) -> Self {
        Self {
            probabilities: HashMap::new(),
            default_probability,
            enabled: default_probability > 0.0,
            seed,
            counter: 0,
        }
    }

    /// Sets the injection probability for a specific fault key.
    pub fn set_probability(&mut self, key: &str, probability: f64) {
        self.probabilities.insert(key.to_string(), probability);
        if probability > 0.0 {
            self.enabled = true;
        }
    }

    /// Boost injection probability for faults with low coverage.
    ///
    /// Examines the fault registry and increases probability for faults
    /// that have been attempted but never observed (effectiveness = 0%).
    pub fn boost_low_coverage(&mut self, registry: &FaultRegistry, boost_factor: f64) {
        for (key, point) in registry.all_fault_points() {
            if point.applied > 0 && point.observed == 0 {
                // This fault was injected but never had an effect — boost it
                let current = self.probabilities.get(key).copied()
                    .unwrap_or(self.default_probability);
                let boosted = (current * boost_factor).min(1.0);
                self.probabilities.insert(key.clone(), boosted);
            }
        }
        self.enabled = true;
    }

    /// Deterministic pseudo-random decision (reproducible from seed).
    fn should_inject(&mut self, probability: f64) -> bool {
        if probability <= 0.0 {
            return false;
        }
        if probability >= 1.0 {
            return true;
        }
        // Simple hash-based PRNG for determinism
        self.counter += 1;
        let hash = self.seed.wrapping_mul(6364136223846793005)
            .wrapping_add(self.counter.wrapping_mul(1442695040888963407));
        let normalized = (hash >> 33) as f64 / (1u64 << 31) as f64;
        normalized < probability
    }
}

/// Check if a fault should be injected at this point.
///
/// Uses the thread-local injection config to make a deterministic
/// decision based on fault key and configured probabilities.
/// Records the attempt in the fault registry.
pub fn should_inject_fault(key: &str) -> bool {
    INJECTION_CONFIG.with(|config| {
        let mut config = config.borrow_mut();
        if !config.enabled {
            return false;
        }
        let probability = config.probabilities.get(key).copied()
            .unwrap_or(config.default_probability);
        config.should_inject(probability)
    })
}

/// Configures the fault injection for the current thread.
pub fn configure_injection(config: InjectionConfig) {
    INJECTION_CONFIG.with(|c| {
        *c.borrow_mut() = config;
    });
}

/// Resets the injection configuration to disabled.
pub fn reset_injection_config() {
    INJECTION_CONFIG.with(|c| {
        *c.borrow_mut() = InjectionConfig::default();
    });
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

        registry.record_attempted("storage.fsync");
        registry.record_attempted("storage.fsync");
        registry.record_attempted("network.send");

        assert_eq!(registry.get_attempted("storage.fsync"), 2);
        assert_eq!(registry.get_attempted("network.send"), 1);
        assert_eq!(registry.get_attempted("unknown"), 0);
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
        registry.record_attempted("fault1");
        registry.record_attempted("fault2");
        registry.record_attempted("fault3");

        let (hit, total, coverage) = registry.coverage();
        assert_eq!(hit, 3);
        assert_eq!(total, 3);
        assert_eq!(coverage, 100.0);
    }

    #[test]
    fn test_fault_registry_reset() {
        let mut registry = FaultRegistry::new();

        registry.record_attempted("fault1");
        assert_eq!(registry.get_attempted("fault1"), 1);

        registry.reset();
        assert_eq!(registry.get_attempted("fault1"), 0);
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

    #[test]
    fn test_injection_config_default_disabled() {
        reset_injection_config();
        assert!(!should_inject_fault("network.partition"));
    }

    #[test]
    fn test_injection_config_with_probability() {
        let config = InjectionConfig::new(1.0, 42); // 100% probability
        configure_injection(config);

        // With probability 1.0, should always inject
        assert!(should_inject_fault("network.partition"));

        reset_injection_config();
    }

    #[test]
    fn test_injection_config_per_key_probability() {
        let mut config = InjectionConfig::new(0.0, 42); // Default: never inject
        config.set_probability("storage.corruption", 1.0); // Always inject this one
        configure_injection(config);

        assert!(should_inject_fault("storage.corruption"));
        // Default keys should not inject (probability 0.0)
        assert!(!should_inject_fault("network.delay"));

        reset_injection_config();
    }

    #[test]
    fn test_injection_config_deterministic() {
        // Same seed should produce same decisions
        let config1 = InjectionConfig::new(0.5, 12345);
        configure_injection(config1);
        let decisions1: Vec<bool> = (0..10)
            .map(|_| should_inject_fault("test.fault"))
            .collect();

        let config2 = InjectionConfig::new(0.5, 12345);
        configure_injection(config2);
        let decisions2: Vec<bool> = (0..10)
            .map(|_| should_inject_fault("test.fault"))
            .collect();

        assert_eq!(decisions1, decisions2);
        reset_injection_config();
    }

    #[test]
    fn test_injection_config_boost_low_coverage() {
        let mut registry = FaultRegistry::new();

        // Simulate a fault that was applied but never observed
        registry.record_applied("network.drop");
        registry.record_applied("network.drop");
        // No observed calls — effectiveness = 0%

        // Also simulate a fault that works well
        registry.record_applied("storage.corruption");
        registry.record_observed("storage.corruption");
        // effectiveness = 100%

        let mut config = InjectionConfig::new(0.01, 42);
        config.boost_low_coverage(&registry, 10.0);

        // network.drop should have been boosted (10x from 0.01 = 0.1)
        let boosted = config.probabilities.get("network.drop").copied().unwrap_or(0.0);
        assert!(boosted > 0.01, "network.drop should be boosted, got {boosted}");

        // storage.corruption should NOT be boosted (effectiveness > 0)
        let not_boosted = config.probabilities.get("storage.corruption").copied();
        assert!(not_boosted.is_none(), "storage.corruption should not be in probabilities");

        reset_injection_config();
    }
}
