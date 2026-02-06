//! Show command for displaying failure bundle information.

use super::{Command, CommandError, validate_bundle_path};
use crate::event_log::ReproBundle;
use std::path::PathBuf;

// ============================================================================
// Show Command
// ============================================================================

/// Displays information about a failure bundle.
#[derive(Debug, Clone)]
pub struct ShowCommand {
    /// Path to the .kmb bundle file.
    pub bundle_path: PathBuf,

    /// Show event log details.
    pub show_events: bool,
}

impl ShowCommand {
    /// Creates a new show command.
    pub fn new(bundle_path: PathBuf) -> Self {
        Self {
            bundle_path,
            show_events: false,
        }
    }

    /// Enables event log display.
    pub fn with_events(mut self, show: bool) -> Self {
        self.show_events = show;
        self
    }
}

impl Command for ShowCommand {
    fn execute(&self) -> Result<(), CommandError> {
        // Validate bundle path
        validate_bundle_path(&self.bundle_path)?;

        // Load bundle
        let bundle = ReproBundle::load_from_file(&self.bundle_path)
            .map_err(|e| CommandError::InvalidBundle(e.to_string()))?;

        // Display bundle information
        println!("═══════════════════════════════════════════════════════");
        println!("Failure Bundle: {}", self.bundle_path.display());
        println!("═══════════════════════════════════════════════════════");
        println!("{}", bundle.summary());
        println!("═══════════════════════════════════════════════════════");

        if self.show_events {
            if let Some(ref events) = bundle.event_log {
                println!("\nEvent Log ({} events):", events.len());
                for (i, event) in events.iter().enumerate().take(20) {
                    println!("  {:4}. [{}ns] {:?}", i, event.time_ns, event.decision);
                }
                if events.len() > 20 {
                    println!("  ... ({} more events)", events.len() - 20);
                }
            } else {
                println!("\nNo event log in bundle");
            }
        }

        println!(
            "\n✓ To reproduce: vopr repro {}",
            self.bundle_path.display()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_command_creation() {
        let cmd = ShowCommand::new(PathBuf::from("test.kmb")).with_events(true);
        assert_eq!(cmd.bundle_path, PathBuf::from("test.kmb"));
        assert!(cmd.show_events);
    }
}
