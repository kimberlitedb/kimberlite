//! Development server orchestrator for Kimberlite.
//!
//! Provides the unified `kimberlite dev` command that starts:
//! - Database server
//! - Studio web UI (optional)
//! - Auto-migration (optional)
//!
//! All services run in a single process with graceful shutdown.

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use kimberlite_config::KimberliteConfig;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;

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
            "Project not initialized. Run 'kimberlite init' in {} first.",
            project_path.display()
        ));
    }

    // Load configuration
    let spinner = create_spinner("Loading configuration...");
    let mut kimberlite_config =
        KimberliteConfig::load_from_dir(project_path).context("Failed to load configuration")?;

    // Apply CLI overrides
    if let Some(port) = config.port {
        kimberlite_config.database.bind_address = format!("127.0.0.1:{port}");
    }
    if let Some(studio_port) = config.studio_port {
        kimberlite_config.development.studio_port = studio_port;
    }
    if config.no_studio {
        kimberlite_config.development.studio = false;
    }

    spinner.finish_with_message("✓ Config loaded");

    // Auto-migration check
    if !config.no_migrate && kimberlite_config.development.auto_migrate {
        let migrations_dir = project_path.join("migrations");
        if migrations_dir.exists() {
            let state_dir = project_path.join(".kimberlite/migrations");
            let mig_config = kimberlite_migration::MigrationConfig {
                migrations_dir,
                state_dir,
                auto_timestamp: true,
            };
            if let Ok(manager) = kimberlite_migration::MigrationManager::new(mig_config) {
                match manager.list_pending() {
                    Ok(pending) if !pending.is_empty() => {
                        println!(
                            "⚠  {} pending migration(s) — run 'kimberlite migration apply' to apply",
                            pending.len()
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    // Start database server
    let data_dir = kimberlite_config.database.data_dir.clone();

    let spinner = create_spinner("Starting database server...");

    let mut dev_server = DevServer::new();
    let requested_addr: SocketAddr = kimberlite_config
        .database
        .bind_address
        .parse()
        .context("Invalid bind address in config")?;
    let data_path = project_path.join(&data_dir);

    // If the user explicitly chose a port, use it as-is (fail if unavailable).
    // Otherwise, find a free port starting from the configured default.
    let bind_addr = if config.port.is_some() {
        requested_addr
    } else {
        find_available_addr(requested_addr)?
    };

    dev_server
        .start(data_path, bind_addr)
        .await
        .context("Failed to start database server")?;

    let db_address = bind_addr.to_string();

    // Give the server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    if bind_addr.port() != requested_addr.port() {
        spinner.finish_with_message(format!(
            "✓ Database started on {db_address} (port {} was in use)",
            requested_addr.port()
        ));
    } else {
        spinner.finish_with_message(format!("✓ Database started on {db_address}"));
    }

    // Start Studio if enabled
    let mut actual_studio_port = kimberlite_config.development.studio_port;
    if kimberlite_config.development.studio {
        let requested_studio_port = kimberlite_config.development.studio_port;
        let spinner = create_spinner("Starting Studio...");

        // Find available port for Studio (unless user explicitly set one)
        if config.studio_port.is_none() {
            let preferred =
                SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), requested_studio_port);
            let resolved = find_available_addr(preferred)?;
            actual_studio_port = resolved.port();
        }

        let studio_config = kimberlite_studio::StudioConfig {
            port: actual_studio_port,
            db_address: db_address.clone(),
            default_tenant: kimberlite_config.studio.default_tenant,
        };

        // Create ProjectionBroadcast for Studio SSE updates.
        // The broadcast is available for future kernel effect wiring (v0.7.0).
        let broadcast =
            std::sync::Arc::new(kimberlite_studio::broadcast::ProjectionBroadcast::default());
        let broadcast_clone = broadcast.clone();

        // Spawn Studio in background
        tokio::spawn(async move {
            if let Err(e) =
                kimberlite_studio::run_studio(studio_config, Some(broadcast_clone)).await
            {
                eprintln!("Studio error: {e}");
            }
        });

        // Give it a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        if actual_studio_port != requested_studio_port {
            spinner.finish_with_message(format!(
                "✓ Studio started on http://127.0.0.1:{actual_studio_port} (port {requested_studio_port} was in use)"
            ));
        } else {
            spinner.finish_with_message(format!(
                "✓ Studio started on http://127.0.0.1:{actual_studio_port}"
            ));
        }
    }

    // Print ready message
    println!();
    println!("Ready! Press Ctrl+C to stop all services.");
    println!();
    println!(" Database:  {db_address}");
    if kimberlite_config.development.studio {
        println!(
            " Studio:    http://127.0.0.1:{actual_studio_port}",
        );
    }
    if bind_addr.port() == 5432 {
        println!(" REPL:      kimberlite repl --tenant 1");
    } else {
        println!(
            " REPL:      kimberlite repl --address {db_address} --tenant 1"
        );
    }
    println!(" Logs:      .kimberlite/logs/dev.log");
    println!();

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for Ctrl+C")?;

    println!();
    println!("Shutting down gracefully...");

    dev_server.stop().await.ok();

    println!("✓ All services stopped");

    Ok(())
}

/// Check if a TCP port is available for binding.
fn is_port_available(addr: SocketAddr) -> bool {
    TcpListener::bind(addr).is_ok()
}

/// Find an available address, starting from `preferred` and scanning upward.
///
/// If the preferred port is free, returns it immediately. Otherwise tries up to
/// 16 consecutive ports. Returns an error only if none are available.
fn find_available_addr(preferred: SocketAddr) -> Result<SocketAddr> {
    const MAX_ATTEMPTS: u16 = 16;

    if is_port_available(preferred) {
        return Ok(preferred);
    }

    let base_port = preferred.port();
    for offset in 1..=MAX_ATTEMPTS {
        let candidate_port = base_port.checked_add(offset).context("Port overflow")?;
        let candidate = SocketAddr::new(preferred.ip(), candidate_port);
        if is_port_available(candidate) {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "Could not find an available port in range {base_port}–{}. \
         Use --port to specify one manually.",
        base_port + MAX_ATTEMPTS
    ))
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

    #[test]
    fn test_find_available_addr_free_port() {
        // Port 0 trick: bind to :0 to get a free port, then verify find_available_addr picks it
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let free_port = listener.local_addr().unwrap().port();
        drop(listener);

        let addr: SocketAddr = format!("127.0.0.1:{free_port}").parse().unwrap();
        let result = find_available_addr(addr).unwrap();
        assert_eq!(result.port(), free_port);
    }

    #[test]
    fn test_find_available_addr_falls_back() {
        // Hold a port open, then ask for it — should get the next one
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port = listener.local_addr().unwrap().port();

        let addr: SocketAddr = format!("127.0.0.1:{occupied_port}").parse().unwrap();
        let result = find_available_addr(addr).unwrap();
        assert_ne!(result.port(), occupied_port);
        assert!(result.port() > occupied_port);
        assert!(result.port() <= occupied_port + 16);
    }

    #[test]
    fn test_is_port_available() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied = listener.local_addr().unwrap();
        assert!(!is_port_available(occupied));
        drop(listener);
        assert!(is_port_available(occupied));
    }
}
