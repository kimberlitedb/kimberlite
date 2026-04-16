//! Run command for executing VOPR simulations.

use super::{
    Command, CommandError, OutputFormat, ProgressReporter, Verbosity, generate_bundle_filename,
};
use crate::event_log::{EventLog, FailureInfo, ReproBundle};
use crate::scenarios::ScenarioType;
use crate::vopr::{VoprConfig, VoprResult, VoprRunner};
use std::path::PathBuf;

// ============================================================================
// Run Command
// ============================================================================

/// Executes a VOPR simulation run.
#[derive(Debug, Clone)]
pub struct RunCommand {
    /// Scenario to run.
    pub scenario: ScenarioType,

    /// Number of iterations.
    pub iterations: u64,

    /// Optional seed (random if not specified).
    pub seed: Option<u64>,

    /// Output format.
    pub format: OutputFormat,

    /// Verbosity level.
    pub verbosity: Verbosity,

    /// Directory for saving failure bundles.
    pub output_dir: Option<PathBuf>,

    /// Enable event logging.
    pub enable_logging: bool,

    /// Show progress bar.
    pub show_progress: bool,
}

impl Default for RunCommand {
    fn default() -> Self {
        Self {
            scenario: ScenarioType::Baseline,
            iterations: 1000,
            seed: None,
            format: OutputFormat::Human,
            verbosity: Verbosity::Normal,
            output_dir: None,
            enable_logging: false,
            show_progress: true,
        }
    }
}

impl RunCommand {
    /// Creates a new run command with default settings.
    pub fn new(scenario: ScenarioType) -> Self {
        Self {
            scenario,
            ..Default::default()
        }
    }

    /// Sets the number of iterations.
    pub fn with_iterations(mut self, iterations: u64) -> Self {
        self.iterations = iterations;
        self
    }

