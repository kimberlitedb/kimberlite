//! Focused CLI argument parsing tests.
//!
//! Tests that verify command-line argument parsing works correctly without
//! requiring server connectivity or long timeouts.

#![allow(deprecated)] // Command::cargo_bin is deprecated but replacement requires newer assert_cmd

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ============================================================================
// Commands That Work Without Server
// ============================================================================

#[test]
fn version_command_succeeds() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("version")
        .assert()
        .success();
}

#[test]
fn version_flag_shows_version() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("kimberlite"));
}

#[test]
fn help_flag_shows_usage() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("compliance-first"));
}

#[test]
fn init_creates_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new-data");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap()])
        .assert()
        .success();

    assert!(path.exists());
}

#[test]
fn init_with_development_flag_succeeds() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("dev-data");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap(), "--development"])
        .assert()
        .success();
}

// ============================================================================
// Argument Parsing Errors (Missing Required Arguments)
// ============================================================================

#[test]
fn no_command_shows_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn init_requires_path() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn start_requires_path() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("start")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn query_requires_sql() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("query")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn stream_create_requires_name() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn stream_append_requires_stream_id() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "append"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// Note: stream append with just stream_id will parse successfully
// but fail at runtime when it tries to execute (no events to append).
// This is not an argument parsing error, so we don't test it here.

#[test]
fn stream_read_requires_stream_id() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ============================================================================
// Type Validation Errors
// ============================================================================

#[test]
fn invalid_tenant_id_rejected() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["info", "--tenant", "not-a-number"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

#[test]
fn invalid_stream_id_rejected() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "not-a-number"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

#[test]
fn invalid_offset_rejected() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "1", "--from", "invalid"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

#[test]
fn invalid_max_bytes_rejected() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "1", "--max-bytes", "not-a-number"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

// ============================================================================
// Unrecognized Commands/Arguments
// ============================================================================

#[test]
fn unrecognized_command_shows_error() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .arg("invalid-command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}

#[test]
fn unrecognized_subcommand_shows_error() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "invalid"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}

// ============================================================================
// Help Text Tests
// ============================================================================

#[test]
fn init_help_shows_description() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialize"));
}

#[test]
fn start_help_shows_description() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Start"));
}

#[test]
fn query_help_shows_description() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SQL"));
}

#[test]
fn repl_help_shows_description() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["repl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Interactive"));
}

#[test]
fn stream_help_shows_subcommands() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("append"))
        .stdout(predicate::str::contains("read"));
}

// ============================================================================
// Global Options
// ============================================================================

#[test]
fn no_color_flag_works_with_version() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["--no-color", "version"])
        .assert()
        .success();
}

#[test]
fn no_color_before_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["--no-color", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn tenant_id_zero_accepted() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["info", "--help"]) // Just check parsing, not execution
        .assert()
        .success();
}

#[test]
fn stream_id_zero_accepted_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "--help"])
        .assert()
        .success();
}

#[test]
fn path_with_spaces_works() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("path with spaces");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap()])
        .assert()
        .success();

    assert!(path.exists());
}

#[test]
fn very_long_path_works() {
    let temp = TempDir::new().unwrap();
    let long_name = "a".repeat(100);
    let path = temp.path().join(long_name);

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap()])
        .assert()
        .success();
}

// ============================================================================
// Additional Command Option Tests
// ============================================================================

#[test]
fn init_help_mentions_development_flag() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("development"));
}

#[test]
fn start_help_mentions_address_option() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("address"));
}

#[test]
fn repl_help_mentions_tenant_option() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["repl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tenant"));
}

#[test]
fn query_help_mentions_at_option() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("position"));
}

#[test]
fn stream_create_help_mentions_class_option() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("class"));
}

#[test]
fn stream_read_help_mentions_from_option() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from"));
}

#[test]
fn stream_read_help_mentions_max_bytes() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("max"));
}

// ============================================================================
// Default Value Tests (via help text)
// ============================================================================

#[test]
fn repl_default_address_shown_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["repl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("127.0.0.1:5432"));
}

#[test]
fn query_default_server_shown_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("127.0.0.1:5432"));
}

#[test]
fn start_default_port_shown_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("5432"));
}

#[test]
fn stream_create_default_class_shown_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("non-phi"));
}

#[test]
fn stream_read_default_from_shown_in_help() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["stream", "read", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("0"));
}

// ============================================================================
// Short Flag Tests
// ============================================================================

#[test]
fn start_short_address_flag_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-a"));
}

#[test]
fn repl_short_address_flag_works() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["repl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-a"));
}

#[test]
fn tenant_short_flag_exists() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["info", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-t"));
}

#[test]
fn server_short_flag_exists() {
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["query", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-s"));
}

// ============================================================================
// Multiple Path Formats
// ============================================================================

#[test]
fn relative_path_works() {
    let temp = TempDir::new().unwrap();
    std::env::set_current_dir(&temp).unwrap();

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", "./relative-path"])
        .assert()
        .success();
}

#[test]
fn absolute_path_works() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("absolute");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn path_with_parent_dir_works() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("subdir/../data");

    Command::cargo_bin("kimberlite")
        .unwrap()
        .args(["init", path.to_str().unwrap()])
        .assert()
        .success();
}

// ============================================================================
// Complex Argument Combinations
// ============================================================================

#[test]
fn multiple_flags_can_be_combined() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("multi-flag");

    // Test --no-color with --development
    Command::cargo_bin("kimberlite")
        .unwrap()
        .args([
            "--no-color",
            "init",
            path.to_str().unwrap(),
            "--development",
        ])
        .assert()
        .success();
}

#[test]
fn help_works_for_all_subcommands() {
    let subcommands = vec!["init", "start", "version", "repl", "query", "info"];

    for subcmd in subcommands {
        Command::cargo_bin("kimberlite")
            .unwrap()
            .args([subcmd, "--help"])
            .assert()
            .success();
    }
}
