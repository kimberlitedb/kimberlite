//! Record type for the append-only log.
//!
//! Each record contains an offset, record kind, compression kind, optional
//! link to previous record's hash, and a payload. Records are serialized with
//! CRC32 checksums for integrity.
//!
//! # Record Format
//!
//! ```text
//! [RECORD_START:u32][offset:u64][prev_hash:32B][kind:u8][compression:u8][length:u32][payload:bytes][crc32:u32][RECORD_END:u32]
//!       4B               8B           32B         1B          1B             4B         variable        4B            4B
//! ```
//!
//! **AUDIT-2026-03 M-8:** Sentinel markers (RECORD_START/END) enable torn write detection.
//! If RECORD_END is missing during recovery, the record was incompletely written (power loss).

use bytes::{Bytes, BytesMut};
use kimberlite_crypto::{ChainHash, chain_hash};
use kimberlite_types::{CompressionKind, Offset, RecordKind};

use crate::StorageError;

// **AUDIT-2026-03 M-8: Torn Write Protection**
// Magic number marking the start of a record (0xBADC0FFE in little-endian).
const RECORD_START: u32 = 0xBADC_0FFE;

// Magic number marking the end of a complete record (0xC0FFEE42 in little-endian).
const RECORD_END: u32 = 0xC0FF_EE42;

// Header size: start_sentinel(4) + offset(8) + prev_hash(32) + kind(1) + compression(1) + length(4) = 50 bytes.
const HEADER_SIZE: usize = 50;

// Total overhead per record: header(50) + crc(4) + end_sentinel(4) = 58 bytes.
const RECORD_OVERHEAD: usize = 58;

/// A single record in the event log.
///
/// Records are the on-disk representation of events. Each record contains
/// an offset (logical position), a record kind, compression kind, the event
/// payload, and is serialized with a CRC32 checksum for integrity.
///
/// # Record Kinds
///
/// - [`RecordKind::Data`]: Normal application data
/// - [`RecordKind::Checkpoint`]: Periodic verification anchor
/// - [`RecordKind::Tombstone`]: Logical deletion marker
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Record {
    offset: Offset,
    prev_hash: Option<ChainHash>,
    kind: RecordKind,
    compression: CompressionKind,
    payload: Bytes,
}

impl Record {
    /// Creates a new data record with the given offset and payload (uncompressed).
    pub fn new(offset: Offset, prev_hash: Option<ChainHash>, payload: Bytes) -> Self {
        Self {
            offset,
            prev_hash,
            kind: RecordKind::Data,
            compression: CompressionKind::None,
            payload,
        }
    }

    /// Creates a new record with a specific kind (uncompressed).
    pub fn with_kind(
        offset: Offset,
        prev_hash: Option<ChainHash>,
        kind: RecordKind,
        payload: Bytes,
    ) -> Self {
        Self {
            offset,
            prev_hash,
            kind,
            compression: CompressionKind::None,
            payload,
        }
    }

    /// Creates a new record with a specific kind and compression.
    pub fn with_compression(
        offset: Offset,
        prev_hash: Option<ChainHash>,
        kind: RecordKind,
        compression: CompressionKind,
        payload: Bytes,
    ) -> Self {
        Self {
            offset,
            prev_hash,
            kind,
            compression,
            payload,
        }
    }

    /// Returns the offset of this record.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Returns the hash of the previous record, if any.
    pub fn prev_hash(&self) -> Option<ChainHash> {
        self.prev_hash
    }

    /// Returns the kind of this record.
    pub fn kind(&self) -> RecordKind {
        self.kind
    }

    /// Returns the compression kind used for the payload.
    pub fn compression(&self) -> CompressionKind {
        self.compression
    }

    /// Returns the payload of this record.
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// Returns true if this is a checkpoint record.
    pub fn is_checkpoint(&self) -> bool {
        self.kind == RecordKind::Checkpoint
    }

    /// Computes the hash of this record for chain linking.
    ///
    /// The hash covers the kind byte and payload to ensure the record kind
    /// is part of the tamper-evident chain. The hash is computed over the
    /// *original* (uncompressed) payload.
    pub fn compute_hash(&self) -> ChainHash {
        let mut data = Vec::with_capacity(1 + self.payload.len());
        data.push(self.kind.as_byte());
        data.extend_from_slice(&self.payload);
        chain_hash(self.prev_hash.as_ref(), &data)
    }

