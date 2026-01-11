//! Kernel state management.
//!
//! The kernel maintains in-memory state that tracks all streams and their
//! current offsets. State is immutable - operations return new state instances.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vdb_types::{Offset, StreamId, StreamMetadata};

/// The kernel's in-memory state.
///
/// State is treated as immutable - methods like [`State::with_stream`] return
/// new state instances rather than mutating in place. This supports the
/// functional core pattern and makes the kernel easier to test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
    /// All streams indexed by their ID.
    /// Uses BTreeMap for deterministic iteration order.
    streams: BTreeMap<StreamId, StreamMetadata>,
}

impl State {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the metadata for a stream, if it exists.
    pub fn get_stream(&self, id: &StreamId) -> Option<&StreamMetadata> {
        self.streams.get(id)
    }

    /// Returns true if a stream with the given ID exists.
    pub fn stream_exists(&self, id: &StreamId) -> bool {
        self.streams.contains_key(id)
    }

    /// Returns a new state with the given stream added.
    ///
    /// If a stream with the same ID already exists, it is replaced.
    pub fn with_stream(mut self, meta: StreamMetadata) -> Self {
        self.streams.insert(meta.stream_id, meta);
        self
    }

    /// Returns a new state with the stream's offset updated.
    ///
    /// If the stream doesn't exist, the state is returned unchanged.
    pub fn with_updated_offset(mut self, id: &StreamId, new_offset: Offset) -> Self {
        if let Some(stream) = self.streams.get_mut(id) {
            stream.current_offset = new_offset;
        }
        self
    }

    /// Returns the number of streams in the state.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}
