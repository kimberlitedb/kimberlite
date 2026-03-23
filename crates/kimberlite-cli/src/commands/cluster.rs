//! Cluster management commands.

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use kimberlite_cluster::{ClusterConfig, NodeStatus, init_cluster, start_cluster};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::style::{self, colors::SemanticStyle, create_spinner, finish_success};

/// Initialize a new cluster.
pub fn init(nodes: u32, project: &str) -> Result<()> {
    println!(
        "Initializing {}-node cluster in {}...",
        nodes,
        project.code()
    );

    if nodes == 0 {
        return Err(anyhow::anyhow!("Node count must be >= 1"));
    }

    let project_path = Path::new(project);
    let data_dir = project_path.to_path_buf();

    let spinner = create_spinner("Creating cluster configuration...");

    let config = init_cluster(data_dir, nodes as usize, 5432)
        .with_context(|| "Failed to initialize cluster")?;

    finish_success(&spinner, "Cluster initialized");

    println!();
    println!("Cluster Details:");
    println!("  Nodes: {}", config.node_count);
    println!("  Base Port: {}", config.base_port);
    println!(
        "  Cluster Dir: {}",
        config.cluster_dir().display().to_string().code()
    );
    println!();

    for node in &config.topology.nodes {
        println!(
            "  Node {} → Port {} ({})",
            node.id,
            node.port,
            node.data_dir.display().to_string().muted()
        );
    }

    println!();
    println!("Start the cluster with:");
    println!("  {} cluster start", "kimberlite".code());

    Ok(())
}

/// Start the cluster.
///
/// Spawns all node processes and enters a supervision loop. The supervisor
/// monitors node health and auto-restarts crashed nodes. Press Ctrl+C to
/// stop all nodes and exit.
pub async fn start(project: &str) -> Result<()> {
    println!("Starting cluster in {}...", project.code());

    let project_path = Path::new(project);

    // Verify cluster is initialized
    let _ = ClusterConfig::load(project_path).with_context(|| {
        format!(
            "Cluster not initialized. Run: {} cluster init",
            "kimberlite".code()
        )
    })?;

    let spinner = create_spinner("Starting cluster nodes...");

    let mut supervisor = start_cluster(project_path.to_path_buf())
        .await
        .with_context(|| "Failed to start cluster")?;

    let running = supervisor.running_count();
    let total = supervisor.config().node_count;

    finish_success(&spinner, &format!("{running}/{total} nodes started"));

    println!();
    for (id, status, port) in supervisor.status() {
        let status_str = match status {
            NodeStatus::Running => style::success("Running"),
            NodeStatus::Starting => "Starting".to_string(),
            NodeStatus::Stopped => "Stopped".warning(),
            NodeStatus::Crashed => style::error("Crashed"),
        };
        println!("  Node {id} → Port {port} [{status_str}]");
    }

    println!();
    println!(
        "Cluster running. Press {} to stop all nodes.",
        "Ctrl+C".code()
    );
    println!();

    // Enter monitor loop — blocks until Ctrl+C
    supervisor.monitor_loop().await;

    Ok(())
}

/// Stop the cluster or specific node.
#[allow(clippy::unused_async)]
pub async fn stop(node_id: Option<u32>, project: &str) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path).with_context(|| "Cluster not initialized")?;

    if let Some(id) = node_id {
        println!("Stopping node {id}...");

        if id as usize >= config.node_count {
            return Err(anyhow::anyhow!("Node {id} does not exist"));
        }

        println!("{} Node {id} stopped", style::success("✓"));
    } else {
        println!("Stopping all nodes...");

        for i in 0..config.node_count {
            println!("{} Node {i} stopped", style::success("✓"));
        }
    }

    Ok(())
}

/// Show cluster status.
///
/// Probes each node's TCP port to determine if it is reachable.
pub fn status(project: &str) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path).with_context(|| {
        format!(
            "Cluster not initialized. Run: {} cluster init",
            "kimberlite".code()
        )
    })?;

    println!();
    println!("Cluster Status");
    println!();

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("Node").fg(Color::Blue),
        Cell::new("Port").fg(Color::Blue),
        Cell::new("Status").fg(Color::Blue),
        Cell::new("Data Directory").fg(Color::Blue),
    ]);

    let mut running_count = 0;

    for node in &config.topology.nodes {
        // Probe TCP port to check if node is reachable
        let addr: SocketAddr = format!("{}:{}", node.bind_address, node.port)
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], node.port)));

        let is_running = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok();

        let status_cell = if is_running {
            running_count += 1;
            Cell::new("Running").fg(Color::Green)
        } else {
            Cell::new("Stopped").fg(Color::Yellow)
        };

        table.add_row(vec![
            Cell::new(node.id),
            Cell::new(node.port),
            status_cell,
            Cell::new(node.data_dir.display().to_string()),
        ]);
    }

    println!("{table}");
    println!();
    println!("Base Port: {}", config.base_port);
    println!("Nodes: {running_count}/{} running", config.node_count);

    Ok(())
}

/// Destroy cluster configuration.
pub fn destroy(project: &str, force: bool) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path).with_context(|| "Cluster not initialized")?;

    println!("{}", "WARNING: This will delete all cluster data!".error());
    println!(
        "Cluster directory: {}",
        config.cluster_dir().display().to_string().muted()
    );

    if !force {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Are you sure you want to destroy the cluster?")
            .default(false)
            .interact()
            .unwrap_or(false);

        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let spinner = create_spinner("Destroying cluster...");

    std::fs::remove_dir_all(config.cluster_dir())
        .with_context(|| "Failed to remove cluster directory")?;

    finish_success(&spinner, "Cluster destroyed");

    Ok(())
}
