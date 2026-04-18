//! Start command - runs the Kimberlite server.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result, bail};
use kimberlite::Kimberlite;
use kimberlite_server::{ReplicationMode, Server, ServerConfig};

use crate::style::{
    banner::print_banner, colors::SemanticStyle, create_spinner, finish_success, print_labeled,
    print_spacer, print_success,
};

/// Entry point for `kimberlite start`.
///
/// `cluster` selects multi-node VSR mode. When set, replication config is
/// read from the environment (`KMB_REPLICA_ID` + `KMB_CLUSTER_PEERS`) —
/// that's the only path that reaches [`ReplicationMode::Cluster`]. The
/// chaos VMs use this path; localhost dev uses `development=true` or the
/// default single-node mode.
pub fn run(path: &str, address: &str, development: bool, cluster: bool) -> Result<()> {
    let data_dir = Path::new(path);

    // Verify data directory exists
    if !data_dir.exists() {
        bail!("Data directory '{path}' does not exist. Run 'kimberlite init {path}' first.");
    }

    // Parse address
    let bind_addr: SocketAddr = parse_address(address)?;

    // Print banner
    print_banner();

    // Print configuration
    let canonical_path = data_dir.canonicalize().unwrap_or(data_dir.to_path_buf());
    print_labeled("Data directory", &canonical_path.display().to_string());
    print_labeled("Bind address", &bind_addr.to_string());

    // Open the database
    let sp = create_spinner("Opening database...");
    let db = Kimberlite::open(data_dir).context("Failed to open database")?;
    finish_success(&sp, "Database opened");

    // Configure server
    let replication = if cluster {
        print_labeled("Mode", &"cluster replication".info());
        ReplicationMode::from_env()
            .context("failed to build cluster config from KMB_REPLICA_ID / KMB_CLUSTER_PEERS")?
    } else if development {
        print_labeled("Mode", &"development (no replication)".warning());
        ReplicationMode::Direct
    } else {
        print_labeled("Mode", &"single-node replication".info());
        ReplicationMode::single_node()
    };

    // Bind the HTTP sidecar publicly when running in cluster mode — the
    // chaos harness (and future monitoring tools) poll probes from the
    // host. Development and single-node modes keep the default loopback
    // bind so nothing is accidentally exposed.
    let mut server_config = ServerConfig::new(bind_addr, data_dir).with_replication(replication);
    if cluster {
        let http_port: u16 = std::env::var("KMB_HTTP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9000);
        let http_addr = SocketAddr::from(([0, 0, 0, 0], http_port));
        print_labeled("HTTP sidecar", &http_addr.to_string());
        server_config.metrics_bind_addr = Some(http_addr);
    }

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
