//! Coverage thresholds and validation for VOPR runs.
//!
//! Defines mandatory minimum coverage levels and validates that VOPR runs
//! achieve sufficient coverage across:
//! - Fault injection points
//! - Invariant checkers
//! - System phases (view changes, repairs, checkpoints)
//! - Query plan diversity

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Coverage thresholds that VOPR runs must meet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageThresholds {
    /// Minimum percentage of fault points that must be hit (0.0 - 1.0)
    pub fault_point_coverage_min: f64,

    /// Fault points that MUST be hit (100% required)
    pub critical_fault_points: Vec<String>,

    /// Invariants that must execute at least once
    pub required_invariants: Vec<String>,

    /// Minimum number of view changes in long runs
    pub min_view_changes: usize,

    /// Minimum number of repair operations in long runs
    pub min_repairs: usize,

    /// Minimum number of unique query plans
    pub min_unique_query_plans: usize,

    /// Minimum number of phase events
    pub min_phase_events: HashMap<String, usize>,
}

impl Default for CoverageThresholds {
    fn default() -> Self {
        Self {
            fault_point_coverage_min: 0.8, // 80% of fault points
            critical_fault_points: vec![
                "sim.storage.fsync".to_string(),
                "sim.storage.write".to_string(),
                "sim.storage.read".to_string(),
                "sim.network.send".to_string(),
            ],
            required_invariants: vec![
                "linearizability".to_string(),
                "hash_chain".to_string(),
                "replica_consistency".to_string(),
                "storage_determinism".to_string(),
                "vsr_agreement".to_string(),
                "projection_applied_position_monotonic".to_string(),
            ],
            min_view_changes: 1,
            min_repairs: 0,
            min_unique_query_plans: 5,
            min_phase_events: [
                ("vsr.prepare_sent".to_string(), 10),
                ("vsr.commit_broadcast".to_string(), 5),
                ("storage.fsync_complete".to_string(), 10),
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl CoverageThresholds {
    /// Creates thresholds for quick smoke tests (lower requirements).
    pub fn smoke_test() -> Self {
        Self {
            fault_point_coverage_min: 0.5, // 50%
            critical_fault_points: vec![
                "sim.storage.fsync".to_string(),
                "sim.network.send".to_string(),
            ],
            required_invariants: vec![
                "linearizability".to_string(),
                "hash_chain".to_string(),
            ],
            min_view_changes: 0,
            min_repairs: 0,
            min_unique_query_plans: 1,
            min_phase_events: HashMap::new(),
        }
    }

    /// Creates thresholds for long-running nightly tests (higher requirements).
    pub fn nightly() -> Self {
        Self {
            fault_point_coverage_min: 0.9, // 90%
            critical_fault_points: vec![
                "sim.storage.fsync".to_string(),
                "sim.storage.write".to_string(),
                "sim.storage.read".to_string(),
                "sim.storage.crash".to_string(),
                "sim.network.send".to_string(),
                "sim.network.deliver".to_string(),
                "sim.network.partition".to_string(),
            ],
            required_invariants: vec![
                "linearizability".to_string(),
                "hash_chain".to_string(),
                "replica_consistency".to_string(),
                "storage_determinism".to_string(),
                "vsr_agreement".to_string(),
                "vsr_prefix_property".to_string(),
                "vsr_view_change_safety".to_string(),
                "vsr_recovery_safety".to_string(),
                "projection_applied_position_monotonic".to_string(),
                "projection_mvcc_visibility".to_string(),
                "projection_applied_index_integrity".to_string(),
                "sql_tlp_partitioning".to_string(),
                "sql_norec_equivalence".to_string(),
            ],
            min_view_changes: 5,
            min_repairs: 2,
            min_unique_query_plans: 20,
            min_phase_events: [
                ("vsr.prepare_sent".to_string(), 100),
                ("vsr.commit_broadcast".to_string(), 50),
                ("vsr.view_change_complete".to_string(), 5),
                ("storage.fsync_complete".to_string(), 100),
                ("storage.repair_complete".to_string(), 2),
            ]
            .into_iter()
            .collect(),
        }
    }
}

/// Actual coverage achieved in a VOPR run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Total number of fault points registered
    pub total_fault_points: usize,

    /// Number of fault points that were hit
    pub fault_points_hit: usize,

    /// Fault points that were hit at least once
    pub hit_fault_points: Vec<String>,

    /// Fault points that were never hit
    pub missed_fault_points: Vec<String>,

    /// Invariants that executed (name -> execution count)
    pub invariant_executions: HashMap<String, usize>,

    /// Invariants that never executed
    pub missed_invariants: Vec<String>,

    /// Number of view changes that occurred
    pub view_changes: usize,

    /// Number of repair operations
    pub repairs: usize,

    /// Number of unique query plans executed
    pub unique_query_plans: usize,

    /// Phase events that occurred (name -> count)
    pub phase_events: HashMap<String, usize>,

    /// Total events processed
    pub total_events: u64,

    /// Total simulation time (nanoseconds)
    pub simulation_time_ns: u64,
}

impl CoverageReport {
    /// Creates an empty coverage report.
    pub fn new() -> Self {
        Self {
            total_fault_points: 0,
            fault_points_hit: 0,
            hit_fault_points: Vec::new(),
            missed_fault_points: Vec::new(),
            invariant_executions: HashMap::new(),
            missed_invariants: Vec::new(),
            view_changes: 0,
            repairs: 0,
            unique_query_plans: 0,
            phase_events: HashMap::new(),
            total_events: 0,
            simulation_time_ns: 0,
        }
    }

    /// Computes fault point coverage percentage.
    pub fn fault_point_coverage(&self) -> f64 {
        if self.total_fault_points == 0 {
            return 0.0;
        }
        self.fault_points_hit as f64 / self.total_fault_points as f64
    }
}

impl Default for CoverageReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of validating coverage against thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageValidationResult {
    /// Whether coverage meets all thresholds
    pub passed: bool,

    /// Violations found (empty if passed)
    pub violations: Vec<CoverageViolation>,

    /// Warnings (non-critical issues)
    pub warnings: Vec<String>,
}

/// A specific coverage violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageViolation {
    /// Type of violation
    pub kind: ViolationKind,

    /// Human-readable message
    pub message: String,

    /// Expected value
    pub expected: String,

    /// Actual value
    pub actual: String,
}

/// Types of coverage violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationKind {
    /// Fault point coverage too low
    FaultPointCoverage,

    /// Critical fault point not hit
    CriticalFaultPointMissed,

    /// Required invariant did not execute
    InvariantNotExecuted,

    /// Not enough view changes
    InsufficientViewChanges,

    /// Not enough repairs
    InsufficientRepairs,

    /// Not enough unique query plans
    InsufficientQueryPlans,

    /// Required phase event count too low
    InsufficientPhaseEvents,
}

/// Validates coverage against thresholds.
pub fn validate_coverage(
    report: &CoverageReport,
    thresholds: &CoverageThresholds,
) -> CoverageValidationResult {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    // Check 1: Overall fault point coverage
    let fault_coverage = report.fault_point_coverage();
    if fault_coverage < thresholds.fault_point_coverage_min {
        violations.push(CoverageViolation {
            kind: ViolationKind::FaultPointCoverage,
            message: "Fault point coverage below threshold".to_string(),
            expected: format!("{:.1}%", thresholds.fault_point_coverage_min * 100.0),
            actual: format!("{:.1}%", fault_coverage * 100.0),
        });
    }

    // Check 2: Critical fault points
    for critical_fp in &thresholds.critical_fault_points {
        if !report.hit_fault_points.contains(critical_fp) {
            violations.push(CoverageViolation {
                kind: ViolationKind::CriticalFaultPointMissed,
                message: format!("Critical fault point '{}' was never hit", critical_fp),
                expected: "Hit at least once".to_string(),
                actual: "Never hit".to_string(),
            });
        }
    }

    // Check 3: Required invariants
    for required_inv in &thresholds.required_invariants {
        let exec_count = report.invariant_executions.get(required_inv).copied().unwrap_or(0);
        if exec_count == 0 {
            violations.push(CoverageViolation {
                kind: ViolationKind::InvariantNotExecuted,
                message: format!("Required invariant '{}' never executed", required_inv),
                expected: "Executed at least once".to_string(),
                actual: "Never executed".to_string(),
            });
        } else if exec_count < 10 {
            warnings.push(format!(
                "Invariant '{}' executed only {} times (may indicate low coverage)",
                required_inv, exec_count
            ));
        }
    }

    // Check 4: View changes (only for long runs)
    if thresholds.min_view_changes > 0 && report.view_changes < thresholds.min_view_changes {
        violations.push(CoverageViolation {
            kind: ViolationKind::InsufficientViewChanges,
            message: "Not enough view changes occurred".to_string(),
            expected: format!("At least {}", thresholds.min_view_changes),
            actual: format!("{}", report.view_changes),
        });
    }

    // Check 5: Repairs (only for long runs)
    if thresholds.min_repairs > 0 && report.repairs < thresholds.min_repairs {
        violations.push(CoverageViolation {
            kind: ViolationKind::InsufficientRepairs,
            message: "Not enough repair operations occurred".to_string(),
            expected: format!("At least {}", thresholds.min_repairs),
            actual: format!("{}", report.repairs),
        });
    }

    // Check 6: Query plan diversity
    if report.unique_query_plans < thresholds.min_unique_query_plans {
        violations.push(CoverageViolation {
            kind: ViolationKind::InsufficientQueryPlans,
            message: "Not enough unique query plans exercised".to_string(),
            expected: format!("At least {}", thresholds.min_unique_query_plans),
            actual: format!("{}", report.unique_query_plans),
        });
    }

    // Check 7: Phase events
    for (phase, min_count) in &thresholds.min_phase_events {
        let actual_count = report.phase_events.get(phase).copied().unwrap_or(0);
        if actual_count < *min_count {
            violations.push(CoverageViolation {
                kind: ViolationKind::InsufficientPhaseEvents,
                message: format!("Phase event '{}' occurred too few times", phase),
                expected: format!("At least {}", min_count),
                actual: format!("{}", actual_count),
            });
        }
    }

    // Warnings for missed non-critical fault points
    if report.missed_fault_points.len() > (report.total_fault_points / 5) {
        warnings.push(format!(
            "Many fault points missed ({}/{}). Consider longer runs or different scenarios.",
            report.missed_fault_points.len(),
            report.total_fault_points
        ));
    }

    CoverageValidationResult {
        passed: violations.is_empty(),
        violations,
        warnings,
    }
}

/// Formats a coverage validation result for display.
pub fn format_validation_result(result: &CoverageValidationResult) -> String {
    let mut output = String::new();

    if result.passed {
        output.push_str("✅ Coverage validation PASSED\n\n");
    } else {
        output.push_str("❌ Coverage validation FAILED\n\n");
    }

    if !result.violations.is_empty() {
        output.push_str("Violations:\n");
        for (i, violation) in result.violations.iter().enumerate() {
            output.push_str(&format!(
                "  {}. {} (expected: {}, actual: {})\n",
                i + 1,
                violation.message,
                violation.expected,
                violation.actual
            ));
        }
        output.push('\n');
    }

    if !result.warnings.is_empty() {
        output.push_str("Warnings:\n");
        for warning in &result.warnings {
            output.push_str(&format!("  ⚠️  {}\n", warning));
        }
        output.push('\n');
    }

    output
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_passing_report() -> CoverageReport {
        let mut report = CoverageReport::new();
        report.total_fault_points = 10;
        report.fault_points_hit = 8; // 80%
        report.hit_fault_points = vec![
            "sim.storage.fsync".to_string(),
            "sim.storage.write".to_string(),
            "sim.storage.read".to_string(),
            "sim.network.send".to_string(),
        ];
        report.invariant_executions = [
            ("linearizability".to_string(), 100),
            ("hash_chain".to_string(), 50),
            ("replica_consistency".to_string(), 30),
            ("storage_determinism".to_string(), 20),
            ("vsr_agreement".to_string(), 15),
            ("projection_applied_position_monotonic".to_string(), 10),
        ]
        .into_iter()
        .collect();
        report.view_changes = 1;
        report.unique_query_plans = 5;
        report.phase_events = [
            ("vsr.prepare_sent".to_string(), 10),
            ("vsr.commit_broadcast".to_string(), 5),
            ("storage.fsync_complete".to_string(), 10),
        ]
        .into_iter()
        .collect();
        report
    }

    #[test]
    fn test_default_thresholds_reasonable() {
        let thresholds = CoverageThresholds::default();
        assert_eq!(thresholds.fault_point_coverage_min, 0.8);
        assert!(thresholds.critical_fault_points.len() >= 3);
        assert!(thresholds.required_invariants.len() >= 5);
    }

    #[test]
    fn test_smoke_test_thresholds_lower() {
        let smoke = CoverageThresholds::smoke_test();
        let default = CoverageThresholds::default();
        assert!(smoke.fault_point_coverage_min < default.fault_point_coverage_min);
        assert!(smoke.required_invariants.len() < default.required_invariants.len());
    }

    #[test]
    fn test_nightly_thresholds_higher() {
        let nightly = CoverageThresholds::nightly();
        let default = CoverageThresholds::default();
        assert!(nightly.fault_point_coverage_min > default.fault_point_coverage_min);
        assert!(nightly.required_invariants.len() > default.required_invariants.len());
        assert!(nightly.min_view_changes > default.min_view_changes);
    }

    #[test]
    fn test_coverage_report_fault_point_percentage() {
        let mut report = CoverageReport::new();
        report.total_fault_points = 10;
        report.fault_points_hit = 8;
        assert_eq!(report.fault_point_coverage(), 0.8);
    }

    #[test]
    fn test_coverage_report_zero_fault_points() {
        let report = CoverageReport::new();
        assert_eq!(report.fault_point_coverage(), 0.0);
    }

    #[test]
    fn test_validate_coverage_passing() {
        let report = minimal_passing_report();
        let thresholds = CoverageThresholds::default();

        let result = validate_coverage(&report, &thresholds);

        assert!(result.passed, "Validation should pass for minimal report");
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_validate_coverage_fault_point_too_low() {
        let mut report = minimal_passing_report();
        report.fault_points_hit = 5; // Only 50%

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(!result.passed);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::FaultPointCoverage));
    }

