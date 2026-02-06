//! Minimize command for delta debugging test case reduction.

use super::{Command, CommandError, validate_bundle_path};
use crate::delta_debug::{DeltaConfig, DeltaDebugger};
use crate::event_log::ReproBundle;
use std::path::PathBuf;

// ============================================================================
// Minimize Command
// ============================================================================

/// Minimizes a failure bundle using delta debugging.
#[derive(Debug, Clone)]
pub struct MinimizeCommand {
    /// Path to the .kmb bundle file.
    pub bundle_path: PathBuf,

    /// Initial granularity for ddmin algorithm.
    pub granularity: usize,

    /// Maximum minimization iterations.
    pub max_iterations: usize,

    /// Output path for minimized bundle.
    pub output: Option<PathBuf>,

    /// Preserve event ordering during minimization.
    pub preserve_order: bool,
}

impl MinimizeCommand {
    /// Creates a new minimize command.
    pub fn new(bundle_path: PathBuf) -> Self {
        Self {
            bundle_path,
            granularity: 8,
            max_iterations: 100,
            output: None,
            preserve_order: true,
        }
    }

    /// Sets initial granularity.
    pub fn with_granularity(mut self, granularity: usize) -> Self {
        self.granularity = granularity;
        self
    }

    /// Sets maximum iterations.
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Sets output path for minimized bundle.
    pub fn with_output(mut self, output: PathBuf) -> Self {
        self.output = Some(output);
        self
    }

    /// Sets whether to preserve event ordering.
    pub fn with_preserve_order(mut self, preserve_order: bool) -> Self {
        self.preserve_order = preserve_order;
        self
    }
}

impl Command for MinimizeCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Validate bundle path
        validate_bundle_path(&self.bundle_path)?;

        // Load bundle
        let bundle = ReproBundle::load_from_file(&self.bundle_path)
            .map_err(|e| CommandError::InvalidBundle(e.to_string()))?;

        println!("═══════════════════════════════════════════");
        println!("VOPR Delta Debugging - Minimize Test Case");
        println!("═══════════════════════════════════════════");
        println!("Bundle: {}", self.bundle_path.display());
        println!("Seed: {}", bundle.seed);
        println!("Scenario: {}", bundle.scenario);
        println!(
            "Original failure: {} ({})",
            bundle.failure.invariant_name, bundle.failure.message
        );

        let original_events = bundle.event_log.as_ref().map(|e| e.len()).unwrap_or(0);
        println!("Original events: {}", original_events);
        println!("═══════════════════════════════════════════\n");

        // Configure delta debugging
        let config = DeltaConfig {
            max_iterations: self.max_iterations,
            initial_granularity: self.granularity,
            preserve_order: self.preserve_order,
        };

        // Run delta debugging
        let mut debugger = DeltaDebugger::new(bundle, config)
            .map_err(|e| CommandError::Simulation(e.to_string()))?;

        let result = debugger
            .minimize()
            .map_err(|e| CommandError::Simulation(e.to_string()))?;

        println!("\n═══════════════════════════════════════════");
        println!("Minimization Results");
        println!("═══════════════════════════════════════════");
        println!("Original events:  {}", result.original_events);
        println!("Minimized events: {}", result.minimized_events);
        println!("Reduction:        {:.1}%", result.reduction_pct);
        println!("Iterations:       {}", result.iterations);
        println!("Test runs:        {}", result.test_runs);
        println!("═══════════════════════════════════════════");

        // Save minimized bundle
        let output_path = self.output.clone().unwrap_or_else(|| {
            let mut path = self.bundle_path.clone();
            path.set_extension("");
            let new_name = format!("{}.min.kmb", path.file_name().unwrap().to_string_lossy());
            path.set_file_name(new_name);
            path
        });

        result
            .minimized_bundle
            .save_to_file(&output_path)
            .map_err(CommandError::Io)?;

        println!("\n✓ Minimized bundle saved: {}", output_path.display());
        println!("  Original:  {} events", result.original_events);
        println!("  Minimized: {} events", result.minimized_events);

        println!("\n✓ To reproduce: vopr repro {}", output_path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_command_creation() {
        let cmd = MinimizeCommand::new(PathBuf::from("test.kmb"));
        assert_eq!(cmd.granularity, 8);
        assert_eq!(cmd.max_iterations, 100);
        assert!(cmd.preserve_order);
        assert!(cmd.output.is_none());
    }

    #[test]
    fn minimize_command_with_granularity() {
        let cmd = MinimizeCommand::new(PathBuf::from("test.kmb")).with_granularity(4);
        assert_eq!(cmd.granularity, 4);
    }

    #[test]
    fn minimize_command_with_max_iterations() {
        let cmd = MinimizeCommand::new(PathBuf::from("test.kmb")).with_max_iterations(50);
        assert_eq!(cmd.max_iterations, 50);
    }

    #[test]
    fn minimize_command_with_output() {
        let cmd = MinimizeCommand::new(PathBuf::from("test.kmb"))
            .with_output(PathBuf::from("output.kmb"));
        assert_eq!(cmd.output, Some(PathBuf::from("output.kmb")));
    }

    #[test]
    fn minimize_command_with_preserve_order() {
        let cmd = MinimizeCommand::new(PathBuf::from("test.kmb")).with_preserve_order(false);
        assert!(!cmd.preserve_order);
    }
}
