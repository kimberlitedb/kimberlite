//! Integration tests for CLI commands.
//!
//! Tests that verify command functionality end-to-end with actual operations.
//! Note: Some tests are simplified because they require a running server.

#![allow(deprecated)] // Command::cargo_bin is deprecated but replacement requires newer assert_cmd

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Tenant Commands (require server connection - test help/validation only)
// ============================================================================

#[test]
fn tenant_create_requires_id_and_name() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn tenant_create_requires_name() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "create", "--id", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn tenant_create_help_shows_options() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--id"))
        .stdout(predicate::str::contains("--name"));
}

#[test]
fn tenant_list_help_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "list", "--help"])
        .assert()
        .success();
}

#[test]
fn tenant_delete_help_shows_options() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "delete", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Config Commands
// ============================================================================

#[test]
fn config_show_works_in_initialized_project() {
    let temp = TempDir::new().unwrap();

    // Initialize project first
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Show config
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["config", "show", "--project", temp.path().to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn config_validate_works_in_initialized_project() {
    let temp = TempDir::new().unwrap();

    // Initialize project
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Validate config
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "config",
            "validate",
            "--project",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn config_show_supports_format_options() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["config", "show", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--format"))
        .stdout(predicate::str::contains("json"));
}

#[test]
fn config_validate_help_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["config", "validate", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Cluster Commands
// ============================================================================

#[test]
fn cluster_init_creates_cluster_config() {
    let temp = TempDir::new().unwrap();

    // Initialize project first
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Initialize cluster
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "cluster",
            "init",
            "--nodes",
            "3",
            "--project",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn cluster_init_default_nodes() {
    let temp = TempDir::new().unwrap();

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Init without --nodes should use default (3)
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "cluster",
            "init",
            "--project",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn cluster_status_help_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["cluster", "status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("status"));
}

#[test]
fn cluster_stop_help_shows_options() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["cluster", "stop", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Migration Commands
// ============================================================================

#[test]
fn migration_create_generates_file() {
    let temp = TempDir::new().unwrap();

    // Initialize project (this creates migrations/ directory)
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Verify migrations directory was created by init
    let migrations_dir = temp.path().join("migrations");
    assert!(
        migrations_dir.exists(),
        "migrations directory should be created by init"
    );

    // Create migration - note we run from the project directory since
    // the CLI uses relative paths for state_dir
    Command::cargo_bin("kimberlite")
        .unwrap()
        .current_dir(temp.path())
        .args(["migration", "create", "add_users_table"])
        .assert()
        .success();

    // Verify migration file was created
    let entries: Vec<_> = fs::read_dir(&migrations_dir)
        .expect("migrations directory should exist")
        .filter_map(Result::ok)
        .collect();

    // Should have at least one migration file (plus possibly .gitkeep)
    let migration_files: Vec<_> = entries
        .iter()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == "sql" || s == "toml")
        })
        .collect();

    assert!(
        !migration_files.is_empty(),
        "migration file should be created"
    );
}

#[test]
fn migration_status_works_in_project() {
    let temp = TempDir::new().unwrap();

    // Initialize project
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", temp.path().to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Check migration status
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "migration",
            "status",
            "--project",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn migration_create_requires_name() {
    let temp = TempDir::new().unwrap();

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "migration",
            "create",
            "--project",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn migration_validate_help_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["migration", "validate", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Init Command (Extended Tests)
// ============================================================================

#[test]
fn init_creates_required_directories() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("my-project");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", project_dir.to_str().unwrap(), "--yes"])
        .assert()
        .success();

    // Verify project directory exists
    assert!(project_dir.exists());

    // Verify expected files/directories
    assert!(project_dir.join("kimberlite.toml").exists());
    assert!(project_dir.join("migrations").exists());
}

#[test]
fn init_with_yes_flag_skips_prompts() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().join("auto-init");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", project_dir.to_str().unwrap(), "--yes"])
        .assert()
        .success();

    assert!(project_dir.exists());
}

#[test]
fn init_current_directory_works() {
    let temp = TempDir::new().unwrap();

    // Change to temp directory and init with default path
    std::env::set_current_dir(&temp).unwrap();

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", "--yes"])
        .assert()
        .success();

    // Verify files were created in current directory
    assert!(temp.path().join("kimberlite.toml").exists());
}

// ============================================================================
// Stream Commands (Extended Tests)
// ============================================================================

#[test]
fn stream_help_shows_subcommands() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn stream_list_help_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"));
}

// ============================================================================
// Version and Info Commands
// ============================================================================

#[test]
fn info_command_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["info", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn config_validate_in_nonexistent_project_fails() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["config", "validate", "--project", "/nonexistent/project"])
        .assert()
        .failure();
}

#[test]
fn tenant_delete_requires_arguments() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["tenant", "delete"])
        .assert()
        .failure();
}
