//! Integration tests for the I/O backend.

use crate::{AlignedBuffer, BLOCK_ALIGNMENT, IoBackend, OpenFlags, SyncBackend};

#[test]
fn aligned_buffer_roundtrip() {
    let data = b"test data for alignment";
    let buf = AlignedBuffer::from_data(data);

    // Should be padded to block alignment
    assert_eq!(buf.len(), BLOCK_ALIGNMENT);
    assert_eq!(&buf.as_slice()[..data.len()], data);
}

#[test]
fn sync_backend_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lifecycle.dat");
    let backend = SyncBackend::new();

    // Create and write
    let mut handle = backend.open(&path, OpenFlags::append_create()).unwrap();
    let data = b"kimberlite test data";
    let written = backend.write(&mut handle, data).unwrap();
    assert_eq!(written, data.len());

    // Fsync
    backend.fsync(&handle).unwrap();

    // Check size
    let size = backend.file_size(&handle).unwrap();
    assert_eq!(size, data.len() as u64);

    // Close
    backend.close(handle).unwrap();

    // Read all
    let read_data = backend.read_all(&path).unwrap();
    assert_eq!(&read_data[..], data);
}

#[test]
fn sync_backend_read_at_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boundaries.dat");
    let backend = SyncBackend::new();

    // Write known data
    let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
    backend.write_all(&path, &data).unwrap();

    let handle = backend.open(&path, OpenFlags::read_only()).unwrap();

    // Read from start
    let mut buf = [0u8; 10];
    let n = backend.read_at(&handle, 0, &mut buf).unwrap();
    assert_eq!(n, 10);
    assert_eq!(&buf, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    // Read from middle
    let n = backend.read_at(&handle, 100, &mut buf).unwrap();
    assert_eq!(n, 10);
    assert_eq!(&buf, &[100, 101, 102, 103, 104, 105, 106, 107, 108, 109]);

    // Read near end (partial read)
    let n = backend.read_at(&handle, 250, &mut buf).unwrap();
    assert_eq!(n, 6); // Only 6 bytes left
    assert_eq!(&buf[..6], &[250, 251, 252, 253, 254, 255]);

    backend.close(handle).unwrap();
}

#[test]
fn write_all_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overwrite.dat");
    let backend = SyncBackend::new();

    backend.write_all(&path, b"first").unwrap();
    backend.write_all(&path, b"second").unwrap();

    let data = backend.read_all(&path).unwrap();
    assert_eq!(&data[..], b"second");
}
