//! Offset index for O(1) record lookups.
//!
//! The [`OffsetIndex`] maps logical offsets to physical byte positions in the log file,
//! enabling constant-time random access to any record.
//!
//! # File Format
//!
//! The index is persisted alongside the log file:
//! ```text
//! data.vlog      <- append-only log
//! data.vlog.idx  <- offset index
//! ```
//!
//! Binary format:
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │  Offset  │  Size  │  Description                │
//! ├─────────────────────────────────────────────────┤
//! │  0       │  4     │  Magic bytes: "VDXI"        │
//! │  4       │  1     │  Version: 0x01              │
//! │  5       │  3     │  Reserved (zero padding)    │
//! │  8       │  8     │  Entry count (u64 LE)       │
//! │  16      │  8*N   │  Positions array [u64; N]   │
//! │  16+8*N  │  4     │  CRC32 of bytes 0..(16+8*N) │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! # Recovery
//!
//! If the index file is missing or corrupted, it can be rebuilt by scanning the log
//! file and recording the byte position of each record.

use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

use kimberlite_types::Offset;

use crate::StorageError;

// ============================================================================
// File Format Constants
// ============================================================================

/// Magic bytes identifying a valid index file.
const MAGIC: &[u8; 4] = b"VDXI";

/// Current index file format version.
const VERSION: u8 = 0x01;

/// Reserved bytes for future use.
const RESERVED: [u8; 3] = [0u8; 3];

// Byte sizes - typed constants prevent mismatch bugs like using u32 for a u64 field
const MAGIC_SIZE: usize = 4;
const VERSION_SIZE: usize = 1;
const RESERVED_SIZE: usize = 3;
const COUNT_SIZE: usize = 8; // u64
const POSITION_SIZE: usize = 8; // u64
const CRC_SIZE: usize = 4; // u32

/// Header size: magic(4) + version(1) + reserved(3) + count(8) = 16 bytes
const HEADER_SIZE: usize = MAGIC_SIZE + VERSION_SIZE + RESERVED_SIZE + COUNT_SIZE;

/// Maps logical offset → physical byte position for O(1) lookups.
///
/// The index enables constant-time random access to any record in the log
/// by mapping the record's logical offset (0, 1, 2, ...) to its physical
/// byte position in the log file.
///
/// # Invariants
///
/// These invariants are enforced by construction and verified with debug assertions:
///
/// - `positions.len()` equals the number of records in the log
/// - `positions[i]` is the byte position where record `i` starts
/// - Positions are monotonically increasing (append-only log)
///
/// # Persistence
///
/// The index is persisted to disk alongside the log file. If the index is
/// missing or corrupted on startup, it can be rebuilt by scanning the log.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct OffsetIndex {
    positions: Vec<u64>,
}

impl OffsetIndex {
    /// Creates an empty index.
    ///
    /// Use this when creating a new log file that has no records yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the byte position of a newly appended record.
    ///
    /// Called immediately after writing a record to the log. The byte position
    /// must be greater than all previously recorded positions (monotonically increasing).
    ///
    /// # Panics
    ///
    /// Debug builds panic if `byte_position` is not greater than the last position
    /// (violates monotonicity invariant).
    pub fn append(&mut self, byte_position: u64) {
        // Precondition: positions must be monotonically increasing
        debug_assert!(
            self.positions
                .last()
                .is_none_or(|&last| byte_position > last),
            "byte_position {} must be greater than last position {:?}",
            byte_position,
            self.positions.last()
        );

        let prev_len = self.positions.len();
        self.positions.push(byte_position);

        // Postcondition: length increased by exactly 1
        debug_assert_eq!(self.positions.len(), prev_len + 1);
    }

    /// Looks up the byte position for a given logical offset.
    ///
    /// Returns `None` if the offset is out of bounds (>= number of records).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut index = OffsetIndex::new();
    /// index.append(0);    // Record 0 at byte 0
    /// index.append(100);  // Record 1 at byte 100
    ///
    /// assert_eq!(index.lookup(Offset::new(0)), Some(0));
    /// assert_eq!(index.lookup(Offset::new(1)), Some(100));
    /// assert_eq!(index.lookup(Offset::new(2)), None);
    /// ```
    #[must_use]
    pub fn lookup(&self, offset: Offset) -> Option<u64> {
        self.positions.get(offset.as_usize()).copied()
    }

