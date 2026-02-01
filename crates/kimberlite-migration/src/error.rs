//! Error types for migration system.

use std::path::PathBuf;
use thiserror::Error;

/// Migration system errors.
#[derive(Error, Debug)]
pub enum Error {
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Migration file parse error.
    #[error("Failed to parse migration file {path}: {reason}")]
    ParseError { path: PathBuf, reason: String },

    /// Migration file not found.
    #[error("Migration file not found: {0}")]
    NotFound(PathBuf),

    /// Invalid migration sequence.
    #[error("Invalid migration sequence: expected {expected}, found {found}")]
    InvalidSequence { expected: u32, found: u32 },

    /// Checksum mismatch (tampering detected).
    #[error("Checksum mismatch for migration {id}: expected {expected}, found {actual}")]
    ChecksumMismatch {
        id: u32,
        expected: String,
        actual: String,
    },

    /// Migration already applied.
    #[error("Migration {0} has already been applied")]
    AlreadyApplied(u32),

    /// Invalid migration name.
    #[error("Invalid migration name: {0}")]
    InvalidName(String),

    /// Lock file error.
    #[error("Lock file error: {0}")]
    LockFile(String),

    /// TOML deserialization error.
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    /// TOML serialization error.
    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

/// Result type for migration operations.
pub type Result<T> = std::result::Result<T, Error>;
