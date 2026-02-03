//! Scenarios command for listing available test scenarios.

use super::{Command, CommandError};
use crate::scenarios::ScenarioType;

// ============================================================================
// Scenarios Command
// ============================================================================

/// Lists all available VOPR scenarios.
#[derive(Debug, Clone, Default)]
pub struct ScenariosCommand {
    /// Show detailed descriptions.
    pub detailed: bool,
}

impl ScenariosCommand {
    /// Creates a new scenarios command.
    pub fn new() -> Self {
        Self { detailed: false }
    }

    /// Enables detailed descriptions.
    pub fn with_detailed(mut self, detailed: bool) -> Self {
        self.detailed = detailed;
        self
    }
}

impl Command for ScenariosCommand {
    fn execute(&self) -> Result<(), CommandError> {
        println!("═══════════════════════════════════════════════════════");
        println!("Available VOPR Scenarios");
        println!("═══════════════════════════════════════════════════════\n");

        let scenarios = [
            (
                ScenarioType::Baseline,
                "Baseline",
                "Basic cluster operation with no faults",
            ),
            (
                ScenarioType::SwizzleClogging,
                "Swizzle Clogging",
                "Intermittent network congestion",
            ),
            (
                ScenarioType::GrayFailures,
                "Gray Failures",
                "Partial failures and asymmetric faults",
            ),
            (
                ScenarioType::MultiTenantIsolation,
                "Multi-Tenant Isolation",
                "Verify tenant isolation under load",
            ),
            (
                ScenarioType::TimeCompression,
                "Time Compression",
                "Accelerated time for long-running scenarios",
            ),
            (
                ScenarioType::Combined,
                "Combined",
                "All fault types enabled",
            ),
        ];

        for (scenario_type, name, description) in &scenarios {
            if self.detailed {
                println!("● {:?}", scenario_type);
                println!("  Name: {}", name);
                println!("  Description: {}", description);
                println!();
            } else {
                println!("  • {:?}: {}", scenario_type, name);
            }
        }

        if !self.detailed {
            println!("\nUse --detailed for more information");
        }

        println!("\n✓ Run a scenario: vopr run <scenario> [iterations]");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenarios_command_creation() {
        let cmd = ScenariosCommand::new().with_detailed(true);
        assert!(cmd.detailed);
    }
}
