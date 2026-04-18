//! Default property layer — Antithesis-style "always-on" properties.
//!
//! Antithesis ships a set of preconfigured properties (never crash, never OOM,
//! never emit unexpected errors, software built for the right platform) that
//! apply to every test run regardless of the scenario. Our equivalent runs
//! before, during, and after the scenario's custom invariant checkers and
//! reports violations uniformly in the final scenario output.
//!
//! # Usage
//!
//! ```ignore
//! let mut props = DefaultProperties::new();
//!
//! // Wrap the scenario driver so panics are counted rather than aborting
//! // the whole campaign.
//! let outcome = props.run_guarded(|| run_scenario(seed));
//!
//! // Record per-iteration observations.
//! props.record_log_growth(grew_by_bytes);
//! if let Err(e) = outcome {
//!     props.record_driver_error(e);
//! }
//!
//! let report = props.report();
//! if !report.all_passed() {
//!     eprintln!("default property violations: {:?}", report);
//! }
//! ```

use std::panic::{AssertUnwindSafe, catch_unwind};

/// A budget for bounded-log-growth checks. If a single iteration grows the
/// simulated log by more than this many bytes, the `bounded_log_growth`
/// property fails. Covers runaway leader/no-op loops, infinite retry storms,
/// and state-machine bugs that synthesise unbounded commands.
pub const DEFAULT_LOG_GROWTH_BUDGET_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB/iter

/// Default properties checked on every scenario run.
#[derive(Debug, Default)]
pub struct DefaultProperties {
    /// Whether the driver panicked during `run_guarded`. Once set true, never
    /// resets — a panic anywhere in the campaign is a failure.
    panicked: bool,
    /// The panic message if the driver panicked.
    panic_message: Option<String>,
    /// Driver-returned errors the caller has flagged as unexpected (i.e. not
    /// the expected `InvariantViolation` for an injected-attack scenario).
    unexpected_errors: Vec<String>,
    /// Per-iteration log growth observations (bytes). Used to flag runs that
    /// breach `DEFAULT_LOG_GROWTH_BUDGET_BYTES` in a single iteration.
    log_growth_budget_exceeded: u64,
    /// Maximum single-iteration log growth seen (for diagnostics).
    peak_log_growth_bytes: u64,
    /// Total iterations observed (for ratio calculations in the report).
    iterations_observed: u64,
}

impl DefaultProperties {
    /// Creates a new set of default properties in the all-pass initial state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `f` under `catch_unwind`. If `f` panics, the panic is recorded and
    /// the error is returned so the campaign can continue. Use this to wrap
    /// scenario drivers so one bad seed doesn't abort a batch.
    pub fn run_guarded<F, T>(&mut self, f: F) -> Result<T, String>
    where
        F: FnOnce() -> T,
    {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(v) => Ok(v),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&'static str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                self.panicked = true;
                self.panic_message = Some(msg.clone());
                Err(msg)
            }
        }
    }

    /// Records a driver-returned error the caller considers unexpected.
    ///
    /// Scenario runners should filter *expected* errors (e.g. injected-attack
    /// scenarios that intentionally violate an invariant) before calling this —
    /// the default-property layer only flags errors that should never occur
    /// under any scenario.
    pub fn record_driver_error(&mut self, err: impl std::fmt::Display) {
        self.unexpected_errors.push(err.to_string());
    }

    /// Records a single-iteration log growth observation (bytes).
    ///
    /// Call at the end of each iteration. If `bytes` exceeds
    /// `DEFAULT_LOG_GROWTH_BUDGET_BYTES`, `bounded_log_growth` fails for this
    /// run.
    pub fn record_log_growth(&mut self, bytes: u64) {
        self.iterations_observed += 1;
        if bytes > self.peak_log_growth_bytes {
            self.peak_log_growth_bytes = bytes;
        }
        if bytes > DEFAULT_LOG_GROWTH_BUDGET_BYTES {
            self.log_growth_budget_exceeded += 1;
        }
    }

    /// Builds a report summarising which default properties passed.
    pub fn report(&self) -> DefaultPropertyReport {
        DefaultPropertyReport {
            never_panicked: !self.panicked,
            no_unexpected_errors: self.unexpected_errors.is_empty(),
            bounded_log_growth: self.log_growth_budget_exceeded == 0,
            panic_message: self.panic_message.clone(),
            unexpected_errors: self.unexpected_errors.clone(),
            log_growth_budget_exceeded_count: self.log_growth_budget_exceeded,
            peak_log_growth_bytes: self.peak_log_growth_bytes,
            iterations_observed: self.iterations_observed,
        }
    }
}

