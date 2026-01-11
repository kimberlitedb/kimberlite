//! vdb-storage: Append-only segment storage for VerityDB
//!
//! This crate implements the durable event log storage:
//! - Segment files with format: [offset:u64][len:u32][payload][crc:u32]
//! - append_batch(): Write events with optional fsync
//! - read_from(): Read events starting from an offset
//! - Segment rotation and compaction

use std::path::{Path, PathBuf};

use bytes::Bytes;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use vdb_types::{BatchPayload, Offset};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
}

#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("error writing batch payload")]
    WriteError,
    #[error("filesystem error")]
    FSError(#[from] io::Error),
}
