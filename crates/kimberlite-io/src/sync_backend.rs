//! Synchronous I/O backend using `std::fs`.
//!
//! This is the default backend that uses standard file system calls.
//! When the `direct_io` feature is enabled on Linux, files opened with
//! `OpenFlags::direct = true` will use `O_DIRECT` to bypass the page cache.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;

use crate::IoError;
use crate::backend::{FileHandle, IoBackend, OpenFlags};

/// Synchronous I/O backend using `std::fs::File`.
///
/// This is the default backend. All operations are blocking and use
/// the OS page cache unless Direct I/O is enabled.
#[derive(Debug)]
pub struct SyncBackend {
    /// Counter for generating unique file handle IDs.
    next_handle_id: AtomicU64,
}

impl SyncBackend {
    /// Creates a new synchronous I/O backend.
    pub fn new() -> Self {
        Self {
            next_handle_id: AtomicU64::new(1),
        }
    }

    /// Returns the next unique handle ID.
    fn next_id(&self) -> u64 {
        self.next_handle_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for SyncBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl IoBackend for SyncBackend {
    fn open(&self, path: &Path, flags: OpenFlags) -> Result<FileHandle, IoError> {
        let mut opts = OpenOptions::new();

        if flags.read {
            opts.read(true);
        }
        if flags.write {
            opts.write(true);
        }
        if flags.create {
            opts.create(true);
        }
        if flags.append {
            opts.append(true);
        }

        // Direct I/O on Linux
        #[cfg(all(target_os = "linux", feature = "direct_io"))]
        if flags.direct {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_DIRECT);
        }

        let file = opts.open(path)?;
        let id = self.next_id();
        Ok(FileHandle::from_file(id, file))
    }

    fn read_at(&self, handle: &FileHandle, offset: u64, buf: &mut [u8]) -> Result<usize, IoError> {
        // Use pread on Unix for positional read without seeking (safe wrapper)
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            let file = handle.file()?;
            let n = file.read_at(buf, offset)?;
            Ok(n)
        }

        // Fallback: seek + read on non-Unix platforms
        #[cfg(not(unix))]
        {
            use std::os::windows::fs::FileExt;
            let file = handle.file()?;
            let n = file.seek_read(buf, offset)?;
            Ok(n)
        }
    }

    fn write(&self, handle: &mut FileHandle, buf: &[u8]) -> Result<usize, IoError> {
        let file = handle.file_mut()?;
        let n = file.write(buf)?;
        Ok(n)
    }

    fn fsync(&self, handle: &FileHandle) -> Result<(), IoError> {
        handle.file()?.sync_all()?;
        Ok(())
    }

    fn close(&self, mut handle: FileHandle) -> Result<(), IoError> {
        // Drop the file to close it
        handle.file = None;
        Ok(())
    }

    fn read_all(&self, path: &Path) -> Result<Bytes, IoError> {
        let data = fs::read(path)?;
        Ok(Bytes::from(data))
    }

    fn write_all(&self, path: &Path, data: &[u8]) -> Result<(), IoError> {
        fs::write(path, data)?;
        Ok(())
    }

    fn file_size(&self, handle: &FileHandle) -> Result<u64, IoError> {
        let metadata = handle.file()?.metadata()?;
        Ok(metadata.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_backend_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.dat");
        let backend = SyncBackend::new();

        // Write data
        let mut handle = backend.open(&path, OpenFlags::append_create()).unwrap();
        let written = backend.write(&mut handle, b"hello world").unwrap();
        assert_eq!(written, 11);
        backend.fsync(&handle).unwrap();
        backend.close(handle).unwrap();

        // Read data back
        let data = backend.read_all(&path).unwrap();
        assert_eq!(&data[..], b"hello world");
    }

    #[test]
    fn sync_backend_read_at() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_read_at.dat");
        let backend = SyncBackend::new();

        // Write data
        backend.write_all(&path, b"0123456789").unwrap();

        // Read at offset
        let handle = backend.open(&path, OpenFlags::read_only()).unwrap();
        let mut buf = [0u8; 5];
        let n = backend.read_at(&handle, 3, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"34567");
        backend.close(handle).unwrap();
    }

    #[test]
    fn sync_backend_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_size.dat");
        let backend = SyncBackend::new();

        backend.write_all(&path, b"twelve bytes").unwrap();

        let handle = backend.open(&path, OpenFlags::read_only()).unwrap();
        assert_eq!(backend.file_size(&handle).unwrap(), 12);
        backend.close(handle).unwrap();
    }

    #[test]
    fn sync_backend_append_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_append.dat");
        let backend = SyncBackend::new();

        // First write
        let mut handle = backend.open(&path, OpenFlags::append_create()).unwrap();
        backend.write(&mut handle, b"hello").unwrap();
        backend.close(handle).unwrap();

        // Second write (append)
        let mut handle = backend.open(&path, OpenFlags::append_create()).unwrap();
        backend.write(&mut handle, b" world").unwrap();
        backend.close(handle).unwrap();

        // Verify
        let data = backend.read_all(&path).unwrap();
        assert_eq!(&data[..], b"hello world");
    }
}
