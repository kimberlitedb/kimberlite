//! Tests for instrumentation infrastructure.

use super::coverage::CoverageReport;
use super::fault_registry::{
    get_fault_coverage, init_fault_registry, register_fault_point, set_global_seed,
};

#[test]
fn test_fault_registry_basic() {
    init_fault_registry(12345, 0);

    // Register some fault points
    register_fault_point("test.point1");
    register_fault_point("test.point1"); // Hit twice
    register_fault_point("test.point2");

    // Get coverage
    let coverage = get_fault_coverage();

    assert_eq!(coverage.get("test.point1"), Some(&2));
    assert_eq!(coverage.get("test.point2"), Some(&1));
}

#[test]
fn test_coverage_report() {
    let mut fault_points = std::collections::HashMap::new();
    fault_points.insert("sim.storage.write".to_string(), 100);
    fault_points.insert("sim.storage.read".to_string(), 50);
    fault_points.insert("sim.storage.fsync".to_string(), 10);

    let report = CoverageReport::from_fault_points(fault_points);

    assert_eq!(report.summary.total_fault_points, 3);
    assert_eq!(report.summary.fault_points_hit, 3);
    assert!((report.summary.fault_point_coverage_percent - 100.0).abs() < 0.01);

    // Verify fault point details
    assert_eq!(
        report.fault_points.get("sim.storage.write").unwrap().hit_count,
        100
    );
    assert_eq!(
        report.fault_points.get("sim.storage.read").unwrap().hit_count,
        50
    );
    assert_eq!(
        report.fault_points.get("sim.storage.fsync").unwrap().hit_count,
        10
    );
}

#[test]
fn test_coverage_report_json_serialization() {
    let mut fault_points = std::collections::HashMap::new();
    fault_points.insert("test.point".to_string(), 5);

    let report = CoverageReport::from_fault_points(fault_points);
    let json = report.to_json().expect("should serialize to JSON");

    assert!(json.contains("\"fault_points\""));
    assert!(json.contains("\"summary\""));
    assert!(json.contains("\"hit_count\": 5"));
}

#[test]
fn test_coverage_report_human_readable() {
    let mut fault_points = std::collections::HashMap::new();
    fault_points.insert("test.point1".to_string(), 10);
    fault_points.insert("test.point2".to_string(), 20);

    let report = CoverageReport::from_fault_points(fault_points);
    let output = report.to_human_readable();

    assert!(output.contains("=== Coverage Report ==="));
    assert!(output.contains("Fault Points: 2/2"));
    assert!(output.contains("test.point1 - hits: 10"));
    assert!(output.contains("test.point2 - hits: 20"));
}

#[test]
fn test_deterministic_fault_registry() {
    // Same seed should produce same behavior
    set_global_seed(54321);

    register_fault_point("deterministic.test");
    let coverage1 = get_fault_coverage();

    set_global_seed(54321); // Reset to same seed
    let coverage2 = get_fault_coverage();

    // Coverage should be the same (deterministic)
    assert_eq!(coverage1, coverage2);
}