    /// Sets the seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the output format.
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = format;
        self
    }

    /// Sets the verbosity level.
    pub fn with_verbosity(mut self, verbosity: Verbosity) -> Self {
        self.verbosity = verbosity;
        self
    }

    /// Sets the output directory for failure bundles.
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = Some(dir);
        self
    }

    /// Enables event logging.
    pub fn with_logging(mut self, enable: bool) -> Self {
        self.enable_logging = enable;
        self
    }

    /// Executes the run command.
    #[allow(clippy::unnecessary_wraps)] // Result for future error handling in CLI
    fn run_simulation(&self) -> Result<RunResult, CommandError> {
        let base_seed = self.seed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });

        let config = VoprConfig {
            seed: base_seed,
            iterations: self.iterations,
            scenario: Some(self.scenario),
            network_faults: true,
            storage_faults: true,
            verbose: self.verbosity.at_least(Verbosity::Verbose),
            max_events: 10_000,
            max_time_ns: 60_000_000_000, // 60 seconds
            check_determinism: false,
            enable_trace: self.verbosity.at_least(Verbosity::Debug),
            save_trace_on_failure: true,
            enhanced_workloads: true,
            failure_diagnosis: true,
            workload_ops_per_tick: 5,
            workload_tick_interval_ns: 10_000_000,
            enable_sql_differential: false,
        };

        let runner = VoprRunner::new(config.clone());
        let mut progress = ProgressReporter::new(self.iterations, self.show_progress);

        let mut failures = Vec::new();
        let mut event_log = if self.enable_logging {
            Some(EventLog::new())
        } else {
            None
        };

        let mut property_aggregate = PropertyAggregate::default();

        for i in 0..self.iterations {
            progress.update(i + 1);

            let result = runner.run_single(config.seed + i);

            // Merge property report into the batch aggregate.
            match &result {
                VoprResult::Success {
                    property_report: Some(report),
                    ..
                }
                | VoprResult::InvariantViolation {
                    property_report: Some(report),
                    ..
                } => property_aggregate.merge(report),
                _ => {}
            }

            match result {
                VoprResult::Success { .. } => {}
                VoprResult::InvariantViolation {
                    seed,
                    invariant,
                    message,
                    events_processed,
                    ..
                } => {
                    failures.push(FailureRecord {
                        seed,
                        iteration: i,
                        invariant_name: invariant.clone(),
                        message: message.clone(),
                    });

                    // Create failure bundle if output dir specified
                    if let Some(ref output_dir) = self.output_dir {
                        let bundle = ReproBundle::new(
                            seed,
                            format!("{:?}", self.scenario),
                            event_log.as_ref().map(|log| log.iter().cloned().collect()),
                            FailureInfo {
                                invariant_name: invariant,
                                message,
                                failed_at_event: events_processed,
                                failed_at_time_ns: 0,
                            },
                        );

                        let filename =
                            generate_bundle_filename(seed, &bundle.failure.invariant_name);
                        let bundle_path = output_dir.join(filename);

                        if let Err(e) = bundle.save_to_file(&bundle_path) {
                            eprintln!("Warning: Failed to save bundle: {}", e);
                        } else if self.verbosity.at_least(Verbosity::Verbose) {
                            println!("  Saved bundle: {}", bundle_path.display());
                        }
                    }
                }
            }

            if let Some(ref mut log) = event_log {
                log.clear(); // Clear log between iterations
            }
        }

        progress.finish();

        Ok(RunResult {
            iterations: self.iterations,
            failures,
            scenario: self.scenario,
            properties: property_aggregate,
        })
    }

    /// Prints results based on output format.
    fn print_results(&self, result: &RunResult) {
        match self.format {
            OutputFormat::Human => self.print_human(result),
            OutputFormat::Json => self.print_json(result),
            OutputFormat::Compact => self.print_compact(result),
        }
    }

    fn print_human(&self, result: &RunResult) {
        println!("\n═══════════════════════════════════════════════════════");
        println!("VOPR Simulation Results");
        println!("═══════════════════════════════════════════════════════");
        println!("Scenario: {:?}", result.scenario);
        println!("Iterations: {}", result.iterations);
        println!("Failures: {}", result.failures.len());

        // Property coverage summary
        let props = &result.properties;
        if props.iterations_aggregated > 0 && !props.all_ids.is_empty() {
            println!("\n─── Property Coverage ──────────────────────────────────");
            let sometimes_total =
                props.sometimes_ever_satisfied.len() + props.sometimes_never_satisfied.len();
            let reached_total = props.reached_ever_hit.len() + props.reached_never_hit.len();
            println!(
                "SOMETIMES: {}/{} satisfied in ≥1 iteration",
                props.sometimes_ever_satisfied.len(),
                sometimes_total,
            );
            println!(
                "REACHED:   {}/{} hit in ≥1 iteration",
                props.reached_ever_hit.len(),
                reached_total,
            );
            if !props.always_never_violations.is_empty() {
                println!(
                    "VIOLATIONS: {} ALWAYS/NEVER property IDs violated across batch",
                    props.always_never_violations.len()
                );
                if self.verbosity.at_least(Verbosity::Verbose) {
                    for (id, count) in &props.always_never_violations {
                        println!("  • {id}: {count} violation(s)");
                    }
                }
            }
            if self.verbosity.at_least(Verbosity::Verbose)
                && !props.sometimes_never_satisfied.is_empty()
            {
                println!("\nCoverage gaps (SOMETIMES never satisfied):");
                let mut ids: Vec<_> = props.sometimes_never_satisfied.iter().collect();
                ids.sort();
                for id in ids {
                    println!("  • {id}");
                }
            }
            if self.verbosity.at_least(Verbosity::Verbose)
                && !props.reached_never_hit.is_empty()
            {
                println!("\nCoverage gaps (REACHED never hit):");
                let mut ids: Vec<_> = props.reached_never_hit.iter().collect();
                ids.sort();
                for id in ids {
                    println!("  • {id}");
                }
            }
        }

        if result.failures.is_empty() {
            println!("\n✓ All iterations passed!");
        } else {
            println!("\n✗ Failures detected:");
            for failure in &result.failures {
                println!(
                    "  • Iteration {}: {} (seed: {})",
                    failure.iteration, failure.invariant_name, failure.seed
                );
                if self.verbosity.at_least(Verbosity::Verbose) {
                    println!("    Message: {}", failure.message);
                }
            }

            if let Some(ref output_dir) = self.output_dir {
                println!("\n📦 Failure bundles saved to: {}", output_dir.display());
                println!("   Reproduce with: vopr repro <bundle.kmb>");
            }
        }
    }

    fn print_json(&self, result: &RunResult) {
        let props = &result.properties;
        let mut sometimes_unsatisfied: Vec<&String> = props.sometimes_never_satisfied.iter().collect();
        sometimes_unsatisfied.sort();
        let mut reached_unhit: Vec<&String> = props.reached_never_hit.iter().collect();
        reached_unhit.sort();

        let json = serde_json::json!({
            "scenario": format!("{:?}", result.scenario),
            "iterations": result.iterations,
            "failures": result.failures.len(),
            "failure_details": result.failures.iter().map(|f| {
                serde_json::json!({
                    "iteration": f.iteration,
                    "seed": f.seed,
                    "invariant": f.invariant_name,
                    "message": f.message,
                })
            }).collect::<Vec<_>>(),
            "properties": {
                "iterations_aggregated": props.iterations_aggregated,
                "sometimes_ever_satisfied": props.sometimes_ever_satisfied.len(),
                "sometimes_never_satisfied": props.sometimes_never_satisfied.len(),
                "reached_ever_hit": props.reached_ever_hit.len(),
                "reached_never_hit": props.reached_never_hit.len(),
                "always_never_violation_count": props.always_never_violations.len(),
                "unsatisfied_sometimes_ids": sometimes_unsatisfied,
                "unreached_ids": reached_unhit,
                "violated_ids": props.always_never_violations.keys().collect::<Vec<_>>(),
            },
        });

        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    }

    fn print_compact(&self, result: &RunResult) {
        println!(
            "{:?}: {}/{} passed",
            result.scenario,
            result.iterations - result.failures.len() as u64,
            result.iterations
        );
    }
}

