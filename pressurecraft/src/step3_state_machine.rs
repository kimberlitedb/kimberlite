//! # Step 3: State Machines
//!
//! **Learning objective:** Build a state machine that transitions from one state to another.
//!
//! ## Key Concepts
//!
//! **State Machine**: A system that can be in one of several states, with defined transitions.
//! - **State**: Current configuration of the system
//! - **Event/Command**: Triggers a transition
//! - **Transition**: State + Event → New State
//!
//! ## State Machine Design
//!
//! ```text
//! Current State + Command → New State
//!
//! Example:
//! State { offset: 0 } + AppendBatch { count: 3 } → State { offset: 3 }
//! ```
//!
//! ## Key Principles
//!
//! 1. **Immutability**: Return a new state, don't mutate the old one
//! 2. **Validation**: Check preconditions before transitioning
//! 3. **Invariants**: Assert postconditions after transitioning
//! 4. **Builder Pattern**: Methods take ownership, modify, return self
//!
//! ## Make Illegal States Unrepresentable
//!
//! Use types to enforce invariants at compile time:
//! - Newtype wrappers (`StreamId(u64)` not `u64`)
//! - Enums for fixed choices
//! - Private fields with public getters

use std::collections::BTreeMap;

use crate::step2_commands_effects::{DataClass, Offset, StreamId, StreamMetadata};

// ============================================================================
// State
// ============================================================================

/// The kernel's in-memory state.
///
/// Tracks all streams and their current offsets.
///
/// Uses a **builder pattern**: methods take ownership of `self`, modify it,
/// and return it. This allows functional-style chaining while avoiding clones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Map of stream_id → metadata
    streams: BTreeMap<StreamId, StreamMetadata>,
    /// Next stream ID to allocate
    next_stream_id: StreamId,
}

impl State {
    /// Creates a new empty state.
    pub fn new() -> Self {
        Self {
            streams: BTreeMap::new(),
            next_stream_id: StreamId::new(1),
        }
    }

    /// Returns metadata for a stream, if it exists.
    pub fn get_stream(&self, id: &StreamId) -> Option<&StreamMetadata> {
        self.streams.get(id)
    }

    /// Returns true if a stream exists.
    pub fn stream_exists(&self, id: &StreamId) -> bool {
        self.streams.contains_key(id)
    }

    /// Returns the next stream ID that would be allocated.
    pub fn next_stream_id(&self) -> StreamId {
        self.next_stream_id
    }

    /// Returns the number of streams.
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    // ------------------------------------------------------------------------
    // Builder Pattern: State Transitions
    // ------------------------------------------------------------------------

    /// Adds a stream and returns the updated state.
    ///
    /// **Precondition:** Stream must not already exist.
    ///
    /// This is `pub(crate)` - only the kernel should call it directly.
    /// External code should use a higher-level API that validates.
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        // Precondition: stream doesn't exist yet
        assert!(
            !self.streams.contains_key(&meta.stream_id),
            "Stream {:?} already exists",
            meta.stream_id
        );

        let stream_id = meta.stream_id;
        self.streams.insert(stream_id, meta);

        // Postcondition: stream now exists
        assert!(self.streams.contains_key(&stream_id));

        self
    }

    /// Updates a stream's offset and returns the updated state.
    ///
    /// **Precondition:** Stream must exist.
    /// **Precondition:** New offset must be greater than current offset.
    pub(crate) fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        // Precondition: stream exists
        let stream = self
            .streams
            .get(&id)
            .expect("Stream must exist to update offset");

        // Precondition: offset increases monotonically
        assert!(
            new_offset >= stream.current_offset,
            "Offset must increase: current={:?}, new={:?}",
            stream.current_offset,
            new_offset
        );

        // Update offset
        let old_offset = stream.current_offset;
        self.streams.get_mut(&id).unwrap().current_offset = new_offset;

        // Postcondition: offset was updated
        assert!(self.streams.get(&id).unwrap().current_offset > old_offset);

        self
    }

    /// Allocates the next stream ID and increments counter.
    ///
    /// Returns (updated_state, allocated_id).
    pub(crate) fn allocate_stream_id(mut self) -> (Self, StreamId) {
        let id = self.next_stream_id;
        self.next_stream_id = StreamId::new(id.0 + 1);

        // Postcondition: next ID was incremented
        assert_eq!(self.next_stream_id.0, id.0 + 1);

        (self, id)
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// State Transitions (Pure Functions)
// ============================================================================

/// Error type for state transition failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    StreamAlreadyExists(StreamId),
    StreamNotFound(StreamId),
    OffsetMismatch { expected: Offset, actual: Offset },
    EmptyBatch,
}

