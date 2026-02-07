//! Migration workflow commands.

use anyhow::{Context, Result};
use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use indicatif::{ProgressBar, ProgressStyle};
use kimberlite_migration::{MigrationConfig, MigrationManager};
use std::path::Path;

use crate::style::{self, colors::SemanticStyle};

/// Create a new migration file.
pub fn create(name: &str, project: &str) -> Result<()> {
    println!(
        "Creating migration {} in project {}...",
        name.header(),
        project.code()
    );

    let project_path = Path::new(project);

    // Load config
    let config = MigrationConfig::with_migrations_dir(project_path.join("migrations"));
    let manager = MigrationManager::new(config)
        .with_context(|| format!("Failed to initialize migration manager at {project}"))?;

    // Create migration file
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Creating migration file...");

    let file = manager
        .create(name)
        .with_context(|| format!("Failed to create migration '{name}'"))?;

    spinner.finish_with_message(format!(
        "{} Created {}",
        style::success("✓"),
        file.path.display().to_string().code()
    ));

    println!();
    println!("Migration ID: {}", file.migration.id);
    println!("File: {}", file.path.display().to_string().code());
    println!();
    println!("Edit the file to add your SQL migration, then run:");
    println!("  {} migration apply", "kmb".code());

    Ok(())
}

/// Apply pending migrations.
pub fn apply(to: Option<u64>, project: &str) -> Result<()> {
    println!("Applying pending migrations in {}...", project.code());

    let project_path = Path::new(project);

    // Load config
    let config = MigrationConfig::with_migrations_dir(project_path.join("migrations"));
    let state_dir = project_path.join(".kimberlite/migrations");
    let config = MigrationConfig {
        migrations_dir: config.migrations_dir,
        state_dir,
        ..config
    };

    let manager = MigrationManager::new(config)
        .with_context(|| format!("Failed to initialize migration manager at {project}"))?;

    // Get pending migrations
    let mut pending = manager
        .list_pending()
        .with_context(|| "Failed to list pending migrations")?;

    if pending.is_empty() {
        println!("{} No pending migrations", style::success("✓"));
        return Ok(());
    }

    // Filter by target migration ID if specified
    if let Some(target_id) = to {
        pending.retain(|f| u64::from(f.migration.id) <= target_id);
    }

    println!();
    println!("Pending migrations:");
    for file in &pending {
        println!(
            "  {} {}",
            file.migration.id.to_string().code(),
            file.migration.name.header()
        );
    }
    println!();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );

    let mut applied_count = 0;

    for file in &pending {
        spinner.set_message(format!("Applying {}...", file.migration.name));

        // Extract the UP SQL (everything before "-- Down Migration" marker)
        let up_sql = MigrationManager::up_sql(file);

        if up_sql.is_empty() {
            spinner.finish_with_message(format!(
                "{} Skipped {} {} (empty SQL)",
                "⏭".warning(),
                file.migration.id.to_string().code(),
                file.migration.name.header()
            ));
            continue;
        }

        // Try to connect to running server and execute SQL
        match try_execute_migration_sql(up_sql, project_path) {
            Ok(()) => {}
            Err(e) => {
                spinner.finish_with_message(format!(
                    "{} Failed {} {}",
                    style::error("✗"),
                    file.migration.id.to_string().code(),
                    file.migration.name.header()
                ));
                return Err(e).with_context(|| {
                    format!(
                        "Migration {} '{}' failed",
                        file.migration.id, file.migration.name
                    )
                });
            }
        }

        // Record as applied
        manager
            .record_applied(file)
            .with_context(|| format!("Failed to record migration {} as applied", file.migration.id))?;

        applied_count += 1;

        spinner.finish_with_message(format!(
            "{} Applied {} {}",
            style::success("✓"),
            file.migration.id.to_string().code(),
            file.migration.name.header()
        ));
    }

    println!();
    println!(
        "{} Applied {} migration(s)",
        style::success("✓"),
        applied_count
    );

    Ok(())
}

/// Attempts to execute migration SQL against a running Kimberlite server.
///
/// Falls back to recording only if no server is available.
fn try_execute_migration_sql(sql: &str, project_path: &Path) -> Result<()> {
    use kimberlite_client::{Client, ClientConfig};
    use kimberlite_types::TenantId;

    // Load server address from project config
    let bind_address = match kimberlite_config::KimberliteConfig::load_from_dir(project_path) {
        Ok(config) => config.database.bind_address,
        Err(_) => "127.0.0.1:5432".to_string(),
    };

    // Connect to server
    let config = ClientConfig::default();
    let mut client = Client::connect(&bind_address, TenantId::new(1), config)
        .with_context(|| format!("Cannot connect to Kimberlite at {bind_address}. Is the server running?"))?;

    // Execute each statement in the migration SQL
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() || stmt.starts_with("--") {
            continue;
        }

        client
            .query(stmt, &[])
            .with_context(|| format!("SQL execution failed: {stmt}"))?;
    }

    Ok(())
}

