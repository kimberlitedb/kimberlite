//! Unified coverage reporting for fault points, invariants, and phases.

use super::fault_registry::FaultRegistry;
use super::phase_tracker::PhaseTracker;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Comprehensive coverage report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Fault point coverage
    pub fault_points: FaultPointCoverage,
    
    /// Phase coverage
    pub phases: PhaseCoverage,
    
    /// Invariant execution counts
    pub invariants: InvariantCoverage,
}

/// Fault point coverage metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultPointCoverage {
    pub total: usize,
    pub hit: usize,
    pub coverage_percent: f64,
    pub fault_points: HashMap<String, u64>,
}

/// Phase coverage metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseCoverage {
    pub total_events: usize,
    pub unique_phases: usize,
    pub phase_counts: HashMap<String, u64>,
}

/// Invariant execution coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantCoverage {
    pub total: usize,
    pub executed: usize,
    pub coverage_percent: f64,
    pub invariant_counts: HashMap<String, u64>,
}

impl CoverageReport {
    /// Generate a coverage report from current state.
    pub fn generate(
        fault_registry: &FaultRegistry,
        phase_tracker: &PhaseTracker,
        invariant_counts: HashMap<String, u64>,
    ) -> Self {
        // Fault point coverage
        let (hit, total, coverage_percent) = fault_registry.coverage();
        let fault_points = FaultPointCoverage {
            total,
            hit,
            coverage_percent,
            fault_points: fault_registry.all_fault_points().clone(),
        };
        
        // Phase coverage
        let phase_counts = phase_tracker.all_phase_counts().clone();
        let phases = PhaseCoverage {
            total_events: phase_tracker.all_events().len(),
            unique_phases: phase_counts.len(),
            phase_counts,
        };
        
        // Invariant coverage
        let total_invariants = invariant_counts.len();
        let executed_invariants = invariant_counts.values().filter(|&&count| count > 0).count();
        let invariant_coverage_percent = if total_invariants == 0 {
            100.0
        } else {
            (executed_invariants as f64 / total_invariants as f64) * 100.0
        };
        
        let invariants = InvariantCoverage {
            total: total_invariants,
            executed: executed_invariants,
            coverage_percent: invariant_coverage_percent,
            invariant_counts,
        };
        
        CoverageReport {
            fault_points,
            phases,
            invariants,
        }
    }
    
    /// Check if coverage meets thresholds.
    pub fn meets_thresholds(
        &self,
        min_fault_coverage: Option<f64>,
        min_invariant_coverage: Option<f64>,
    ) -> Result<(), Vec<String>> {
        let mut failures = Vec::new();

        if let Some(min_fault) = min_fault_coverage {
            if self.fault_points.coverage_percent < min_fault {
                failures.push(format!(
                    "Fault point coverage {:.1}% below threshold {:.1}%",
                    self.fault_points.coverage_percent,
                    min_fault
                ));
            }
        }

        if let Some(min_invariant) = min_invariant_coverage {
            if self.invariants.coverage_percent < min_invariant {
                failures.push(format!(
                    "Invariant coverage {:.1}% below threshold {:.1}%",
                    self.invariants.coverage_percent,
                    min_invariant
                ));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures)
        }
    }

    /// Format the coverage report as human-readable text.
    pub fn to_human_readable(&self) -> String {
        let mut output = String::new();

        output.push_str("Coverage Report:\n");
        output.push_str("======================================\n");

        // Fault point coverage
        output.push_str(&format!(
            "  Fault Points: {}/{} ({:.1}%)\n",
            self.fault_points.hit,
            self.fault_points.total,
            self.fault_points.coverage_percent
        ));

        // Invariant coverage
        output.push_str(&format!(
            "  Invariants:   {}/{} ({:.1}%)\n",
            self.invariants.executed,
            self.invariants.total,
            self.invariants.coverage_percent
        ));

        // Phase coverage
        output.push_str(&format!(
            "  Phases:       {} unique phases, {} total events\n",
            self.phases.unique_phases,
            self.phases.total_events
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_coverage_report_empty() {
        let fault_registry = FaultRegistry::new();
        let phase_tracker = PhaseTracker::new();
        let invariant_counts = HashMap::new();
        
        let report = CoverageReport::generate(&fault_registry, &phase_tracker, invariant_counts);
        
        assert_eq!(report.fault_points.total, 0);
        assert_eq!(report.fault_points.coverage_percent, 100.0);
        assert_eq!(report.invariants.total, 0);
        assert_eq!(report.invariants.coverage_percent, 100.0);
    }
    
    #[test]
    fn test_coverage_thresholds() {
        let fault_registry = FaultRegistry::new();
        let phase_tracker = PhaseTracker::new();
        let invariant_counts = HashMap::new();
        
        let report = CoverageReport::generate(&fault_registry, &phase_tracker, invariant_counts);
        
        // Empty report should pass any threshold
        assert!(report.meets_thresholds(Some(80.0), Some(100.0)).is_ok());
    }
}
