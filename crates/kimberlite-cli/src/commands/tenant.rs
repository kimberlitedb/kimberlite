//! Tenant management commands.

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use indicatif::{ProgressBar, ProgressStyle};
use kimberlite_client::{Client, ClientConfig};
use kimberlite_types::TenantId;
use std::io::{self, Write};
use std::time::Duration;

use crate::style;

/// Create a new tenant.
pub fn create(server: &str, id: u64, name: &str, force: bool) -> Result<()> {
    println!("Creating tenant {} (ID: {})...", style::tenant(name), id);

    // Confirmation prompt unless --force
    if !force {
        print!("Are you sure you want to create this tenant? (y/N): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Show spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Connecting to server...");

    // Connect to server
    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(5)),
        write_timeout: Some(Duration::from_secs(5)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    let tenant_id = TenantId::new(id);
    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to server at {server}"))?;

    spinner.set_message(format!("Creating tenant '{name}'..."));

    // TODO(v0.7.0): Once server supports tenant creation API, call it here
    // For now, we just verify the connection works
    spinner.finish_with_message(format!(
        "{} Tenant {} created successfully (ID: {})",
        style::success("✓"),
        style::tenant(name),
        id
    ));

    println!();
    println!("Note: Full tenant creation API will be implemented in a future phase.");
    println!("For now, tenants are auto-created on first connection.");

    Ok(())
}

/// List all tenants.
pub fn list(_server: &str) -> Result<()> {
    println!("Listing tenants...");

    // Show spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Connecting to server...");

    // TODO(v0.7.0): Once server supports tenant listing API, implement it here
    spinner.finish_and_clear();

    // Mock data for demonstration
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Cyan),
        Cell::new("Name").fg(Color::Cyan),
        Cell::new("Protected").fg(Color::Cyan),
        Cell::new("Created").fg(Color::Cyan),
    ]);

    // Example row
    table.add_row(vec![
        Cell::new("1"),
        Cell::new("dev-fixtures"),
        Cell::new("✓").fg(Color::Green),
        Cell::new("2026-02-01"),
    ]);

    println!("{table}");
    println!();
    println!("Note: Full tenant listing API will be implemented in a future phase.");

    Ok(())
}

/// Delete a tenant.
pub fn delete(_server: &str, id: u64, force: bool) -> Result<()> {
    println!("Deleting tenant ID {id}...");

    // Confirmation prompt unless --force
    if !force {
        print!(
            "{}",
            style::error(&format!(
                "WARNING: This will permanently delete tenant {id} and all its data!\n"
            ))
        );
        print!("Type the tenant ID to confirm deletion: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let confirmed_id: u64 = input.trim().parse().context("Invalid tenant ID entered")?;

        if confirmed_id != id {
            println!("Tenant ID mismatch. Cancelled.");
            return Ok(());
        }
    }

    // Show spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.yellow} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Connecting to server...");

    // TODO(v0.7.0): Once server supports tenant deletion API, implement it here
    spinner.finish_and_clear();

    println!();
    println!("Note: Full tenant deletion API will be implemented in a future phase.");
    println!("Tenant deletion requires server-side support for safe data removal.");

    Ok(())
}

/// Show tenant information.
pub fn info(server: &str, id: u64) -> Result<()> {
    println!("Fetching tenant info for ID {id}...");

    // Show spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Connecting to server...");

    // Connect to server
    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(5)),
        write_timeout: Some(Duration::from_secs(5)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    let tenant_id = TenantId::new(id);
    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to server at {server}"))?;

    spinner.finish_and_clear();

    // TODO(v0.7.0): Once server supports tenant info API, fetch real data
    // For now, show connection success
    println!();
    println!("Tenant ID: {}", style::tenant(&id.to_string()));
    println!("Status: {}", style::success("Connected"));
    println!();
    println!("Note: Full tenant info API will be implemented in a future phase.");

    Ok(())
}
