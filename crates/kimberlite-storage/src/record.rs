//! Record type for the append-only log.
//!
//! Each record contains an offset, record kind, compression kind, optional
//! link to previous record's hash, and a payload. Records are serialized with
//! CRC32 checksums for integrity.
//!
//! # Record Format
//!
//! ```text
//! [offset:u64][prev_hash:32B][kind:u8][compression:u8][length:u32][payload:bytes][crc32:u32]
//!     8B           32B         1B          1B             4B         variable        4B
//! ```

use bytes::{Bytes, BytesMut};
use kimberlite_crypto::{ChainHash, chain_hash};
use kimberlite_types::{CompressionKind, Offset, RecordKind};

use crate::StorageError;

// Header size: offset(8) + prev_hash(32) + kind(1) + compression(1) + length(4) = 46 bytes.
const HEADER_SIZE: usize = 46;

// Total overhead per record: header(46) + crc(4) = 50 bytes.
const RECORD_OVERHEAD: usize = 50;

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
    fn write_into(&self, buf: &mut Vec<u8>) {
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

        // crc (4 bytes) - checksum of everything above
        let crc = kimberlite_crypto::crc32(buf);
        buf.extend_from_slice(&crc.to_le_bytes());
    }

    /// Deserializes a record from bytes.
    ///
    /// Returns the parsed record and the number of bytes consumed.
    /// Uses zero-copy slicing for the payload via [`Bytes::slice`].
    ///
    /// # Errors
    ///
    /// - [`StorageError::UnexpectedEof`] if the data is truncated
    /// - [`StorageError::CorruptedRecord`] if the CRC doesn't match
    /// - [`StorageError::InvalidRecordKind`] if the kind byte is invalid
    /// - [`StorageError::InvalidCompressionKind`] if the compression byte is invalid
    pub fn from_bytes(data: &Bytes) -> Result<(Self, usize), StorageError> {
        if data.len() < HEADER_SIZE {
            return Err(StorageError::UnexpectedEof);
        }

        // Read offset (bytes 0-7)
        let offset = Offset::new(u64::from_le_bytes(
            data[0..8]
                .try_into()
                .expect("slice is exactly 8 bytes after bounds check"),
        ));

        // Read prev_hash (bytes 8-39)
        let prev_hash_bytes: [u8; 32] = data[8..40]
            .try_into()
            .expect("slice is exactly 32 bytes after bounds check");
        let prev_hash = if prev_hash_bytes == [0u8; 32] {
            None
        } else {
            Some(ChainHash::from_bytes(&prev_hash_bytes))
        };

        // Read kind (byte 40)
        let kind = RecordKind::from_byte(data[40]).ok_or(StorageError::InvalidRecordKind {
            byte: data[40],
            offset,
        })?;

        // Read compression (byte 41)
        let compression =
            CompressionKind::from_byte(data[41]).ok_or(StorageError::InvalidCompressionKind {
                byte: data[41],
                offset,
            })?;

        // Read length (bytes 42-45)
        let length = u32::from_le_bytes(
            data[42..46]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        ) as usize;

        // Check we have enough for payload + crc(4)
        let total_size = HEADER_SIZE + length + 4;
        if data.len() < total_size {
            return Err(StorageError::UnexpectedEof);
        }

        // Read payload (bytes 46..46+length) - zero-copy!
        let payload = data.slice(HEADER_SIZE..HEADER_SIZE + length);

        // Read and verify CRC (last 4 bytes)
        let stored_crc = u32::from_le_bytes(
            data[HEADER_SIZE + length..total_size]
                .try_into()
                .expect("slice is exactly 4 bytes after bounds check"),
        );
        let computed_crc = kimberlite_crypto::crc32(&data[0..HEADER_SIZE + length]);

        if stored_crc != computed_crc {
            return Err(StorageError::CorruptedRecord);
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
