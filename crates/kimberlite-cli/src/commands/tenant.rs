//! Tenant management commands.

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use kimberlite_client::{Client, ClientConfig};
use kimberlite_types::TenantId;
use std::io::{self, Write};
use std::time::Duration;

use super::query::format_value;
use crate::style::{self, colors::SemanticStyle, create_spinner, finish_and_clear, finish_success};

/// Create a new tenant.
///
/// Tenants are auto-created on first connection. This command verifies
/// connectivity and reports the tenant as ready.
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

    let sp = create_spinner("Connecting to server...");

    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(5)),
        write_timeout: Some(Duration::from_secs(5)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    let tenant_id = TenantId::new(id);
    // Connecting with a tenant ID auto-creates the tenant
    let _client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to server at {server}"))?;

    finish_success(
        &sp,
        &format!("Tenant {} created (ID: {})", style::tenant(name), id),
    );

    println!();
    println!("Tenant is ready. Connect with:");
    println!("  {} repl --tenant {}", "kmb".code(), id);

    Ok(())
}

/// List all tenants.
///
/// Attempts to query schema info. Without a dedicated tenant enumeration API,
/// this connects to known tenant IDs and checks for tables.
pub fn list(server: &str) -> Result<()> {
    let sp = create_spinner("Discovering tenants...");

    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(2)),
        write_timeout: Some(Duration::from_secs(2)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    // Probe tenants 1-10 for connectivity
    let mut found = Vec::new();
    for id in 1..=10 {
        let tenant_id = TenantId::new(id);
        if let Ok(mut client) = Client::connect(server, tenant_id, config.clone()) {
            let table_count = client
                .query("SELECT name FROM _tables", &[])
                .map(|r| r.rows.len())
                .unwrap_or(0);
            found.push((id, table_count));
        }
    }

    finish_and_clear(&sp);

    if found.is_empty() {
        println!("No tenants found on {server}.");
        println!();
        println!("Create a tenant with:");
        println!("  {} tenant create --id 1 --name my-tenant", "kmb".code());
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Cyan),
        Cell::new("Status").fg(Color::Cyan),
        Cell::new("Tables").fg(Color::Cyan),
    ]);

    for (id, table_count) in &found {
        table.add_row(vec![
            Cell::new(id),
            Cell::new("Active").fg(Color::Green),
            Cell::new(table_count),
        ]);
    }

    println!("{table}");
    println!();
    println!("Found {} tenant(s) on {server}", found.len());

    Ok(())
}

/// Delete a tenant.
///
/// Drops all tables in the tenant. The tenant namespace remains but is empty.
pub fn delete(server: &str, id: u64, force: bool) -> Result<()> {
    println!("Deleting tenant ID {id}...");

    // Confirmation prompt unless --force
    if !force {
        print!(
            "{}",
            style::error(&format!(
                "WARNING: This will drop all tables in tenant {id}!\n"
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

    let sp = create_spinner("Connecting to server...");

    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(5)),
        write_timeout: Some(Duration::from_secs(5)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    let tenant_id = TenantId::new(id);
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to server at {server}"))?;

    // Get list of tables and drop each one
    let tables: Vec<String> = client
        .query("SELECT name FROM _tables", &[])
        .map(|r| {
            r.rows
                .iter()
                .filter_map(|row| row.first().map(format_value))
                .collect()
        })
        .unwrap_or_default();

    if tables.is_empty() {
        finish_and_clear(&sp);
        println!("Tenant {id} has no tables. Nothing to delete.");
        return Ok(());
    }

    for table_name in &tables {
        let drop_sql = format!("DROP TABLE {table_name}");
        if let Err(e) = client.query(&drop_sql, &[]) {
            finish_and_clear(&sp);
            println!(
                "{}",
                format!("Failed to drop table {table_name}: {e}").warning()
            );
        }
    }

    finish_success(
        &sp,
        &format!("Tenant {id}: dropped {} table(s)", tables.len()),
    );

    Ok(())
}

/// Show tenant information.
///
/// Connects to the tenant and queries schema info.
pub fn info(server: &str, id: u64) -> Result<()> {
    let sp = create_spinner(&format!("Fetching info for tenant {id}..."));

    let config = ClientConfig {
        read_timeout: Some(Duration::from_secs(5)),
        write_timeout: Some(Duration::from_secs(5)),
        buffer_size: 16 * 1024 * 1024,
        auth_token: None,
    };

    let tenant_id = TenantId::new(id);
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to server at {server}"))?;

    finish_and_clear(&sp);

    println!();
    println!("Tenant ID: {}", style::tenant(&id.to_string()));
    println!("Status: {}", style::success("Connected"));

    // Query table list
    match client.query("SELECT name FROM _tables", &[]) {
        Ok(result) => {
            if result.rows.is_empty() {
                println!("Tables: {}", "none".muted());
            } else {
                println!("Tables ({}):", result.rows.len());
                for row in &result.rows {
                    if let Some(value) = row.first() {
                        println!("  {}", format_value(value).code());
                    }
                }
            }
        }
        Err(_) => {
            println!("Tables: {}", "unable to query".muted());
        }
    }

    Ok(())
}