impl Command for RunCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Create output directory if specified
        if let Some(ref dir) = self.output_dir {
            std::fs::create_dir_all(dir)?;
        }

        let result = self.run_simulation()?;
        self.print_results(&result);

        if !result.failures.is_empty() {
            Err(CommandError::Simulation(format!(
                "{} failures detected",
                result.failures.len()
            )))
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// Result Types
// ============================================================================

/// Result of a simulation run.
#[derive(Debug)]
struct RunResult {
    iterations: u64,
    failures: Vec<FailureRecord>,
    scenario: ScenarioType,
    /// Aggregate property coverage across all iterations.
    properties: PropertyAggregate,
}

/// Record of a single failure.
#[derive(Debug, Clone)]
struct FailureRecord {
    seed: u64,
    iteration: u64,
    invariant_name: String,
    message: String,
}

/// Aggregate property tracking across a batch of simulation runs.
///
/// For SOMETIMES properties, an ID that was satisfied in ANY iteration is
/// considered covered for the batch. Unsatisfied IDs are those never satisfied
/// in any iteration — the coverage gaps that DPOR and targeted fuzzing should
/// address.
#[derive(Debug, Default, Clone)]
struct PropertyAggregate {
    /// Set of all distinct property IDs observed.
    all_ids: std::collections::HashSet<String>,
    /// SOMETIMES IDs satisfied in at least one iteration.
    sometimes_ever_satisfied: std::collections::HashSet<String>,
    /// SOMETIMES IDs observed but never satisfied in any iteration.
    sometimes_never_satisfied: std::collections::HashSet<String>,
    /// REACHED IDs hit in at least one iteration.
    reached_ever_hit: std::collections::HashSet<String>,
    /// REACHED IDs observed but never hit in any iteration.
    reached_never_hit: std::collections::HashSet<String>,
    /// ALWAYS/NEVER IDs that were ever violated (bugs — aggregated count).
    always_never_violations: std::collections::HashMap<String, u64>,
    /// Number of iterations that contributed to this aggregate.
    iterations_aggregated: u64,
}

impl PropertyAggregate {
    fn merge(&mut self, report: &kimberlite_properties::registry::PropertyReport) {
        self.iterations_aggregated += 1;

        // PropertyReport now gives us explicit satisfied/hit ID lists, so we
        // no longer need to infer ever-satisfied SOMETIMES by flipping IDs
        // between runs (which lost any SOMETIMES satisfied on seed 0).
        for id in &report.satisfied_sometimes_ids {
            self.all_ids.insert(id.clone());
            self.sometimes_ever_satisfied.insert(id.clone());
            self.sometimes_never_satisfied.remove(id);
        }
        for id in &report.unsatisfied_sometimes_ids {
            self.all_ids.insert(id.clone());
            if !self.sometimes_ever_satisfied.contains(id) {
                self.sometimes_never_satisfied.insert(id.clone());
            }
        }

        for id in &report.reached_hit_ids {
            self.all_ids.insert(id.clone());
            self.reached_ever_hit.insert(id.clone());
            self.reached_never_hit.remove(id);
        }
        for id in &report.unreached_ids {
            self.all_ids.insert(id.clone());
            if !self.reached_ever_hit.contains(id) {
                self.reached_never_hit.insert(id.clone());
            }
        }

        for id in &report.violated_ids {
            self.all_ids.insert(id.clone());
            *self.always_never_violations.entry(id.clone()).or_insert(0) += 1;
        }

        let reached_hit_this_run: std::collections::HashSet<String> = self
            .reached_never_hit
            .difference(&report.unreached_ids.iter().cloned().collect())
            .cloned()
            .collect();
        for id in reached_hit_this_run {
            self.reached_never_hit.remove(&id);
            self.reached_ever_hit.insert(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_builder() {
        let cmd = RunCommand::new(ScenarioType::Baseline)
            .with_iterations(100)
            .with_seed(42)
            .with_format(OutputFormat::Json)
            .with_verbosity(Verbosity::Verbose);

        assert_eq!(cmd.scenario, ScenarioType::Baseline);
        assert_eq!(cmd.iterations, 100);
        assert_eq!(cmd.seed, Some(42));
        assert_eq!(cmd.format, OutputFormat::Json);
        assert_eq!(cmd.verbosity, Verbosity::Verbose);
    }
}
