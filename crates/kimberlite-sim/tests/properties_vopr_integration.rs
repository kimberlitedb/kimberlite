//! Integration test: verify VOPR captures property reports.
use kimberlite_sim::vopr::{VoprConfig, VoprRunner, VoprResult};

#[test]
fn vopr_captures_property_report() {
    let config = VoprConfig {
        seed: 42,
        iterations: 1,
        max_events: 1000,
        max_time_ns: 1_000_000_000,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let result = runner.run_single(42);

    match result {
        VoprResult::Success { property_report, .. } => {
            let report = property_report.expect("property report should be populated");
            println!("Property report: {}", report.summary_line());
            println!("Total properties observed: {}", report.total_properties);
            // Phase 1.1 baseline: the RealStateDriver drives kernel commands,
            // firing the 6 kernel ALWAYS annotations plus kernel.multi_event_batch
            // (SOMETIMES). Crypto annotations add one more SOMETIMES. With
            // subsequent phases wiring VSR/compliance/query, this floor rises.
            assert!(
                report.total_properties >= 7,
                "expected at least 7 kernel/crypto annotations to fire; got {}: {}",
                report.total_properties,
                report.summary_line()
            );
            assert_eq!(
                report.always_violations, 0,
                "no ALWAYS annotations should violate in a clean run; violated ids: {:?}",
                report.violated_ids
            );
        }
        VoprResult::InvariantViolation { property_report, invariant, .. } => {
            let report = property_report.expect("property report should be populated");
            panic!(
                "Unexpected simulation invariant violation: {invariant}. Property report: {}",
                report.summary_line()
            );
        }
    }
}
