//! Two-stage append pipeline for overlapping CPU and I/O work.
//!
//! The pipeline separates the append path into:
//! - **Stage 1 (CPU)**: Serialize records, compute hash chain, compress payloads
//! - **Stage 2 (I/O)**: Write to disk, fsync
//!
//! By preparing the next batch while the previous one is being flushed, we can
//! overlap CPU and I/O work within a single thread using double-buffering.

use bytes::{Bytes, BytesMut};
use kimberlite_crypto::ChainHash;
use kimberlite_types::{CompressionKind, Offset, RecordKind};

use crate::Record;
use crate::StorageError;
use crate::codec::CodecRegistry;

/// A batch of records that has been serialized and is ready for I/O.
///
/// Created by the CPU stage of the pipeline, consumed by the I/O stage.
#[derive(Debug)]
pub struct PreparedBatch {
    /// Serialized record data ready for writing.
    pub data: BytesMut,
    /// Byte positions of each record in the batch (for offset index updates).
    pub index_entries: Vec<(Offset, u64)>,
    /// Hash of the last record in this batch (for chain continuity).
    pub final_hash: ChainHash,
    /// Total bytes in the serialized data.
    pub bytes_written: u64,
    /// Number of records in this batch.
    pub records_written: usize,
}

/// Two-stage append pipeline that overlaps CPU preparation with I/O.
///
/// The pipeline holds the state needed to prepare batches (CPU stage) and
/// tracks the previous batch's write completion (I/O stage). Within a single
/// thread, this achieves overlap by deferring fsync — prepare the next batch
/// while the previous write hits disk.
#[derive(Debug)]
pub struct AppendPipeline {
    /// Buffer for the next batch being prepared.
    prepare_buf: BytesMut,
    /// Default buffer capacity.
    default_capacity: usize,
}

impl AppendPipeline {
    /// Creates a new pipeline with the given default buffer capacity.
    pub fn new(default_capacity: usize) -> Self {
        Self {
            prepare_buf: BytesMut::with_capacity(default_capacity),
            default_capacity,
        }
    }

    /// Prepares a batch of events for writing (CPU stage).
    ///
    /// Serializes each event as a `Record`, computes the hash chain, and
    /// optionally compresses payloads. Returns a `PreparedBatch` ready for
    /// the I/O stage.
    ///
    /// # Arguments
    ///
    /// * `events` - The event payloads to serialize
    /// * `start_offset` - Logical offset for the first record
    /// * `prev_hash` - Hash of the previous record (None for genesis)
    /// * `base_byte_pos` - File byte position where this batch starts
    /// * `compression` - Compression algorithm to use
    /// * `codecs` - Codec registry for compression
    pub fn prepare_batch(
        &mut self,
        events: &[Bytes],
        start_offset: Offset,
        prev_hash: Option<ChainHash>,
        base_byte_pos: u64,
        compression: CompressionKind,
        codecs: &CodecRegistry,
    ) -> Result<PreparedBatch, StorageError> {
        assert!(!events.is_empty(), "cannot prepare empty batch");

        self.prepare_buf.clear();
        let mut index_entries = Vec::with_capacity(events.len());
        let mut current_offset = start_offset;
        let mut current_hash = prev_hash;
        let mut byte_pos = base_byte_pos;

        for event in events {
            // Record byte position for index
            index_entries.push((current_offset, byte_pos));

            // Compress if enabled
            let (stored_payload, record_compression) = if compression == CompressionKind::None {
                (event.clone(), CompressionKind::None)
            } else {
                let compressed = codecs.compress(compression, event)?;
                if compressed.len() < event.len() {
                    (Bytes::from(compressed), compression)
                } else {
                    (event.clone(), CompressionKind::None)
                }
            };

            // Hash is computed over the ORIGINAL payload
            let hash_record = Record::new(current_offset, current_hash, event.clone());
            current_hash = Some(hash_record.compute_hash());

            // Serialize the on-disk record with compressed payload
            let record = Record::with_compression(
                current_offset,
                hash_record.prev_hash(),
                RecordKind::Data,
                record_compression,
                stored_payload,
            );
            record.to_bytes_into(&mut self.prepare_buf);

            byte_pos = base_byte_pos + self.prepare_buf.len() as u64;
            current_offset += Offset::from(1u64);
        }

        let data = self.take_prepared();
        let bytes_written = data.len() as u64;

        Ok(PreparedBatch {
            data,
            index_entries,
            final_hash: current_hash.expect("batch was non-empty"),
            bytes_written,
            records_written: events.len(),
        })
    }

