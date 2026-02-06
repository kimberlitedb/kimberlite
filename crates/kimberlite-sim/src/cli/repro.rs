//! Reproduce command for replaying failures from .kmb bundles.

use super::{Command, CommandError, validate_bundle_path};
use crate::event_log::ReproBundle;
use crate::scenarios::ScenarioType;
use crate::vopr::{VoprConfig, VoprResult, VoprRunner};
use std::path::PathBuf;

// ============================================================================
// Repro Command
// ============================================================================

/// Reproduces a failure from a .kmb bundle file.
#[derive(Debug, Clone)]
pub struct ReproCommand {
    /// Path to the .kmb bundle file.
    pub bundle_path: PathBuf,

    /// Enable verbose output.
    pub verbose: bool,
}

impl ReproCommand {
    /// Creates a new repro command.
    pub fn new(bundle_path: PathBuf) -> Self {
        Self {
            bundle_path,
            verbose: false,
        }
    }

    /// Enables verbose output.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

impl Command for ReproCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Validate bundle path
        validate_bundle_path(&self.bundle_path)?;

        // Load bundle
        let bundle = ReproBundle::load_from_file(&self.bundle_path)
            .map_err(|e| CommandError::InvalidBundle(e.to_string()))?;

        // Print bundle summary
        println!("═══════════════════════════════════════════════════════");
        println!("Reproducing failure from bundle");
        println!("═══════════════════════════════════════════════════════");
        println!("{}", bundle.summary());
        println!("═══════════════════════════════════════════════════════\n");

        // Parse scenario
        let scenario = match bundle.scenario.as_str() {
            "Baseline" => ScenarioType::Baseline,
            "SwizzleClogging" => ScenarioType::SwizzleClogging,
            "GrayFailures" => ScenarioType::GrayFailures,
            "MultiTenantIsolation" => ScenarioType::MultiTenantIsolation,
            _ => {
                return Err(CommandError::InvalidScenario(format!(
                    "Unknown scenario: {}",
                    bundle.scenario
                )));
            }
        };

        // Reproduce with same seed
        let config = VoprConfig {
            seed: bundle.seed,
            iterations: 1,
            scenario: Some(scenario),
            network_faults: true,
            storage_faults: true,
            verbose: self.verbose,
            max_events: 10_000,
            max_time_ns: 60_000_000_000,
            check_determinism: false,
            enable_trace: self.verbose,
            save_trace_on_failure: true,
            enhanced_workloads: true,
            failure_diagnosis: true,
            workload_ops_per_tick: 5,
            workload_tick_interval_ns: 10_000_000,
        };

        let runner = VoprRunner::new(config);
        let result = runner.run_single(bundle.seed);

        match result {
            VoprResult::Success { .. } => {
                println!("⚠ Warning: Reproduced successfully (no failure detected)");
                println!("   This may indicate:");
                println!("   - Nondeterminism in the simulation");
                println!("   - Bundle was created from wrong seed");
                println!("   - Bug was fixed");
                Ok(())
            }
            VoprResult::InvariantViolation {
                invariant, message, ..
            } => {
                println!("✓ Successfully reproduced failure");
                println!("\nInvariant violated: {}", invariant);
                println!("Message: {}", message);

                if bundle.failure.invariant_name != invariant {
                    println!("\n⚠ Warning: Different invariant violated than in bundle");
                    println!("   Expected: {}", bundle.failure.invariant_name);
                    println!("   Actual: {}", invariant);
                }

                Err(CommandError::Simulation(format!(
                    "Invariant violation: {}",
                    invariant
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repro_command_creation() {
        let cmd = ReproCommand::new(PathBuf::from("test.kmb")).with_verbose(true);
        assert_eq!(cmd.bundle_path, PathBuf::from("test.kmb"));
        assert!(cmd.verbose);
    }
}
