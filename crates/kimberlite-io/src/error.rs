//! I/O error types.

use std::path::PathBuf;

/// Errors from the I/O backend.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// Underlying OS I/O error.
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// File not found.
    #[error("file not found: {path}")]
    NotFound { path: PathBuf },

    /// Invalid file handle.
    #[error("invalid file handle: {handle}")]
    InvalidHandle { handle: u64 },

    /// Alignment error for Direct I/O.
    #[error("buffer not aligned to {required} bytes (actual alignment: {actual})")]
    AlignmentError { required: usize, actual: usize },
}
