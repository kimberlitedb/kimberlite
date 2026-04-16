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
            // The library-level run_simulation invokes the kernel via VSR state
            // transitions. At least some properties should fire.
        }
        VoprResult::InvariantViolation { property_report, invariant, .. } => {
            println!("Unexpected violation: {invariant}");
            let report = property_report.expect("property report should be populated");
            println!("Property report: {}", report.summary_line());
        }
    }
}