    /// Serializes the record to bytes.
    ///
    /// Format: `[offset:u64][prev_hash:32B][kind:u8][compression:u8][length:u32][payload][crc32:u32]`
    ///
    /// All integers are little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(RECORD_OVERHEAD + self.payload.len());
        self.write_into(&mut buf);
        buf
    }

    /// Serializes the record into an existing `BytesMut` buffer (zero-copy path).
    pub fn to_bytes_into(&self, buf: &mut BytesMut) {
        buf.reserve(RECORD_OVERHEAD + self.payload.len());
        let mut tmp = Vec::with_capacity(RECORD_OVERHEAD + self.payload.len());
        self.write_into(&mut tmp);
        buf.extend_from_slice(&tmp);
    }

    /// Internal serialization into a `Vec<u8>`.
    ///
    /// **AUDIT-2026-03 M-8:** Includes sentinel markers for torn write detection.
    fn write_into(&self, buf: &mut Vec<u8>) {
        // RECORD_START sentinel (4 bytes) - AUDIT-2026-03 M-8
        buf.extend_from_slice(&RECORD_START.to_le_bytes());

        // offset (8 bytes)
        buf.extend_from_slice(&self.offset.as_u64().to_le_bytes());

        // prev_hash (32 bytes) - zeros if genesis
        match &self.prev_hash {
            Some(hash) => buf.extend_from_slice(hash.as_bytes()),
            None => buf.extend_from_slice(&[0u8; 32]),
        }

        // kind (1 byte)
        buf.push(self.kind.as_byte());

        // compression (1 byte)
        buf.push(self.compression.as_byte());

        // length (4 bytes)
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());

        // payload (variable)
        buf.extend_from_slice(&self.payload);

        // crc (4 bytes) - checksum of everything from start_sentinel to payload (inclusive)
        let crc = kimberlite_crypto::crc32(buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        // RECORD_END sentinel (4 bytes) - AUDIT-2026-03 M-8
        // If this is missing during recovery, the record was incompletely written (torn write)
        buf.extend_from_slice(&RECORD_END.to_le_bytes());
    }

    /// Deserializes a record from bytes.
    ///
    /// Returns the parsed record and the number of bytes consumed.
    /// Uses zero-copy slicing for the payload via [`Bytes::slice`].
    ///
    /// **AUDIT-2026-03 M-8:** Detects torn writes via RECORD_END sentinel check.
    ///
    /// # Errors
    ///
    /// - [`StorageError::UnexpectedEof`] if the data is truncated
    /// - [`StorageError::CorruptedRecord`] if the CRC doesn't match
    /// - [`StorageError::TornWrite`] if RECORD_START or RECORD_END sentinel is missing
    /// - [`StorageError::InvalidRecordKind`] if the kind byte is invalid
    /// - [`StorageError::InvalidCompressionKind`] if the compression byte is invalid
    pub fn from_bytes(data: &Bytes) -> Result<(Self, usize), StorageError> {
        if data.len() < HEADER_SIZE {
            return Err(StorageError::UnexpectedEof);
        }

        // **AUDIT-2026-03 M-8: Torn Write Detection**
        // Check RECORD_START sentinel (bytes 0-3)
        let start_sentinel = u32::from_le_bytes(
            data[0..4]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        );
        if start_sentinel != RECORD_START {
            return Err(StorageError::TornWrite {
                reason: "missing or corrupted RECORD_START sentinel".to_string(),
            });
        }

        // Read offset (bytes 4-11)
        let offset = Offset::new(u64::from_le_bytes(
            data[4..12]
                .try_into()
                .expect("slice is exactly 8 bytes after bounds check"),
        ));

        // Read prev_hash (bytes 12-43)
        let prev_hash_bytes: [u8; 32] = data[12..44]
            .try_into()
            .expect("slice is exactly 32 bytes after bounds check");
        let prev_hash = if prev_hash_bytes == [0u8; 32] {
            None
        } else {
            Some(ChainHash::from_bytes(&prev_hash_bytes))
        };

        // Read kind (byte 44)
        let kind = RecordKind::from_byte(data[44]).ok_or(StorageError::InvalidRecordKind {
            byte: data[44],
            offset,
        })?;

        // Read compression (byte 45)
        let compression =
            CompressionKind::from_byte(data[45]).ok_or(StorageError::InvalidCompressionKind {
                byte: data[45],
                offset,
            })?;

        // Read length (bytes 46-49)
        let length = u32::from_le_bytes(
            data[46..50]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        ) as usize;

        // Check we have enough for payload + crc(4) + end_sentinel(4)
        let total_size = HEADER_SIZE + length + 4 + 4;
        if data.len() < total_size {
            return Err(StorageError::UnexpectedEof);
        }

        // Read payload (bytes 50..50+length) - zero-copy!
        let payload = data.slice(HEADER_SIZE..HEADER_SIZE + length);

        // Read and verify CRC (bytes 50+length..54+length)
        let crc_offset = HEADER_SIZE + length;
        let stored_crc = u32::from_le_bytes(
            data[crc_offset..crc_offset + 4]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        );
        let computed_crc = kimberlite_crypto::crc32(&data[0..crc_offset]);

        if stored_crc != computed_crc {
            return Err(StorageError::CorruptedRecord);
        }

        // **AUDIT-2026-03 M-8: Torn Write Detection**
        // Check RECORD_END sentinel (bytes 54+length..58+length)
        let end_sentinel_offset = crc_offset + 4;
        let end_sentinel = u32::from_le_bytes(
            data[end_sentinel_offset..end_sentinel_offset + 4]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        );
        if end_sentinel != RECORD_END {
            return Err(StorageError::TornWrite {
                reason: format!(
                    "missing or corrupted RECORD_END sentinel at offset {}: expected {:#010x}, found {:#010x}",
                    offset.as_u64(),
                    RECORD_END,
                    end_sentinel
                ),
            });
        }

        Ok((
            Record {
                offset,
                prev_hash,
                kind,
                compression,
                payload,
            },
            total_size,
        ))
    }
}
