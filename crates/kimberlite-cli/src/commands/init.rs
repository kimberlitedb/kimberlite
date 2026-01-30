//! Initialize command - creates a new data directory.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::style::{
    colors::SemanticStyle, create_spinner, finish_success, print_code_example, print_hint,
    print_labeled, print_spacer, print_success,
};

/// Configuration file structure for the data directory.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Server configuration.
    pub server: ServerConfig,
    /// Storage configuration.
    pub storage: StorageConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Address to bind to.
    pub bind_address: String,
    /// Maximum connections.
    pub max_connections: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Enable fsync for durability.
    pub fsync: bool,
    /// Page cache capacity in pages (4KB each).
    pub cache_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "127.0.0.1:5432".to_string(),
                max_connections: 1024,
            },
            storage: StorageConfig {
                fsync: true,
                cache_capacity: 4096, // 16MB
            },
        }
    }
}

impl Config {
    /// Creates a development configuration with relaxed durability.
    pub fn development() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "127.0.0.1:5432".to_string(),
                max_connections: 256,
            },
            storage: StorageConfig {
                fsync: false,         // Faster for development
                cache_capacity: 1024, // 4MB
            },
        }
    }
}

pub fn run(path: &str, development: bool) -> Result<()> {
    let data_dir = Path::new(path);

    // Check if directory already exists and has content
    if data_dir.exists() {
        let entries: Vec<_> = fs::read_dir(data_dir)
            .context("Failed to read directory")?
            .collect();

        if !entries.is_empty() {
            bail!(
                "Directory '{path}' already exists and is not empty. \
                 Use a different path or remove existing data."
            );
        }
    }

    // Print mode
    print_spacer();
    if development {
        println!(
            "Initializing in {} mode",
            "DEVELOPMENT".warning()
        );
    } else {
        println!(
            "Initializing in {} mode",
            "PRODUCTION".success()
        );
    }
    print_spacer();

    // Step 1: Create directories
    let sp = create_spinner("Creating directories...");
    fs::create_dir_all(data_dir).context("Failed to create data directory")?;
    let log_dir = data_dir.join("log");
    let store_dir = data_dir.join("store");
    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;
    fs::create_dir_all(&store_dir).context("Failed to create store directory")?;
    finish_success(&sp, "Created directories");

    // Step 2: Write configuration
    let sp = create_spinner("Writing configuration...");
    let config = if development {
        Config::development()
    } else {
        Config::default()
    };
    let config_path = data_dir.join("config.toml");
    let config_content =
        toml::to_string_pretty(&config).context("Failed to serialize configuration")?;
    fs::write(&config_path, config_content).context("Failed to write configuration file")?;
    finish_success(&sp, "Wrote configuration");

    // Step 3: Initialize projection store
    let sp = create_spinner("Initializing projection store...");
    let projections_path = data_dir.join("projections.db");
    let _store =
        kmb_store::BTreeStore::open_with_capacity(&projections_path, config.storage.cache_capacity)
            .context("Failed to initialize projection store")?;
    finish_success(&sp, "Initialized projection store");

    // Summary
    print_spacer();
    print_success("Data directory initialized successfully!");
    print_spacer();

    let canonical_path = data_dir
        .canonicalize()
        .unwrap_or(data_dir.to_path_buf());
    print_labeled("Path", &canonical_path.display().to_string());
    print_labeled("Config", &config_path.display().to_string());

    // Next steps
    print_spacer();
    println!("{}", "Next steps:".header());
    print_spacer();

    print_hint("Start the server:");
    print_code_example(&format!("kimberlite start {path}"));
    print_spacer();

    print_hint("Connect with the REPL:");
    print_code_example("kimberlite repl");

    Ok(())
}
