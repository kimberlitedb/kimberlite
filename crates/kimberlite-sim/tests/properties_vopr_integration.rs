//! Integration test: verify VOPR captures property reports.
use kimberlite_sim::vopr::{VoprConfig, VoprResult, VoprRunner};

#[test]
fn vopr_captures_property_report() {
    let config = VoprConfig {
        seed: 42,
        iterations: 1,
        // Longer horizon so the driver has room to run several VSR rounds +
        // forced view changes per seed.
        max_events: 20_000,
        max_time_ns: 30_000_000_000,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let result = runner.run_single(42);

    let report = match result {
        VoprResult::Success {
            property_report, ..
        } => property_report.expect("property report should be populated"),
        VoprResult::InvariantViolation {
            property_report,
            invariant,
            ..
        } => {
            let report = property_report.expect("property report should be populated");
            panic!(
                "Unexpected simulation invariant violation: {invariant}. Property report: {}",
                report.summary_line()
            );
        }
    };

    println!("Property report: {}", report.summary_line());
    println!("Total properties observed: {}", report.total_properties);

    // Phase 1.1 baseline: 7 kernel annotations + crypto SOMETIMES.
    // Phase 1.2 adds VSR: ≥10 vsr.* annotations via prepare/commit rounds
    // plus scheduled view changes.
    assert_eq!(
        report.always_violations, 0,
        "no ALWAYS annotations should violate in a clean run; violated ids: {:?}",
        report.violated_ids
    );

    // Inspect the registry directly to count categorised annotations — the
    // report struct hides satisfied SOMETIMES counts behind an aggregate.
    let snap = kimberlite_properties::registry::snapshot();
    let vsr_ids: Vec<&String> = snap.keys().filter(|id| id.starts_with("vsr.")).collect();
    let kernel_ids: Vec<&String> = snap.keys().filter(|id| id.starts_with("kernel.")).collect();

    println!("kernel.* observed: {kernel_ids:?}");
    println!("vsr.* observed: {vsr_ids:?}");
    println!("SOMETIMES satisfied: {:?}", report.satisfied_sometimes_ids);
    println!(
        "SOMETIMES unsatisfied: {:?}",
        report.unsatisfied_sometimes_ids
    );

    assert!(
        kernel_ids.len() >= 7,
        "expected ≥7 kernel.* annotations to fire; got {}: {:?}",
        kernel_ids.len(),
        kernel_ids
    );
    assert!(
        vsr_ids.len() >= 10,
        "expected ≥10 vsr.* annotations to fire in Phase 1.2; got {}: {:?}",
        vsr_ids.len(),
        vsr_ids
    );

    // Phase 1.3: compliance suite fires 35+ annotations (17 reached! audit,
    // 2 consent, 5 erasure, 5 breach, 6 export, 2 audit invariants).
    let compliance_ids: Vec<&String> = snap
        .keys()
        .filter(|id| id.starts_with("compliance."))
        .collect();
    assert!(
        compliance_ids.len() >= 25,
        "expected ≥25 compliance.* annotations to fire in Phase 1.3; got {}: {:?}",
        compliance_ids.len(),
        compliance_ids
    );

    // Phase 1.4: query suite fires ≥8 query.* annotations (schema widths,
    // BETWEEN, LIKE, CASE, JOIN, GROUP BY, SUM overflow guard, AVG guard).
    let query_ids: Vec<&String> = snap.keys().filter(|id| id.starts_with("query.")).collect();
    assert!(
        query_ids.len() >= 8,
        "expected ≥8 query.* annotations to fire in Phase 1.4; got {}: {:?}",
        query_ids.len(),
        query_ids
    );

    // Cumulative target: after storage side-car lands we aim for ≥65
    // distinct annotations per seed (kernel 7 + vsr 10 + compliance 25+ +
    // query 10 + storage 7 + crypto 1 ≈ 60–67).
    assert!(
        report.total_properties >= 65,
        "expected ≥65 total annotations; got {}",
        report.total_properties
    );

    let storage_ids: Vec<&String> = snap
        .keys()
        .filter(|id| id.starts_with("storage."))
        .collect();
    assert!(
        storage_ids.len() >= 6,
        "expected ≥6 storage.* annotations; got {}: {:?}",
        storage_ids.len(),
        storage_ids
    );
}
