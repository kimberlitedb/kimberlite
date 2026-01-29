//! Start command - runs the Kimberlite server.

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result, bail};
use kimberlite::Kimberlite;
use kmb_server::{ReplicationMode, Server, ServerConfig};
use tracing::info;

use super::init::Config;

pub fn run(path: &str, address: &str, development: bool) -> Result<()> {
    let data_dir = Path::new(path);

    // Verify data directory exists
    if !data_dir.exists() {
        bail!("Data directory '{path}' does not exist. Run 'kimberlite init {path}' first.");
    }

    // Load configuration
    let config_path = data_dir.join("config.toml");
    let config: Config = if config_path.exists() {
        let content = fs::read_to_string(&config_path).context("Failed to read config file")?;
        toml::from_str(&content).context("Failed to parse config file")?
    } else {
        info!("No config.toml found, using defaults");
        Config::default()
    };

    // Parse address
    let bind_addr: SocketAddr = parse_address(address)?;

    info!("Starting Kimberlite server...");
    println!();
    println!("Kimberlite - the compliance-first database");
    println!();
    println!(
        "  Data directory: {}",
        data_dir
            .canonicalize()
            .unwrap_or(data_dir.to_path_buf())
            .display()
    );
    println!("  Bind address:   {bind_addr}");

    // Open the database
    let db = Kimberlite::open(data_dir).context("Failed to open database")?;

    // Configure server
    let replication = if development {
        println!("  Mode:           development (no replication)");
        ReplicationMode::None
    } else {
        println!("  Mode:           single-node replication");
        ReplicationMode::single_node()
    };

    let server_config = ServerConfig::new(bind_addr, data_dir)
        .with_max_connections(config.server.max_connections)
        .with_replication(replication);

    println!();
    println!("Server is ready. Press Ctrl+C to stop.");
    println!();

    // Create and run server with signal handling
    let mut server =
        Server::with_signal_handling(server_config, db).context("Failed to create server")?;

    server
        .run_with_shutdown()
        .context("Server error during operation")?;

    println!();
    println!("Server stopped gracefully.");

    Ok(())
}

/// Parses an address string into a `SocketAddr`.
///
/// Accepts:
/// - Port only: "3000" -> "127.0.0.1:3000"
/// - Full address: "127.0.0.1:3000"
/// - IPv6: `[::1]:3000`
fn parse_address(address: &str) -> Result<SocketAddr> {
    // Try parsing as a full address first
    if let Ok(addr) = address.parse::<SocketAddr>() {
        return Ok(addr);
    }

    // Try parsing as just a port
    if let Ok(port) = address.parse::<u16>() {
        return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
    }

    bail!(
        "Invalid address '{address}'. Use a port (e.g., '3000') or full address (e.g., '127.0.0.1:3000')"
    );
}
