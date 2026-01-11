//! vdb-storage: Append-only segment storage for VerityDB
//!
//! This crate implements the durable event log storage:
//! - Segment files with format: [offset:u64][len:u32][payload][crc:u32]
//! - append_batch(): Write events with optional fsync
//! - read_from(): Read events starting from an offset
//! - Segment rotation and compaction

use std::path::PathBuf;

use bytes::Bytes;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use vdb_types::{BatchPayload, Offset, StreamId};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Record {
    offset: Offset,
    payload: Bytes,
}

impl Record {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // offset (8 bytes)
        buf.extend_from_slice(&self.offset.as_u64().to_le_bytes());

        // length (4 bytes)
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());

        // payload (variable)
        buf.extend_from_slice(&self.payload);

        // crc (4 bytes) - checksum of everything above
        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());

        buf
    }

    pub fn from_bytes(data: &Bytes) -> Result<(Self, usize), StorageError> {
        // Need at least header: offset(8) + len(4) = 12 bytes
        if data.len() < 12 {
            return Err(StorageError::UnexpectedEof);
        }

        // Read offset (bytes 0-7)
        let offset = Offset::new(u64::from_le_bytes(data[0..8].try_into().unwrap()));

        // Read length (bytes 8-11)
        let length = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;

        // Check we have enough for payload + crc(4)
        let total_size = 12 + length + 4;
        if data.len() < total_size {
            return Err(StorageError::UnexpectedEof);
        }

        // Read payload (bytes 12..12+length)
        let payload = data.slice(12..12 + length);

        // Read and verify CRC (last 4 bytes)
        let stored_crc = u32::from_le_bytes(data[12 + length..total_size].try_into().unwrap());
        let computed_crc = crc32fast::hash(&data[0..12 + length]);

        if stored_crc != computed_crc {
            return Err(StorageError::CorruptedRecord);
        }

        Ok((Record { offset, payload }, total_size))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Storage {
    data_dir: PathBuf,
}

impl Storage {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub async fn append_batch(
        &self,
        BatchPayload {
            stream_id,
            events,
            expected_offset,
        }: BatchPayload,
        fsync: bool,
    ) -> Result<Offset, StorageError> {
        let stream_dir = self.data_dir.join(stream_id.to_string());
        fs::create_dir_all(&stream_dir).await?;

        let segment_path = stream_dir.join("segment_000000.log");

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)
            .await?;

        let mut current_offset = expected_offset;

        for event in events {
            let record = Record {
                offset: current_offset,
                payload: event,
            };
            file.write_all(&record.to_bytes()).await?;
            current_offset += Offset::from(1);
        }

        if fsync {
            file.sync_all().await?;
        }

        Ok(current_offset)
    }

    pub async fn read_from(
        &self,
        stream_id: StreamId,
        from_offset: Offset,
        max_bytes: u64,
    ) -> Result<Vec<Bytes>, StorageError> {
        let segment_path = self
            .data_dir
            .join(stream_id.to_string())
            .join("segment_000000.log");

        let data: Bytes = fs::read(&segment_path).await?.into();

        let mut results = Vec::new();
        let mut bytes_read: u64 = 0;
        let mut pos = 0;

        while pos < data.len() && bytes_read < max_bytes {
            let (record, consumed) = Record::from_bytes(&data.slice(pos..))?;
            pos += consumed;

            // Skip records before our target offset
            if record.offset < from_offset {
                continue;
            }

            bytes_read += record.payload.len() as u64;
            results.push(record.payload);
        }

        Ok(results)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("error writing batch payload")]
    WriteError,
    #[error("filesystem error")]
    FSError(#[from] io::Error),
    #[error("unexpected end of file")]
    UnexpectedEof,
    #[error("corrupted record: CRC mismatch")]
    CorruptedRecord,
}
