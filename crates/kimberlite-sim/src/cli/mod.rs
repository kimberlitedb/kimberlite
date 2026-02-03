//! CLI command routing and structure for VOPR.
//!
//! This module provides a beautiful, intuitive command-line interface
//! for running simulations, reproducing failures, and analyzing results.
//!
//! ## Commands
//!
//! - `run` - Run simulation with specified scenario
//! - `repro` - Reproduce failure from .kmb bundle
//! - `show` - Display failure summary
//! - `scenarios` - List all available scenarios
//! - `stats` - Display coverage and invariant statistics
//!
//! ## Design Goals
//!
//! - **Intuitive**: Commands feel like git/cargo/rr
//! - **Beautiful**: Rich formatting, colors, progress indicators
//! - **Actionable**: Clear next steps on failures

pub mod run;
pub mod repro;
pub mod show;
pub mod scenarios;
pub mod stats;

pub use run::RunCommand;
pub use repro::ReproCommand;
pub use show::ShowCommand;
pub use scenarios::ScenariosCommand;
pub use stats::StatsCommand;

use std::path::PathBuf;

// ============================================================================
// Command Trait
// ============================================================================

/// Common interface for all VOPR commands.
pub trait Command {
    /// Executes the command.
    fn execute(&self) -> Result<(), CommandError>;
}

// ============================================================================
// Command Errors
// ============================================================================

/// Errors that can occur during command execution.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Simulation error: {0}")]
    Simulation(String),

    #[error("Invalid bundle file: {0}")]
    InvalidBundle(String),

    #[error("Invalid scenario: {0}")]
    InvalidScenario(String),

    #[error("No failures found")]
    NoFailures,
}

// ============================================================================
// Common CLI Types
// ============================================================================

/// Output format for results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text output.
    Human,
    /// JSON output for tooling.
    Json,
    /// Compact summary.
    Compact,
}

impl OutputFormat {
    /// Parses output format from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

/// Verbosity level for output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet = 0,
    Normal = 1,
    Verbose = 2,
    Debug = 3,
}

impl Verbosity {
    /// Returns true if at least the specified level.
    pub fn at_least(&self, level: Verbosity) -> bool {
        *self >= level
    }
}

// ============================================================================
// Progress Reporting
// ============================================================================

/// Progress reporter for long-running operations.
pub struct ProgressReporter {
    /// Total iterations.
    total: u64,
    /// Current iteration.
    current: u64,
    /// Show progress bar.
    show_progress: bool,
}

impl ProgressReporter {
    /// Creates a new progress reporter.
    pub fn new(total: u64, show_progress: bool) -> Self {
        Self {
            total,
            current: 0,
            show_progress,
        }
    }

    /// Updates progress.
    pub fn update(&mut self, current: u64) {
        self.current = current;
        if self.show_progress && current % 100 == 0 {
            self.print_progress();
        }
    }

    /// Finishes progress reporting.
    pub fn finish(&self) {
        if self.show_progress {
            println!("\n✓ Completed {}/{} iterations", self.current, self.total);
        }
    }

    fn print_progress(&self) {
        let percent = (self.current as f64 / self.total as f64 * 100.0) as u64;
        print!("\r[");
        let bar_width: usize = 40;
        let filled = ((bar_width as u64 * self.current) / self.total) as usize;
        for i in 0..bar_width {
            if i < filled {
                print!("=");
            } else if i == filled {
                print!(">");
            } else {
                print!(" ");
            }
        }
        print!("] {}% ({}/{})", percent, self.current, self.total);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

// ============================================================================
// Result Formatting
// ============================================================================

/// Formats a success message with checkmark.
pub fn format_success(message: &str) -> String {
    format!("✓ {}", message)
}

/// Formats an error message with cross mark.
pub fn format_error(message: &str) -> String {
    format!("✗ {}", message)
}

/// Formats a warning message with warning sign.
pub fn format_warning(message: &str) -> String {
    format!("⚠ {}", message)
}

/// Formats an info message with info sign.
pub fn format_info(message: &str) -> String {
    format!("ℹ {}", message)
}

// ============================================================================
// File Operations
// ============================================================================

/// Validates that a bundle file exists and has .kmb extension.
pub fn validate_bundle_path(path: &PathBuf) -> Result<(), CommandError> {
    if !path.exists() {
        return Err(CommandError::InvalidBundle(format!(
            "File not found: {}",
            path.display()
        )));
    }

    if path.extension().and_then(|s| s.to_str()) != Some("kmb") {
        return Err(CommandError::InvalidBundle(format!(
            "Invalid file extension (expected .kmb): {}",
            path.display()
        )));
    }

    Ok(())
}

/// Generates a bundle filename based on seed and timestamp.
pub fn generate_bundle_filename(seed: u64, invariant: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    format!("failure-{}-{}-{}.kmb", invariant, seed, timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_parsing() {
        assert_eq!(OutputFormat::from_str("human"), Some(OutputFormat::Human));
        assert_eq!(OutputFormat::from_str("json"), Some(OutputFormat::Json));
        assert_eq!(
            OutputFormat::from_str("compact"),
            Some(OutputFormat::Compact)
        );
        assert_eq!(OutputFormat::from_str("invalid"), None);
    }

    #[test]
    fn verbosity_levels() {
        assert!(Verbosity::Verbose.at_least(Verbosity::Normal));
        assert!(!Verbosity::Normal.at_least(Verbosity::Verbose));
        assert!(Verbosity::Debug.at_least(Verbosity::Debug));
    }

    #[test]
    fn format_messages() {
        assert!(format_success("test").starts_with('✓'));
        assert!(format_error("test").starts_with('✗'));
        assert!(format_warning("test").starts_with('⚠'));
        assert!(format_info("test").starts_with('ℹ'));
    }

    #[test]
    fn bundle_filename_generation() {
        let filename = generate_bundle_filename(12345, "invariant_test");
        assert!(filename.starts_with("failure-invariant_test-12345-"));
        assert!(filename.ends_with(".kmb"));
    }
}
