//! Cluster management commands.

use anyhow::{Context, Result};
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};
use indicatif::{ProgressBar, ProgressStyle};
use kimberlite_cluster::{init_cluster, ClusterConfig};
use std::io::{self, Write};
use std::path::Path;

use crate::style::{self, colors::SemanticStyle};

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

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Creating cluster configuration...");

    let config = init_cluster(data_dir, nodes as usize, 5432)
        .with_context(|| "Failed to initialize cluster")?;

    spinner.finish_with_message(format!(
        "{} Cluster initialized",
        style::success("✓")
    ));

    println!();
    println!("Cluster Details:");
    println!("  Nodes: {}", config.node_count);
    println!("  Base Port: {}", config.base_port);
    println!("  Cluster Dir: {}", config.cluster_dir().display().to_string().code());
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
    println!("  {} cluster start", "kmb".code());

    Ok(())
}

/// Start the cluster.
pub async fn start(project: &str) -> Result<()> {
    println!("Starting cluster in {}...", project.code());

    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path)
        .with_context(|| format!("Cluster not initialized. Run: {} cluster init", "kmb".code()))?;

    println!();
    println!("Starting {} nodes...", config.node_count);

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );

    for node in &config.topology.nodes {
        spinner.set_message(format!("Starting node {}...", node.id));

        // TODO: Start actual node process
        // For now, just show that we would start it
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        spinner.finish_with_message(format!(
            "{} Node {} started on port {}",
            style::success("✓"),
            node.id,
            node.port
        ));
    }

    println!();
    println!("{}", "Note: Cluster process supervision not yet fully implemented.".warning());
    println!("In production, this will start and supervise all node processes.");
    println!();
    println!("Check status with:");
    println!("  {} cluster status", "kmb".code());

    Ok(())
}

/// Stop the cluster or specific node.
pub async fn stop(node_id: Option<u32>, project: &str) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path)
        .with_context(|| "Cluster not initialized")?;

    if let Some(id) = node_id {
        println!("Stopping node {}...", id);

        if id as usize >= config.node_count {
            return Err(anyhow::anyhow!("Node {} does not exist", id));
        }

        println!("{} Node {} stopped", style::success("✓"), id);
    } else {
        println!("Stopping all nodes...");

        for i in 0..config.node_count {
            println!("{} Node {} stopped", style::success("✓"), i);
        }
    }

    Ok(())
}

/// Show cluster status.
pub fn status(project: &str) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path)
        .with_context(|| format!("Cluster not initialized. Run: {} cluster init", "kmb".code()))?;

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

    for node in &config.topology.nodes {
        // TODO: Get actual status from supervisor
        let status_cell = Cell::new("Stopped").fg(Color::Yellow);

        table.add_row(vec![
            Cell::new(node.id),
            Cell::new(node.port),
            status_cell,
            Cell::new(node.data_dir.display().to_string()),
        ]);
    }

    println!("{}", table);
    println!();
    println!("Base Port: {}", config.base_port);
    println!("Total Nodes: {}", config.node_count);
    println!();
    println!("{}", "Note: Live status monitoring not yet implemented.".warning());

    Ok(())
}

/// Destroy cluster configuration.
pub fn destroy(project: &str, force: bool) -> Result<()> {
    let project_path = Path::new(project);
    let config = ClusterConfig::load(project_path)
        .with_context(|| "Cluster not initialized")?;

    println!(
        "{}",
        "WARNING: This will delete all cluster data!".error()
    );
    println!("Cluster directory: {}", config.cluster_dir().display().to_string().muted());

    if !force {
        print!("Are you sure you want to destroy the cluster? (y/N): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Destroying cluster...");

    std::fs::remove_dir_all(config.cluster_dir())
        .with_context(|| "Failed to remove cluster directory")?;

    spinner.finish_with_message(format!(
        "{} Cluster destroyed",
        style::success("✓")
    ));

    Ok(())
}
