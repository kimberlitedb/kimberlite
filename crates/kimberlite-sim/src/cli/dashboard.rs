//! Dashboard command for coverage visualization.

#[cfg(feature = "dashboard")]
use super::{Command, CommandError};
#[cfg(feature = "dashboard")]
use crate::coverage_fuzzer::{CoverageFuzzer, SelectionStrategy};
#[cfg(feature = "dashboard")]
use crate::dashboard::DashboardServer;
#[cfg(feature = "dashboard")]
use std::sync::{Arc, Mutex};

// ============================================================================
// Dashboard Command
// ============================================================================

/// Launches the coverage dashboard web server.
#[cfg(feature = "dashboard")]
#[derive(Debug, Clone)]
pub struct DashboardCommand {
    /// Server port (default: 8080).
    pub port: u16,

    /// Path to saved coverage file (optional).
    pub coverage_file: Option<String>,
}

#[cfg(feature = "dashboard")]
impl DashboardCommand {
    /// Creates a new dashboard command.
    pub fn new() -> Self {
        Self {
            port: 8080,
            coverage_file: None,
        }
    }

    /// Sets the server port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Sets the coverage file path.
    pub fn with_coverage_file(mut self, path: String) -> Self {
        self.coverage_file = Some(path);
        self
    }
}

#[cfg(feature = "dashboard")]
impl Default for DashboardCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "dashboard")]
impl Command for DashboardCommand {
    fn execute(&self) -> Result<(), CommandError> {
        println!("═══════════════════════════════════════════");
        println!("VOPR Coverage Dashboard");
        println!("═══════════════════════════════════════════\n");

        // Create fuzzer with coverage tracker
        let fuzzer = if let Some(path) = &self.coverage_file {
            println!("Loading coverage from: {}", path);
            // TODO: Implement coverage loading from file
            // For now, create new fuzzer
            Arc::new(Mutex::new(CoverageFuzzer::new(
                SelectionStrategy::EnergyBased,
            )))
        } else {
            println!("Starting with empty coverage");
            Arc::new(Mutex::new(CoverageFuzzer::new(
                SelectionStrategy::EnergyBased,
            )))
        };

        // Create and run server
        let server = DashboardServer::new(fuzzer).with_port(self.port);

        // Run server (blocks until Ctrl+C)
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| CommandError::Simulation(e.to_string()))?;

        runtime
            .block_on(server.run())
            .map_err(|e| CommandError::Io(e))?;

        Ok(())
    }
}

// Stub implementation when dashboard feature is disabled
#[cfg(not(feature = "dashboard"))]
#[derive(Debug, Clone)]
pub struct DashboardCommand;

#[cfg(not(feature = "dashboard"))]
impl DashboardCommand {
    pub fn new() -> Self {
        Self
    }

    pub fn with_port(self, _port: u16) -> Self {
        self
    }

    pub fn with_coverage_file(self, _path: String) -> Self {
        self
    }
}

#[cfg(not(feature = "dashboard"))]
impl Default for DashboardCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "dashboard"))]
use super::{Command, CommandError};

#[cfg(not(feature = "dashboard"))]
impl Command for DashboardCommand {
    fn execute(&self) -> Result<(), CommandError> {
        Err(CommandError::Simulation(
            "Dashboard feature not enabled. Rebuild with --features dashboard".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_command_creation() {
        let cmd = DashboardCommand::new();
        #[cfg(feature = "dashboard")]
        {
            assert_eq!(cmd.port, 8080);
            assert!(cmd.coverage_file.is_none());
        }
        #[cfg(not(feature = "dashboard"))]
        {
            let _ = cmd; // Suppress unused variable warning
        }
    }

    #[test]
    #[cfg(feature = "dashboard")]
    fn dashboard_command_with_port() {
        let cmd = DashboardCommand::new().with_port(9090);
        assert_eq!(cmd.port, 9090);
    }

    #[test]
    #[cfg(feature = "dashboard")]
    fn dashboard_command_with_coverage_file() {
        let cmd = DashboardCommand::new().with_coverage_file("coverage.json".to_string());
        assert_eq!(cmd.coverage_file, Some("coverage.json".to_string()));
    }

    #[test]
    #[cfg(not(feature = "dashboard"))]
    fn dashboard_command_without_feature_fails() {
        let cmd = DashboardCommand::new();
        let result = cmd.execute();
        assert!(result.is_err());
    }
}
