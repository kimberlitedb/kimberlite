//! TUI command for interactive simulation.

use super::{Command, CommandError};
use crate::scenarios::ScenarioType;
#[cfg(feature = "tui")]
use crate::vopr::VoprConfig;

// ============================================================================
// TUI Command
// ============================================================================

/// Launches the interactive TUI.
#[derive(Debug, Clone)]
pub struct TuiCommand {
    /// Scenario to run (optional, None = all scenarios).
    pub scenario: Option<ScenarioType>,

    /// Number of iterations.
    pub iterations: u64,

    /// RNG seed (optional).
    pub seed: Option<u64>,
}

impl TuiCommand {
    /// Creates a new TUI command.
    pub fn new() -> Self {
        Self {
            scenario: None,
            iterations: 1000,
            seed: None,
        }
    }

    /// Sets the scenario.
    pub fn with_scenario(mut self, scenario: ScenarioType) -> Self {
        self.scenario = Some(scenario);
        self
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
}

impl Default for TuiCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl Command for TuiCommand {
    fn execute(&self) -> Result<(), CommandError> {
        #[cfg(feature = "tui")]
        {
            let config = VoprConfig {
                seed: self.seed.unwrap_or_else(rand::random),
                iterations: self.iterations,
                scenario: self.scenario.clone(),
                ..Default::default()
            };

            crate::tui::run_tui(config).map_err(CommandError::Io)?;
            Ok(())
        }

        #[cfg(not(feature = "tui"))]
        {
            Err(CommandError::Simulation(
                "TUI feature not enabled. Rebuild with --features tui".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_command_creation() {
        let cmd = TuiCommand::new();
        assert_eq!(cmd.iterations, 1000);
        assert!(cmd.scenario.is_none());
        assert!(cmd.seed.is_none());
    }

    #[test]
    fn tui_command_with_iterations() {
        let cmd = TuiCommand::new().with_iterations(500);
        assert_eq!(cmd.iterations, 500);
    }

    #[test]
    fn tui_command_with_seed() {
        let cmd = TuiCommand::new().with_seed(12345);
        assert_eq!(cmd.seed, Some(12345));
    }

    #[test]
    fn tui_command_with_scenario() {
        let cmd = TuiCommand::new().with_scenario(ScenarioType::Baseline);
        assert!(cmd.scenario.is_some());
    }
}
