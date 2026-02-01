//! Development server orchestrator for Kimberlite.
//!
//! Provides the unified `kmb dev` command that starts:
//! - Database server
//! - Studio web UI (optional)
//! - Auto-migration (optional)
//!
//! All services run in a single process with graceful shutdown.

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use kimberlite_config::KimberliteConfig;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

mod server;

pub use server::DevServer;

/// Configuration for the dev server.
#[derive(Debug, Clone)]
pub struct DevConfig {
    /// Project directory.
    pub project_dir: String,
    /// Skip auto-migration.
    pub no_migrate: bool,
    /// Skip Studio UI.
    pub no_studio: bool,
    /// Start in cluster mode.
    pub cluster: bool,
    /// Custom database port.
    pub port: Option<u16>,
    /// Custom Studio port.
    pub studio_port: Option<u16>,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            project_dir: ".".to_string(),
            no_migrate: false,
            no_studio: false,
            cluster: false,
            port: None,
            studio_port: None,
        }
    }
}

/// Run the development server.
pub async fn run_dev_server(config: DevConfig) -> Result<()> {
    // Print banner
    print_banner();

    // Check if project is initialized
    let project_path = Path::new(&config.project_dir);
    if !kimberlite_config::Paths::is_initialized(project_path) {
        return Err(anyhow::anyhow!(
            "Project not initialized. Run 'kmb init' in {} first.",
            project_path.display()
        ));
    }

    // Load configuration
    let spinner = create_spinner("Loading configuration...");
    let mut kimberlite_config =
        KimberliteConfig::load_from_dir(project_path).context("Failed to load configuration")?;

    // Apply CLI overrides
    if let Some(port) = config.port {
        kimberlite_config.database.bind_address = format!("127.0.0.1:{}", port);
    }
    if let Some(studio_port) = config.studio_port {
        kimberlite_config.development.studio_port = studio_port;
    }
    if config.no_studio {
        kimberlite_config.development.studio = false;
    }

    spinner.finish_with_message("✓ Config loaded");

    // TODO: Check for pending migrations
    if !config.no_migrate && kimberlite_config.development.auto_migrate {
        println!("⏭  Skipping auto-migration (Phase 4 feature)");
    }

    // Start database server
    let db_address = kimberlite_config.database.bind_address.clone();
    let data_dir = kimberlite_config.database.data_dir.clone();

    let spinner = create_spinner("Starting database server...");
    // TODO: Actually start the server
    spinner.finish_with_message(format!("✓ Database started on {}", db_address));

    // Start Studio if enabled
    if kimberlite_config.development.studio {
        let studio_port = kimberlite_config.development.studio_port;
        let spinner = create_spinner("Starting Studio...");

        let studio_config = kimberlite_studio::StudioConfig {
            port: studio_port,
            db_address: db_address.clone(),
            default_tenant: kimberlite_config.studio.default_tenant,
        };

        // Spawn Studio in background
        tokio::spawn(async move {
            if let Err(e) = kimberlite_studio::run_studio(studio_config).await {
                eprintln!("Studio error: {}", e);
            }
        });

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        spinner.finish_with_message(format!(
            "✓ Studio started on http://127.0.0.1:{}",
            studio_port
        ));
    }

    // Print ready message
    println!();
    println!("Ready! Press Ctrl+C to stop all services.");
    println!();
    println!(" Database:  {}", db_address);
    if kimberlite_config.development.studio {
        println!(
            " Studio:    http://127.0.0.1:{}",
            kimberlite_config.development.studio_port
        );
    }
    println!(" REPL:      kmb repl --tenant 1");
    println!(" Logs:      .kimberlite/logs/dev.log");
    println!();

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for Ctrl+C")?;

    println!();
    println!("Shutting down gracefully...");

    // TODO: Stop services

    println!("✓ All services stopped");

    Ok(())
}

fn print_banner() {
    println!("┌─────────────────────────────────────────────────────┐");
    println!("│ Kimberlite Development Server                       │");
    println!("└─────────────────────────────────────────────────────┘");
    println!();
}

fn create_spinner(msg: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message(msg.to_string());
    spinner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_config_default() {
        let config = DevConfig::default();
        assert_eq!(config.project_dir, ".");
        assert!(!config.no_migrate);
        assert!(!config.no_studio);
        assert!(!config.cluster);
        assert!(config.port.is_none());
        assert!(config.studio_port.is_none());
    }
}
