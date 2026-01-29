//! Initialize command - creates a new data directory.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::info;

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

    // Create directory structure
    info!("Initializing data directory at: {path}");

    fs::create_dir_all(data_dir).context("Failed to create data directory")?;

    let log_dir = data_dir.join("log");
    let store_dir = data_dir.join("store");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;
    fs::create_dir_all(&store_dir).context("Failed to create store directory")?;

    // Create configuration file
    let config = if development {
        println!("Initializing in DEVELOPMENT mode (relaxed durability)");
        Config::development()
    } else {
        println!("Initializing in PRODUCTION mode");
        Config::default()
    };

    let config_path = data_dir.join("config.toml");
    let config_content =
        toml::to_string_pretty(&config).context("Failed to serialize configuration")?;
    fs::write(&config_path, config_content).context("Failed to write configuration file")?;

    // Initialize the projection store (creates superblock)
    let projections_path = data_dir.join("projections.db");
    let _store =
        kmb_store::BTreeStore::open_with_capacity(&projections_path, config.storage.cache_capacity)
            .context("Failed to initialize projection store")?;

    println!();
    println!("Data directory initialized successfully!");
    println!();
    println!(
        "  Path:   {}",
        data_dir
            .canonicalize()
            .unwrap_or(data_dir.to_path_buf())
            .display()
    );
    println!("  Config: {}", config_path.display());
    println!();
    println!("To start the server:");
    println!("  kimberlite start {path}");
    println!();
    println!("To connect with the REPL:");
    println!("  kimberlite repl");

    Ok(())
}