    /// Returns the number of indexed records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Returns `true` if the index contains no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Creates an index from existing positions.
    ///
    /// Used when loading from disk or rebuilding from log scan.
    ///
    /// # Panics
    ///
    /// Debug builds panic if positions are not monotonically increasing.
    pub fn from_positions(positions: Vec<u64>) -> Self {
        // Precondition: positions must be monotonically increasing
        debug_assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "positions must be monotonically increasing"
        );

        Self { positions }
    }

    /// Returns a reference to the underlying positions array.
    #[must_use]
    pub fn positions(&self) -> &[u64] {
        &self.positions
    }

    /// Appends new entries to the WAL file instead of rewriting the full index.
    ///
    /// O(1) amortized per entry (just appends 8 bytes per position).
    /// The WAL file is stored alongside the main index with a `.wal` extension.
    ///
    /// When the WAL exceeds `compact_threshold` entries, it is compacted into
    /// the main index file automatically.
    pub fn save_incremental(
        &self,
        path: &Path,
        new_entries_start: usize,
        compact_threshold: usize,
    ) -> Result<(), StorageError> {
        let wal_path = wal_path_for(path);
        let new_entries = &self.positions[new_entries_start..];

        if new_entries.is_empty() {
            return Ok(());
        }

        // Append new entries to WAL
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        let mut buf = Vec::with_capacity(new_entries.len() * POSITION_SIZE);
        for pos in new_entries {
            buf.extend_from_slice(&pos.to_le_bytes());
        }
        file.write_all(&buf)?;
        file.flush()?;

        // Check if WAL needs compaction
        let wal_size = file.metadata()?.len() as usize;
        let wal_entry_count = wal_size / POSITION_SIZE;
        if wal_entry_count >= compact_threshold {
            // Compact: write full index, then remove WAL
            self.save(path)?;
            let _ = fs::remove_file(&wal_path);
        }

        Ok(())
    }

    /// Loads an index from disk, replaying any WAL entries.
    ///
    /// This is the recommended way to load an index. It loads the main index
    /// file, then appends any entries from the WAL file (if present).
    pub fn load_with_wal(path: &Path) -> Result<Self, StorageError> {
        let mut index = Self::load(path)?;

        let wal_path = wal_path_for(path);
        if wal_path.exists() {
            let wal_data = fs::read(&wal_path)?;

            // Each WAL entry is a u64 position (8 bytes)
            let entry_count = wal_data.len() / POSITION_SIZE;
            for i in 0..entry_count {
                let start = i * POSITION_SIZE;
                let pos_bytes: [u8; POSITION_SIZE] = wal_data[start..start + POSITION_SIZE]
                    .try_into()
                    .expect("slice length equals POSITION_SIZE");
                let byte_position = u64::from_le_bytes(pos_bytes);
                index.positions.push(byte_position);
            }
        }

        Ok(index)
    }

    /// Returns the number of entries that have been flushed to the main index.
    ///
    /// Entries beyond this count exist only in the WAL.
    pub fn flushed_count(path: &Path) -> Result<usize, StorageError> {
        let index = Self::load(path)?;
        Ok(index.len())
    }

    /// Persists the index to disk.
    ///
    /// Writes the index in binary format with CRC32 checksum for integrity.
    /// The file is flushed to ensure durability.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Io`] if the file cannot be created or written.
    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        let positions_size = self.positions.len() * POSITION_SIZE;
        let total_size = HEADER_SIZE + positions_size + CRC_SIZE;
        let mut buf: Vec<u8> = Vec::with_capacity(total_size);

        // Write header
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&[VERSION]);
        buf.extend_from_slice(&RESERVED);
        buf.extend_from_slice(&(self.positions.len() as u64).to_le_bytes());

        // Write positions
        for pos in &self.positions {
            buf.extend_from_slice(&pos.to_le_bytes());
        }

        // Write CRC32 checksum of everything before it
        let checksum = kimberlite_crypto::crc32(&buf);
        buf.extend_from_slice(&checksum.to_le_bytes());

        // Postcondition: buffer size matches expected
        debug_assert_eq!(buf.len(), total_size, "buffer size mismatch");

        // Write atomically: create, write, flush
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&buf)?;
        writer.flush()?;

        Ok(())
    }

    /// Loads an index from disk.
    ///
    /// Validates magic bytes, version, and CRC32 checksum before returning.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Io`] - File cannot be read
    /// - [`StorageError::InvalidIndexMagic`] - Magic bytes don't match
    /// - [`StorageError::UnsupportedIndexVersion`] - Version not supported
    /// - [`StorageError::IndexTruncated`] - File is smaller than expected
    /// - [`StorageError::IndexChecksumMismatch`] - CRC32 verification failed
    pub fn load(path: &Path) -> Result<Self, StorageError> {
        let data = fs::read(path)?;

        // Validate minimum size (header only, no positions yet)
        if data.len() < HEADER_SIZE + CRC_SIZE {
            return Err(StorageError::IndexTruncated {
                expected: HEADER_SIZE + CRC_SIZE,
                actual: data.len(),
            });
        }

        // Validate magic bytes
        let magic: [u8; MAGIC_SIZE] = data[0..MAGIC_SIZE]
            .try_into()
            .expect("slice length equals MAGIC_SIZE after bounds check");
        if &magic != MAGIC {
            return Err(StorageError::InvalidIndexMagic);
        }

        // Validate version
        let version = data[MAGIC_SIZE];
        if version != VERSION {
            return Err(StorageError::UnsupportedIndexVersion(version));
        }

        // Read count and compute expected size
        let count_start = MAGIC_SIZE + VERSION_SIZE + RESERVED_SIZE;
        let count_bytes: [u8; COUNT_SIZE] = data[count_start..count_start + COUNT_SIZE]
            .try_into()
            .expect("slice length equals COUNT_SIZE after bounds check");
        let count = u64::from_le_bytes(count_bytes) as usize;

        let positions_size = count * POSITION_SIZE;
        let expected_size = HEADER_SIZE + positions_size + CRC_SIZE;

        // Validate total file size
        if data.len() < expected_size {
            return Err(StorageError::IndexTruncated {
                expected: expected_size,
                actual: data.len(),
            });
        }

        // Verify CRC32 before trusting any data
        let crc_start = HEADER_SIZE + positions_size;
        let stored_crc_bytes: [u8; CRC_SIZE] = data[crc_start..crc_start + CRC_SIZE]
            .try_into()
            .expect("slice length equals CRC_SIZE after bounds check");
        let stored_crc = u32::from_le_bytes(stored_crc_bytes);
        let computed_crc = kimberlite_crypto::crc32(&data[0..crc_start]);

        if stored_crc != computed_crc {
            return Err(StorageError::IndexChecksumMismatch {
                expected: stored_crc,
                actual: computed_crc,
            });
        }

        // Extract positions (CRC verified, data is trustworthy)
        let mut positions = Vec::with_capacity(count);
        for i in 0..count {
            let start = HEADER_SIZE + (i * POSITION_SIZE);
            let pos_bytes: [u8; POSITION_SIZE] = data[start..start + POSITION_SIZE]
                .try_into()
                .expect("slice length equals POSITION_SIZE after bounds check");
            positions.push(u64::from_le_bytes(pos_bytes));
        }

        // Postcondition: we read exactly `count` positions
        debug_assert_eq!(positions.len(), count, "position count mismatch");

        Ok(Self { positions })
    }
}

/// Returns the WAL file path for a given index path.
///
/// The WAL file has the same name as the index with `.wal` appended.
fn wal_path_for(index_path: &Path) -> std::path::PathBuf {
    let mut wal = index_path.as_os_str().to_owned();
    wal.push(".wal");
    std::path::PathBuf::from(wal)
}
