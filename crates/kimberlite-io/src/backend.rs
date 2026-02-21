//! I/O backend trait.
//!
//! The [`IoBackend`] trait abstracts file I/O operations to enable:
//! - Standard `std::fs` I/O (default)
//! - Direct I/O (O_DIRECT on Linux) for bypassing the page cache
//! - Future: io_uring for async I/O
//!
//! This abstraction allows the storage layer to be tested with mock backends
//! and supports future async I/O without changing the storage API.

use std::path::Path;

use bytes::Bytes;

use crate::IoError;

/// Flags for opening files.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenFlags {
    /// Open for reading.
    pub read: bool,
    /// Open for writing.
    pub write: bool,
    /// Create the file if it doesn't exist.
    pub create: bool,
    /// Open in append mode.
    pub append: bool,
    /// Use Direct I/O (O_DIRECT on Linux, ignored elsewhere).
    pub direct: bool,
}

impl OpenFlags {
    /// Flags for reading an existing file.
    pub fn read_only() -> Self {
        Self {
            read: true,
            ..Self::default()
        }
    }

    /// Flags for creating or appending to a file.
    pub fn append_create() -> Self {
        Self {
            read: true,
            write: true,
            create: true,
            append: true,
            ..Self::default()
        }
    }

    /// Flags for creating or appending with Direct I/O.
    pub fn append_create_direct() -> Self {
        Self {
            read: true,
            write: true,
            create: true,
            append: true,
            direct: true,
        }
    }
}

/// Opaque handle to an open file.
///
/// The handle is backend-specific. For `SyncBackend`, it wraps a `std::fs::File`
/// descriptor. The handle must be closed via [`IoBackend::close`].
#[derive(Debug)]
pub struct FileHandle {
    /// Internal file descriptor or identifier.
    pub(crate) id: u64,
    /// The open file (for sync backend).
    pub(crate) file: Option<std::fs::File>,
}

impl FileHandle {
    /// Creates a new file handle wrapping a `std::fs::File`.
    pub(crate) fn from_file(id: u64, file: std::fs::File) -> Self {
        Self {
            id,
            file: Some(file),
        }
    }

    /// Returns the internal file reference.
    pub(crate) fn file(&self) -> Result<&std::fs::File, IoError> {
        self.file
            .as_ref()
            .ok_or(IoError::InvalidHandle { handle: self.id })
    }

    /// Returns the internal file reference mutably.
    pub(crate) fn file_mut(&mut self) -> Result<&mut std::fs::File, IoError> {
        self.file
            .as_mut()
            .ok_or(IoError::InvalidHandle { handle: self.id })
    }
}

/// Abstraction over file I/O operations.
///
/// Implementations provide different I/O strategies (standard, Direct I/O,
/// io_uring) while presenting a uniform interface to the storage layer.
///
/// All methods are synchronous. Future async backends (io_uring) will use
/// a different trait or polling mechanism.
pub trait IoBackend: Send + Sync {
    /// Opens a file with the given flags.
    fn open(&self, path: &Path, flags: OpenFlags) -> Result<FileHandle, IoError>;

    /// Reads data from a file at the given byte offset.
    ///
    /// Returns the number of bytes read.
    fn read_at(&self, handle: &FileHandle, offset: u64, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Writes data to a file (at the current position or end in append mode).
    ///
    /// Returns the number of bytes written.
    fn write(&self, handle: &mut FileHandle, buf: &[u8]) -> Result<usize, IoError>;

    /// Syncs file data and metadata to disk.
    fn fsync(&self, handle: &FileHandle) -> Result<(), IoError>;

    /// Closes a file handle.
    fn close(&self, handle: FileHandle) -> Result<(), IoError>;

    /// Reads an entire file into memory.
    ///
    /// Convenience method for small files (manifests, indexes).
    fn read_all(&self, path: &Path) -> Result<Bytes, IoError>;

    /// Writes data to a file atomically (write + fsync).
    ///
    /// Convenience method for small files (manifests, indexes).
    fn write_all(&self, path: &Path, data: &[u8]) -> Result<(), IoError>;

    /// Returns the file size in bytes.
    fn file_size(&self, handle: &FileHandle) -> Result<u64, IoError>;
}