    #[test]
    fn test_validate_coverage_critical_fault_point_missed() {
        let mut report = minimal_passing_report();
        report.hit_fault_points.retain(|fp| fp != "sim.storage.fsync");

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(!result.passed);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::CriticalFaultPointMissed));
    }

    #[test]
    fn test_validate_coverage_invariant_not_executed() {
        let mut report = minimal_passing_report();
        report.invariant_executions.remove("linearizability");

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(!result.passed);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InvariantNotExecuted));
    }

    #[test]
    fn test_validate_coverage_insufficient_view_changes() {
        let mut report = minimal_passing_report();
        report.view_changes = 0;

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(!result.passed);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InsufficientViewChanges));
    }

    #[test]
    fn test_validate_coverage_insufficient_query_plans() {
        let mut report = minimal_passing_report();
        report.unique_query_plans = 2;

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(!result.passed);
        assert!(result
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::InsufficientQueryPlans));
    }

    #[test]
    fn test_validate_coverage_warnings_for_low_invariant_count() {
        let mut report = minimal_passing_report();
        // Set linearizability to 5 executions (passes threshold of >0 but warns if <10)
        report.invariant_executions.insert("linearizability".to_string(), 5);

        let thresholds = CoverageThresholds::default();
        let result = validate_coverage(&report, &thresholds);

        assert!(result.passed); // Still passes
        assert!(!result.warnings.is_empty()); // But has warnings
    }

    #[test]
    fn test_format_validation_result_passing() {
        let result = CoverageValidationResult {
            passed: true,
            violations: Vec::new(),
            warnings: Vec::new(),
        };

        let output = format_validation_result(&result);
        assert!(output.contains("✅"));
        assert!(output.contains("PASSED"));
    }

    #[test]
    fn test_format_validation_result_failing() {
        let result = CoverageValidationResult {
            passed: false,
            violations: vec![CoverageViolation {
                kind: ViolationKind::FaultPointCoverage,
                message: "Coverage too low".to_string(),
                expected: "80%".to_string(),
                actual: "50%".to_string(),
            }],
            warnings: vec!["Many fault points missed".to_string()],
        };

        let output = format_validation_result(&result);
        assert!(output.contains("❌"));
        assert!(output.contains("FAILED"));
        assert!(output.contains("Coverage too low"));
        assert!(output.contains("Many fault points missed"));
    }

    #[test]
    fn test_coverage_serialization() {
        let report = minimal_passing_report();
        let json = serde_json::to_string(&report).expect("should serialize");
        let deserialized: CoverageReport =
            serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(report.total_fault_points, deserialized.total_fault_points);
        assert_eq!(report.fault_points_hit, deserialized.fault_points_hit);
    }
}
