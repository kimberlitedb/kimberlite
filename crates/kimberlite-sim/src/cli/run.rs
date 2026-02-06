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
        };

        let runner = VoprRunner::new(config.clone());
        let mut progress = ProgressReporter::new(self.iterations, self.show_progress);

        let mut failures = Vec::new();
        let mut event_log = if self.enable_logging {
            Some(EventLog::new())
        } else {
            None
        };

        for i in 0..self.iterations {
            progress.update(i + 1);

            let result = runner.run_single(config.seed + i);

            match result {
                VoprResult::Success { .. } => {}
                VoprResult::InvariantViolation {
                    seed,
                    invariant,
                    message,
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
                                failed_at_event: 0, // TODO: Track actual event number
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
}

/// Record of a single failure.
#[derive(Debug, Clone)]
struct FailureRecord {
    seed: u64,
    iteration: u64,
    invariant_name: String,
    message: String,
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