/// Rollback migrations.
pub fn rollback(count: u64, project: &str) -> Result<()> {
    println!("Rolling back migrations in {}...", project.code());

    let project_path = Path::new(project);

    // Load config
    let config = MigrationConfig::with_migrations_dir(project_path.join("migrations"));
    let state_dir = project_path.join(".kimberlite/migrations");
    let config = MigrationConfig {
        migrations_dir: config.migrations_dir,
        state_dir,
        ..config
    };

    let manager = MigrationManager::new(config)
        .with_context(|| format!("Failed to initialize migration manager at {project}"))?;

    // Get all applied migrations, sorted by ID descending for rollback order
    let all_files = manager
        .list_files()
        .with_context(|| "Failed to list migration files")?;

    let pending = manager
        .list_pending()
        .with_context(|| "Failed to list pending migrations")?;

    let pending_ids: std::collections::HashSet<_> =
        pending.iter().map(|f| f.migration.id).collect();

    // Get applied migrations in reverse order
    let mut applied: Vec<_> = all_files
        .iter()
        .filter(|f| !pending_ids.contains(&f.migration.id))
        .collect();
    applied.sort_by(|a, b| b.migration.id.cmp(&a.migration.id));

    // Limit to requested count
    let to_rollback: Vec<_> = applied.into_iter().take(count as usize).collect();

    if to_rollback.is_empty() {
        println!("{} No migrations to rollback", style::success("✓"));
        return Ok(());
    }

    println!();
    println!("Migrations to rollback:");
    for file in &to_rollback {
        println!(
            "  {} {}",
            file.migration.id.to_string().code(),
            file.migration.name.header()
        );
    }
    println!();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );

    let mut rolled_back = 0;

    for file in &to_rollback {
        spinner.set_message(format!("Rolling back {}...", file.migration.name));

        // Get DOWN SQL
        let down_sql = MigrationManager::down_sql(file);

        match down_sql {
            Some(sql) if !sql.is_empty() => {
                // Execute rollback SQL
                match try_execute_migration_sql(sql, project_path) {
                    Ok(()) => {}
                    Err(e) => {
                        spinner.finish_with_message(format!(
                            "{} Failed to rollback {} {}",
                            style::error("✗"),
                            file.migration.id.to_string().code(),
                            file.migration.name.header()
                        ));
                        return Err(e).with_context(|| {
                            format!(
                                "Rollback of migration {} '{}' failed",
                                file.migration.id, file.migration.name
                            )
                        });
                    }
                }
            }
            _ => {
                println!(
                    "  {} No DOWN SQL for migration {} — removing tracker entry only",
                    "⚠".warning(),
                    file.migration.id
                );
            }
        }

        // Remove from tracker
        manager
            .remove_applied(file.migration.id)
            .with_context(|| {
                format!(
                    "Failed to remove migration {} from tracker",
                    file.migration.id
                )
            })?;

        rolled_back += 1;

        spinner.finish_with_message(format!(
            "{} Rolled back {} {}",
            style::success("✓"),
            file.migration.id.to_string().code(),
            file.migration.name.header()
        ));
    }

    println!();
    println!(
        "{} Rolled back {} migration(s)",
        style::success("✓"),
        rolled_back
    );

    Ok(())
}

/// Show migration status.
pub fn status(project: &str) -> Result<()> {
    let project_path = Path::new(project);

    // Load config
    let config = MigrationConfig::with_migrations_dir(project_path.join("migrations"));
    let state_dir = project_path.join(".kimberlite/migrations");
    let config = MigrationConfig {
        migrations_dir: config.migrations_dir,
        state_dir,
        ..config
    };

    let manager = MigrationManager::new(config)
        .with_context(|| format!("Failed to initialize migration manager at {project}"))?;

    // Get all migrations
    let all_files = manager
        .list_files()
        .with_context(|| "Failed to list migration files")?;

    let pending = manager
        .list_pending()
        .with_context(|| "Failed to list pending migrations")?;

    if all_files.is_empty() {
        println!("{} No migrations found", "ℹ".info());
        println!();
        println!("Create your first migration with:");
        println!("  {} migration create <name>", "kmb".code());
        return Ok(());
    }

    // Create table
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Blue),
        Cell::new("Name").fg(Color::Blue),
        Cell::new("Status").fg(Color::Blue),
        Cell::new("Checksum").fg(Color::Blue),
    ]);

    let pending_ids: std::collections::HashSet<_> =
        pending.iter().map(|f| f.migration.id).collect();

    for file in &all_files {
        let status = if pending_ids.contains(&file.migration.id) {
            Cell::new("Pending").fg(Color::Yellow)
        } else {
            Cell::new("Applied").fg(Color::Green)
        };

        let checksum_short = &file.checksum[..8];

        table.add_row(vec![
            Cell::new(file.migration.id),
            Cell::new(&file.migration.name),
            status,
            Cell::new(checksum_short),
        ]);
    }

    println!();
    println!("Migration Status");
    println!("{table}");
    println!();

    let applied_count = all_files.len() - pending.len();
    println!(
        "Applied: {} | Pending: {}",
        style::success(&applied_count.to_string()),
        if pending.is_empty() {
            style::success(&pending.len().to_string())
        } else {
            pending.len().to_string().warning()
        }
    );

    Ok(())
}

/// Validate migration files.
pub fn validate(project: &str) -> Result<()> {
    println!("Validating migrations in {}...", project.code());

    let project_path = Path::new(project);

    // Load config
    let config = MigrationConfig::with_migrations_dir(project_path.join("migrations"));
    let state_dir = project_path.join(".kimberlite/migrations");
    let config = MigrationConfig {
        migrations_dir: config.migrations_dir,
        state_dir,
        ..config
    };

    let manager = MigrationManager::new(config)
        .with_context(|| format!("Failed to initialize migration manager at {project}"))?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Valid template"),
    );
    spinner.set_message("Validating...");

    manager
        .validate()
        .with_context(|| "Migration validation failed")?;

    spinner.finish_with_message(format!("{} All migrations are valid", style::success("✓")));

    println!();
    println!("Validation checks:");
    println!("  {} File checksums match lock file", style::success("✓"));
    println!("  {} Migration sequence is continuous", style::success("✓"));
    println!("  {} No gaps in migration IDs", style::success("✓"));

    Ok(())
}
