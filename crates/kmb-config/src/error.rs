//! Configuration error types

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file at {path}: {source}")]
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse TOML config at {path}: {source}")]
    ParseError {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("Failed to merge configuration: {0}")]
    MergeError(String),

    #[error("Invalid configuration: {0}")]
    ValidationError(String),

    #[error("Environment variable error: {0}")]
    EnvError(String),

    #[error("XDG directory error: {0}")]
    XdgError(String),
}