/// Creates a stream in the state.
///
/// PURE: Returns new state or error, no side effects.
pub fn create_stream(
    state: State,
    stream_id: StreamId,
    stream_name: String,
    data_class: DataClass,
) -> Result<State, TransitionError> {
    // Validate: stream doesn't exist
    if state.stream_exists(&stream_id) {
        return Err(TransitionError::StreamAlreadyExists(stream_id));
    }

    // Create metadata
    let meta = StreamMetadata {
        stream_id,
        stream_name,
        data_class,
        current_offset: Offset::ZERO,
    };

    // Transition: add stream to state
    let new_state = state.with_stream(meta);

    Ok(new_state)
}

/// Appends events to a stream in the state.
///
/// PURE: Returns new state or error, no side effects.
///
/// Note: This updates the OFFSET but doesn't store events.
/// The actual storage happens via effects (see Step 4).
pub fn append_batch(
    state: State,
    stream_id: StreamId,
    event_count: usize,
    expected_offset: Offset,
) -> Result<State, TransitionError> {
    // Validate: stream exists
    let stream = state
        .get_stream(&stream_id)
        .ok_or(TransitionError::StreamNotFound(stream_id))?;

    // Validate: expected offset matches current offset
    if stream.current_offset != expected_offset {
        return Err(TransitionError::OffsetMismatch {
            expected: expected_offset,
            actual: stream.current_offset,
        });
    }

    // Validate: batch not empty
    if event_count == 0 {
        return Err(TransitionError::EmptyBatch);
    }

    // Transition: update offset
    let new_offset = expected_offset.increment_by(event_count as u64);
    let new_state = state.with_updated_offset(stream_id, new_offset);

    Ok(new_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(state.stream_count(), 0);
        assert_eq!(state.next_stream_id(), StreamId::new(1));
    }

    #[test]
    fn create_stream_adds_to_state() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        let state = create_stream(
            state,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();

        assert_eq!(state.stream_count(), 1);
        assert!(state.stream_exists(&stream_id));

        let meta = state.get_stream(&stream_id).unwrap();
        assert_eq!(meta.stream_name, "events");
        assert_eq!(meta.current_offset, Offset::ZERO);
    }

    #[test]
    fn create_duplicate_stream_fails() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        let state = create_stream(
            state,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();

        // Try to create same stream again
        let result = create_stream(state, stream_id, "duplicate".to_string(), DataClass::Public);

        assert!(matches!(
            result,
            Err(TransitionError::StreamAlreadyExists(_))
        ));
    }

    #[test]
    fn append_batch_updates_offset() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        let state = create_stream(
            state,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();

        let state = append_batch(state, stream_id, 5, Offset::ZERO).unwrap();

        let meta = state.get_stream(&stream_id).unwrap();
        assert_eq!(meta.current_offset, Offset::new(5));
    }

    #[test]
    fn append_with_wrong_offset_fails() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        let state = create_stream(
            state,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();

        // Try to append at wrong offset
        let result = append_batch(state, stream_id, 5, Offset::new(100));

        assert!(matches!(result, Err(TransitionError::OffsetMismatch { .. })));
    }

    #[test]
    fn append_to_nonexistent_stream_fails() {
        let state = State::new();
        let stream_id = StreamId::new(999);

        let result = append_batch(state, stream_id, 5, Offset::ZERO);

        assert!(matches!(result, Err(TransitionError::StreamNotFound(_))));
    }

    #[test]
    fn state_transitions_are_deterministic() {
        let stream_id = StreamId::new(1);

        // Perform same transitions twice
        let state1 = State::new();
        let state1 = create_stream(
            state1,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();
        let state1 = append_batch(state1, stream_id, 3, Offset::ZERO).unwrap();

        let state2 = State::new();
        let state2 = create_stream(
            state2,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();
        let state2 = append_batch(state2, stream_id, 3, Offset::ZERO).unwrap();

        // States must be identical
        assert_eq!(state1, state2);
    }

    #[test]
    fn multiple_appends_chain() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        let state = create_stream(
            state,
            stream_id,
            "events".to_string(),
            DataClass::Internal,
        )
        .unwrap();

        // Chain multiple appends
        let state = append_batch(state, stream_id, 2, Offset::new(0)).unwrap();
        let state = append_batch(state, stream_id, 3, Offset::new(2)).unwrap();
        let state = append_batch(state, stream_id, 1, Offset::new(5)).unwrap();

        let meta = state.get_stream(&stream_id).unwrap();
        assert_eq!(meta.current_offset, Offset::new(6));
    }

    #[test]
    fn allocate_stream_id_increments() {
        let state = State::new();
        assert_eq!(state.next_stream_id(), StreamId::new(1));

        let (state, id1) = state.allocate_stream_id();
        assert_eq!(id1, StreamId::new(1));
        assert_eq!(state.next_stream_id(), StreamId::new(2));

        let (state, id2) = state.allocate_stream_id();
        assert_eq!(id2, StreamId::new(2));
        assert_eq!(state.next_stream_id(), StreamId::new(3));
    }
}
