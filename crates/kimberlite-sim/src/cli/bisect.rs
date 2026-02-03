//! Bisect command for finding first failing event.

use super::{Command, CommandError, validate_bundle_path};
use crate::bisect::{BisectConfig, BisectEngine};
use crate::event_log::ReproBundle;
use std::path::PathBuf;

// ============================================================================
// Bisect Command
// ============================================================================

/// Bisects a failure bundle to find first failing event.
#[derive(Debug, Clone)]
pub struct BisectCommand {
    /// Path to the .kmb bundle file.
    pub bundle_path: PathBuf,

    /// Checkpoint interval (events).
    pub checkpoint_interval: u64,

    /// Output path for minimized bundle.
    pub output: Option<PathBuf>,
}

impl BisectCommand {
    /// Creates a new bisect command.
    pub fn new(bundle_path: PathBuf) -> Self {
        Self {
            bundle_path,
            checkpoint_interval: 1000,
            output: None,
        }
    }

    /// Sets checkpoint interval.
    pub fn with_checkpoint_interval(mut self, interval: u64) -> Self {
        self.checkpoint_interval = interval;
        self
    }

    /// Sets output path for minimized bundle.
    pub fn with_output(mut self, output: PathBuf) -> Self {
        self.output = Some(output);
        self
    }
}

impl Command for BisectCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Validate bundle path
        validate_bundle_path(&self.bundle_path)?;

        // Load bundle
        let bundle = ReproBundle::load_from_file(&self.bundle_path)
            .map_err(|e| CommandError::InvalidBundle(e.to_string()))?;

        println!("═══════════════════════════════════════════");
        println!("VOPR Bisect - Find First Failing Event");
        println!("═══════════════════════════════════════════");
        println!("Bundle: {}", self.bundle_path.display());
        println!("Seed: {}", bundle.seed);
        println!("Scenario: {}", bundle.scenario);
        println!("Original failure: {} ({})",
            bundle.failure.invariant_name,
            bundle.failure.message
        );
        println!("Failed at event: {}", bundle.failure.failed_at_event);
        println!("═══════════════════════════════════════════\n");

        // Configure bisection
        let config = BisectConfig {
            checkpoint_interval: self.checkpoint_interval,
            max_iterations: 50,
            verify_invariant: bundle.failure.invariant_name.clone(),
        };

        // Run bisection
        let mut engine = BisectEngine::new(bundle, config);
        let result = engine.bisect()
            .map_err(|e| CommandError::Simulation(e.to_string()))?;

        println!("\n═══════════════════════════════════════════");
        println!("Bisection Results");
        println!("═══════════════════════════════════════════");
        println!("First bad event:  {}", result.first_bad_event);
        println!("Last good event:  {}", result.last_good_event);
        println!("Iterations:       {}", result.iterations);
        println!("Checkpoints used: {}", result.checkpoints_created);
        println!("Time:             {:.2}s", result.replay_time_ms as f64 / 1000.0);
        println!("═══════════════════════════════════════════");

        // Save minimized bundle
        let output_path = self.output.clone()
            .unwrap_or_else(|| {
                let mut path = self.bundle_path.clone();
                path.set_extension("");
                let new_name = format!("{}.minimal.kmb", path.file_name().unwrap().to_string_lossy());
                path.set_file_name(new_name);
                path
            });

        result.minimized_bundle.save_to_file(&output_path)
            .map_err(CommandError::Io)?;

        println!("\n✓ Minimized bundle saved: {}", output_path.display());
        println!("  Original: {} events", result.minimized_bundle.event_log.as_ref().map(|e| e.len() + 1).unwrap_or(0));
        println!("  Minimized: {} events", result.first_bad_event);

        println!("\n✓ To reproduce: vopr repro {}", output_path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_command_creation() {
        let cmd = BisectCommand::new(PathBuf::from("test.kmb"));
        assert_eq!(cmd.checkpoint_interval, 1000);
        assert!(cmd.output.is_none());
    }

    #[test]
    fn bisect_command_with_checkpoint_interval() {
        let cmd = BisectCommand::new(PathBuf::from("test.kmb"))
            .with_checkpoint_interval(500);
        assert_eq!(cmd.checkpoint_interval, 500);
    }

    #[test]
    fn bisect_command_with_output() {
        let cmd = BisectCommand::new(PathBuf::from("test.kmb"))
            .with_output(PathBuf::from("output.kmb"));
        assert_eq!(cmd.output, Some(PathBuf::from("output.kmb")));
    }
}
