//! Start command - runs the Kimberlite server.

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use anyhow::{bail, Context, Result};
use kimberlite::Kimberlite;
use kmb_server::{ReplicationMode, Server, ServerConfig};

use super::init::Config;
use crate::style::{
    banner::print_banner, colors::SemanticStyle, create_spinner, finish_success, print_labeled,
    print_spacer, print_success,
};

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
        Config::default()
    };

    // Parse address
    let bind_addr: SocketAddr = parse_address(address)?;

    // Print banner
    print_banner();

    // Print configuration
    let canonical_path = data_dir
        .canonicalize()
        .unwrap_or(data_dir.to_path_buf());
    print_labeled("Data directory", &canonical_path.display().to_string());
    print_labeled("Bind address", &bind_addr.to_string());

    // Open the database
    let sp = create_spinner("Opening database...");
    let db = Kimberlite::open(data_dir).context("Failed to open database")?;
    finish_success(&sp, "Database opened");

    // Configure server
    let replication = if development {
        print_labeled("Mode", &"development (no replication)".warning());
        ReplicationMode::None
    } else {
        print_labeled("Mode", &"single-node replication".info());
        ReplicationMode::single_node()
    };

    let server_config = ServerConfig::new(bind_addr, data_dir)
        .with_max_connections(config.server.max_connections)
        .with_replication(replication);

    // Create server
    let sp = create_spinner("Starting server...");
    let mut server =
        Server::with_signal_handling(server_config, db).context("Failed to create server")?;
    finish_success(&sp, "Server started");

    print_spacer();
    print_success("Server is ready. Press Ctrl+C to stop.");
    print_spacer();

    // Run server
    server
        .run_with_shutdown()
        .context("Server error during operation")?;

    print_spacer();
    print_success("Server stopped gracefully.");

    Ok(())
}

/// Parses an address string into a `SocketAddr`.
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
