//! Configuration management commands.

use anyhow::{Context, Result};
use kmb_config::{KimberliteConfig, Paths};
use std::path::Path;

/// Show current configuration.
pub fn show(project: &str, format: &str) -> Result<()> {
    let project_path = Path::new(project);

    // Check if project is initialized
    if !Paths::is_initialized(project_path) {
        anyhow::bail!(
            "Project not initialized. Run 'kmb init' in {} first.",
            project_path.display()
        );
    }

    // Load configuration
    let config = KimberliteConfig::load_from_dir(project_path)
        .context("Failed to load configuration")?;

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&config)?;
            println!("{}", json);
        }
        "toml" => {
            let toml_str = toml::to_string_pretty(&config)?;
            println!("{}", toml_str);
        }
        "text" | _ => {
            println!("Kimberlite Configuration");
            println!("========================\n");

            println!("Project:");
            println!("  Name: {}", config.project.name);
            println!();

            println!("Database:");
            println!("  Data directory: {}", config.database.data_dir.display());
            println!("  Bind address: {}", config.database.bind_address);
            println!("  Max connections: {}", config.database.max_connections);
            println!();

            println!("Development:");
            println!("  Studio: {}", config.development.studio);
            println!("  Studio port: {}", config.development.studio_port);
            println!("  Auto-migrate: {}", config.development.auto_migrate);
            println!("  Watch: {}", config.development.watch);
            println!();

            println!("Replication:");
            println!("  Mode: {:?}", config.replication.mode);
            println!();

            println!("Cluster:");
            println!("  Nodes: {}", config.cluster.nodes);
            println!("  Base port: {}", config.cluster.base_port);
            println!();

            println!("Migrations:");
            println!("  Directory: {}", config.migrations.directory.display());
            println!("  Auto-timestamp: {}", config.migrations.auto_timestamp);
            println!();

            println!("Studio:");
            println!(
                "  Default tenant: {}",
                config
                    .studio
                    .default_tenant
                    .map_or("None".to_string(), |t| t.to_string())
            );
            println!("  Time-travel: {}", config.studio.time_travel);
            println!();

            println!("Tenants:");
            println!("  Mode: {:?}", config.tenants.mode);
            println!(
                "  Allow dynamic create: {}",
                config.tenants.allow_dynamic_create
            );
            println!(
                "  Require confirmation: {}",
                config.tenants.require_confirmation
            );
        }
    }

    Ok(())
}

/// Set a configuration value.
pub fn set(project: &str, key: &str, value: &str) -> Result<()> {
    println!(
        "Setting {} = {} in {}",
        key,
        value,
        Path::new(project).display()
    );
    println!();
    println!("Note: Config set command will be fully implemented in a future phase.");
    println!("For now, edit kimberlite.toml or kimberlite.local.toml directly.");
    Ok(())
}

/// Validate configuration files.
pub fn validate(project: &str) -> Result<()> {
    let project_path = Path::new(project);

    println!("Validating configuration in {}...", project_path.display());

    // Check if project is initialized
    if !Paths::is_initialized(project_path) {
        anyhow::bail!(
            "Project not initialized. Run 'kmb init' in {} first.",
            project_path.display()
        );
    }

    // Try to load configuration
    match KimberliteConfig::load_from_dir(project_path) {
        Ok(_) => {
            println!("✓ Configuration is valid");
            Ok(())
        }
        Err(e) => {
            println!("✗ Configuration validation failed:");
            println!("  {}", e);
            Err(e)
        }
    }
}