    /// Returns a mutable reference to the preparation buffer.
    pub fn prepare_buffer(&mut self) -> &mut BytesMut {
        self.prepare_buf.clear();
        &mut self.prepare_buf
    }

    /// Takes the prepared buffer and returns a fresh one.
    fn take_prepared(&mut self) -> BytesMut {
        std::mem::replace(
            &mut self.prepare_buf,
            BytesMut::with_capacity(self.default_capacity),
        )
    }

    /// Returns the default buffer capacity.
    pub fn default_capacity(&self) -> usize {
        self.default_capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_buffer_lifecycle() {
        let mut pipeline = AppendPipeline::new(4096);

        // Get prepare buffer and write data
        let buf = pipeline.prepare_buffer();
        buf.extend_from_slice(b"hello world");
        assert_eq!(buf.len(), 11);

        // Take prepared data
        let prepared = pipeline.take_prepared();
        assert_eq!(&prepared[..], b"hello world");

        // New buffer is empty
        let buf = pipeline.prepare_buffer();
        assert!(buf.is_empty());
    }

    #[test]
    fn prepared_batch_fields() {
        let batch = PreparedBatch {
            data: BytesMut::from(&b"test"[..]),
            index_entries: vec![(Offset::new(0), 0), (Offset::new(1), 50)],
            final_hash: ChainHash::from_bytes(&[0xab; 32]),
            bytes_written: 100,
            records_written: 2,
        };
        assert_eq!(batch.records_written, 2);
        assert_eq!(batch.bytes_written, 100);
        assert_eq!(batch.index_entries.len(), 2);
    }

    #[test]
    fn prepare_batch_uncompressed() {
        let mut pipeline = AppendPipeline::new(4096);
        let codecs = CodecRegistry::new();
        let events = vec![
            Bytes::from("event one"),
            Bytes::from("event two"),
            Bytes::from("event three"),
        ];

        let batch = pipeline
            .prepare_batch(
                &events,
                Offset::new(0),
                None,
                0,
                CompressionKind::None,
                &codecs,
            )
            .unwrap();

        assert_eq!(batch.records_written, 3);
        assert_eq!(batch.index_entries.len(), 3);
        assert!(batch.bytes_written > 0);
        // First record starts at byte 0
        assert_eq!(batch.index_entries[0].1, 0);
        // Offsets are sequential
        assert_eq!(batch.index_entries[0].0, Offset::new(0));
        assert_eq!(batch.index_entries[1].0, Offset::new(1));
        assert_eq!(batch.index_entries[2].0, Offset::new(2));
    }

    #[test]
    fn prepare_batch_with_lz4() {
        let mut pipeline = AppendPipeline::new(4096);
        let codecs = CodecRegistry::new();
        // Repetitive data compresses well with LZ4
        let events = vec![Bytes::from(vec![42u8; 1000])];

        let batch = pipeline
            .prepare_batch(
                &events,
                Offset::new(0),
                None,
                0,
                CompressionKind::Lz4,
                &codecs,
            )
            .unwrap();

        assert_eq!(batch.records_written, 1);
        // Compressed batch should be smaller than uncompressed
        // Record overhead is 50 bytes, so uncompressed would be 1050
        assert!(batch.bytes_written < 1050);
    }

    #[test]
    fn prepare_batch_chain_continuity() {
        let mut pipeline = AppendPipeline::new(4096);
        let codecs = CodecRegistry::new();

        // First batch
        let events1 = vec![Bytes::from("first")];
        let batch1 = pipeline
            .prepare_batch(
                &events1,
                Offset::new(0),
                None,
                0,
                CompressionKind::None,
                &codecs,
            )
            .unwrap();

        // Second batch chains from first
        let events2 = vec![Bytes::from("second")];
        let batch2 = pipeline
            .prepare_batch(
                &events2,
                Offset::new(1),
                Some(batch1.final_hash),
                batch1.bytes_written,
                CompressionKind::None,
                &codecs,
            )
            .unwrap();

        assert_eq!(batch2.records_written, 1);
        assert_eq!(batch2.index_entries[0].0, Offset::new(1));
        assert_eq!(batch2.index_entries[0].1, batch1.bytes_written);
        // Hashes should differ
        assert_ne!(batch1.final_hash, batch2.final_hash);
    }

    #[test]
    #[should_panic(expected = "cannot prepare empty batch")]
    fn prepare_batch_empty_panics() {
        let mut pipeline = AppendPipeline::new(4096);
        let codecs = CodecRegistry::new();
        let _ = pipeline.prepare_batch(
            &[],
            Offset::new(0),
            None,
            0,
            CompressionKind::None,
            &codecs,
        );
    }
}