/// Snapshot of default-property outcomes at the end of a scenario run.
#[derive(Debug, Clone)]
pub struct DefaultPropertyReport {
    /// No panic occurred inside `run_guarded`.
    pub never_panicked: bool,
    /// No driver errors were flagged as unexpected.
    pub no_unexpected_errors: bool,
    /// No iteration exceeded `DEFAULT_LOG_GROWTH_BUDGET_BYTES`.
    pub bounded_log_growth: bool,
    /// Panic message if one occurred.
    pub panic_message: Option<String>,
    /// Unexpected driver errors (stringified).
    pub unexpected_errors: Vec<String>,
    /// How many iterations breached the log-growth budget.
    pub log_growth_budget_exceeded_count: u64,
    /// Largest single-iteration log growth observed.
    pub peak_log_growth_bytes: u64,
    /// How many iterations were observed via `record_log_growth`.
    pub iterations_observed: u64,
}

impl DefaultPropertyReport {
    /// True if every default property passed.
    pub fn all_passed(&self) -> bool {
        self.never_panicked && self.no_unexpected_errors && self.bounded_log_growth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_report_is_all_pass() {
        let props = DefaultProperties::new();
        let report = props.report();
        assert!(report.all_passed());
    }

    #[test]
    fn run_guarded_catches_panic_and_continues() {
        let mut props = DefaultProperties::new();

        let err = props
            .run_guarded(|| panic!("boom"))
            .expect_err("panic should surface as Err");
        assert!(err.contains("boom"));

        // Subsequent guarded calls still work.
        let ok: i32 = props.run_guarded(|| 42).expect("no panic");
        assert_eq!(ok, 42);

        let report = props.report();
        assert!(!report.never_panicked);
        assert!(report.panic_message.as_ref().unwrap().contains("boom"));
        assert!(!report.all_passed());
    }

    #[test]
    fn panic_with_owned_string_is_captured() {
        let mut props = DefaultProperties::new();
        let _ = props.run_guarded(|| panic!("{}", String::from("owned-payload")));
        assert!(
            props
                .report()
                .panic_message
                .unwrap()
                .contains("owned-payload")
        );
    }

    #[test]
    fn record_driver_error_fails_property() {
        let mut props = DefaultProperties::new();
        props.record_driver_error("network exploded");
        let report = props.report();
        assert!(!report.no_unexpected_errors);
        assert_eq!(report.unexpected_errors.len(), 1);
    }

    #[test]
    fn bounded_log_growth_passes_below_budget() {
        let mut props = DefaultProperties::new();
        props.record_log_growth(1_024);
        props.record_log_growth(2 * 1024 * 1024);
        assert!(props.report().bounded_log_growth);
    }

    #[test]
    fn bounded_log_growth_fails_above_budget() {
        let mut props = DefaultProperties::new();
        props.record_log_growth(DEFAULT_LOG_GROWTH_BUDGET_BYTES + 1);
        let report = props.report();
        assert!(!report.bounded_log_growth);
        assert_eq!(report.log_growth_budget_exceeded_count, 1);
        assert_eq!(report.peak_log_growth_bytes, DEFAULT_LOG_GROWTH_BUDGET_BYTES + 1);
    }
}
