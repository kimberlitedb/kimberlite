//! Kernel state management.
//!
//! The kernel maintains in-memory state that tracks all streams and their
//! current offsets. State transitions are done by taking ownership and
//! returning a new state (builder pattern).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vdb_types::{Offset, StreamId, StreamMetadata};

/// The kernel's in-memory state.
///
/// State uses a builder pattern - methods take ownership of `self`, mutate,
/// and return `self`. This supports the functional core pattern while
/// avoiding unnecessary clones of the internal BTreeMap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct State {
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

    /// Adds a stream and returns the updated state.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        self.streams.insert(meta.stream_id, meta);
        self
    }

    /// Updates a stream's offset and returns the updated state.
    ///
    /// If the stream doesn't exist, returns self unchanged.
    ///
    /// Internal to the kernel - external code should use `apply_committed`
    /// which handles validation and effects.
    pub(crate) fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.current_offset = new_offset;
        }
        self
    }

    /// Returns the number of streams in the state.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}
