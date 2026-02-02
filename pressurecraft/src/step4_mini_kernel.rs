//! # Step 4: Mini Kernel
//!
//! **Learning objective:** Build a complete kernel with the `apply()` function.
//!
//! ## Key Concepts
//!
//! **The Kernel**: The heart of FCIS - a pure function that processes commands.
//!
//! ```text
//! apply(state, command) → Result<(new_state, effects), error>
//! ```
//!
//! This function:
//! - Takes current state and a command
//! - Validates the command
//! - Computes state transitions
//! - Produces effects to execute
//! - Returns everything or an error
//!
//! **Crucially:** The kernel is PURE. It doesn't:
//! - Execute effects (that's the runtime's job)
//! - Read from disk or network
//! - Use randomness or clocks
//! - Mutate any global state
//!
//! ## This is Kimberlite's Core Pattern
//!
//! The production kernel in `kimberlite-kernel/src/kernel.rs` uses this exact pattern:
//! - `apply_committed(state, command) -> Result<(State, Vec<Effect>), KernelError>`
//!
//! ## Builder Pattern for State
//!
//! Note how state transitions use the builder pattern:
//! - `state.with_stream(meta)` - Takes ownership, returns new state
//! - `state.with_updated_offset(id, offset)` - Same pattern
//!
//! This avoids unnecessary clones while maintaining immutability.

use bytes::Bytes;

use crate::step2_commands_effects::{DataClass, Effect, Offset, StreamId, StreamMetadata};

// Re-export for convenience
pub use crate::step2_commands_effects::Command;

// ============================================================================
// State (Simplified from Step 3)
// ============================================================================

use std::collections::BTreeMap;

/// The kernel's in-memory state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    streams: BTreeMap<StreamId, StreamMetadata>,
}

impl State {
    pub fn new() -> Self {
        Self {
            streams: BTreeMap::new(),
        }
    }

    pub fn get_stream(&self, id: &StreamId) -> Option<&StreamMetadata> {
        self.streams.get(id)
    }

    pub fn stream_exists(&self, id: &StreamId) -> bool {
        self.streams.contains_key(id)
    }

    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Builder pattern: add stream and return updated state.
    fn with_stream(mut self, meta: StreamMetadata) -> Self {
        self.streams.insert(meta.stream_id, meta);
        self
    }

    /// Builder pattern: update offset and return updated state.
    fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        if let Some(stream) = self.streams.get_mut(&id) {
            stream.current_offset = new_offset;
        }
        self
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Kernel Error
// ============================================================================

/// Errors that can occur in the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    StreamAlreadyExists(StreamId),
    StreamNotFound(StreamId),
    OffsetMismatch { expected: Offset, actual: Offset },
    EmptyBatch,
    InvalidStreamName(String),
}

// ============================================================================
// The Kernel: The `apply()` Function
// ============================================================================

/// Applies a command to the state, producing a new state and effects.
///
/// **This is the functional core.**
///
/// PURE FUNCTION:
/// - No IO (doesn't touch disk, network, etc.)
/// - No clocks (timestamp comes from caller if needed)
/// - No randomness (deterministic)
/// - Same input → same output, always
///
/// Returns:
/// - `Ok((new_state, effects))` on success
/// - `Err(error)` if command is invalid
pub fn apply(state: State, cmd: Command) -> Result<(State, Vec<Effect>), KernelError> {
    match cmd {
        Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
        } => apply_create_stream(state, stream_id, stream_name, data_class),

        Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        } => apply_append_batch(state, stream_id, events, expected_offset),

        Command::ReadStream {
            stream_id,
            from_offset,
            max_events,
        } => apply_read_stream(state, stream_id, from_offset, max_events),
    }
}

/// Applies CreateStream command.
fn apply_create_stream(
    state: State,
    stream_id: StreamId,
    stream_name: String,
    data_class: DataClass,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Validation: stream name
    if stream_name.is_empty() {
        return Err(KernelError::InvalidStreamName(
            "Stream name cannot be empty".to_string(),
        ));
    }

    // Validation: stream doesn't exist
    if state.stream_exists(&stream_id) {
        return Err(KernelError::StreamAlreadyExists(stream_id));
    }

    // Create metadata
    let meta = StreamMetadata {
        stream_id,
        stream_name: stream_name.clone(),
        data_class,
        current_offset: Offset::ZERO,
    };

    // State transition: add stream
    let new_state = state.with_stream(meta.clone());

    // Effects to execute
    let effects = vec![Effect::MetadataWrite(meta)];

    Ok((new_state, effects))
}

/// Applies AppendBatch command.
fn apply_append_batch(
    state: State,
    stream_id: StreamId,
    events: Vec<Bytes>,
    expected_offset: Offset,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Validation: batch not empty
    if events.is_empty() {
        return Err(KernelError::EmptyBatch);
    }

    // Validation: stream exists
    let stream = state
        .get_stream(&stream_id)
        .ok_or(KernelError::StreamNotFound(stream_id))?;

    // Validation: offset matches (optimistic concurrency control)
    if stream.current_offset != expected_offset {
        return Err(KernelError::OffsetMismatch {
            expected: expected_offset,
            actual: stream.current_offset,
        });
    }

    // Compute new offset
    let event_count = events.len() as u64;
    let new_offset = expected_offset.increment_by(event_count);

    // State transition: update offset
    let new_state = state.with_updated_offset(stream_id, new_offset);

    // Effects to execute
    let effects = vec![
        Effect::StorageAppend {
            stream_id,
            base_offset: expected_offset,
            events,
        },
        Effect::WakeProjection {
            stream_id,
            from_offset: expected_offset,
            to_offset: new_offset,
        },
    ];

    Ok((new_state, effects))
}

/// Applies ReadStream command (query).
fn apply_read_stream(
    state: State,
    stream_id: StreamId,
    from_offset: Offset,
    max_events: usize,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Validation: stream exists
    let _stream = state
        .get_stream(&stream_id)
        .ok_or(KernelError::StreamNotFound(stream_id))?;

    // State unchanged (reads don't mutate state)
    let new_state = state;

    // Effect: send response (simplified - real impl would read from storage)
    let effects = vec![Effect::SendResponse {
        events: vec![], // Would be populated by runtime
        next_offset: from_offset.increment_by(max_events as u64),
    }];

    Ok((new_state, effects))
}

// ============================================================================
// Runtime: The Imperative Shell
// ============================================================================

/// Executes an effect (impure, performs side effects).
///
/// This is the IMPERATIVE SHELL. It:
/// - Writes to storage
/// - Sends network messages
/// - Logs audit events
/// - Does all the impure stuff
pub fn execute_effect(effect: Effect) {
    match effect {
        Effect::StorageAppend {
            stream_id,
            base_offset,
            events,
        } => {
            // In production: write to durable storage
            println!(
                "STORAGE: Append {} events to stream {:?} at offset {:?}",
                events.len(),
                stream_id,
                base_offset
            );
        }

        Effect::MetadataWrite(meta) => {
            // In production: write to metadata store
            println!(
                "METADATA: Write stream {:?} ({})",
                meta.stream_id, meta.stream_name
            );
        }

        Effect::AuditLog {
            action,
            stream_id,
            timestamp_ms,
        } => {
            // In production: append to audit log
            println!(
                "AUDIT: [{}ms] {} on stream {:?}",
                timestamp_ms, action, stream_id
            );
        }

        Effect::WakeProjection {
            stream_id,
            from_offset,
            to_offset,
        } => {
            // In production: notify projection engine
            println!(
                "PROJECTION: Wake for stream {:?}, offsets {:?}-{:?}",
                stream_id, from_offset, to_offset
            );
        }

        Effect::SendResponse { events, next_offset } => {
            // In production: send to client over network
            println!(
                "RESPONSE: {} events, next offset {:?}",
                events.len(),
                next_offset
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_create_stream_success() {
        let state = State::new();
        let cmd = Command::create_stream(
            StreamId::new(1),
            "events".to_string(),
            DataClass::Internal,
        );

        let (new_state, effects) = apply(state, cmd).unwrap();

        // State updated
        assert!(new_state.stream_exists(&StreamId::new(1)));

        // Effects produced
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::MetadataWrite(_)));
    }

    #[test]
    fn apply_create_duplicate_stream_fails() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        // Create stream
        let (state, _) = apply(
            state,
            Command::create_stream(stream_id, "events".to_string(), DataClass::Internal),
        )
        .unwrap();

        // Try to create again
        let result = apply(
            state,
            Command::create_stream(stream_id, "duplicate".to_string(), DataClass::Public),
        );

        assert!(matches!(
            result,
            Err(KernelError::StreamAlreadyExists(_))
        ));
    }

    #[test]
    fn apply_append_batch_success() {
        let state = State::new();

        // Create stream
        let (state, _) = apply(
            state,
            Command::create_stream(
                StreamId::new(1),
                "events".to_string(),
                DataClass::Internal,
            ),
        )
        .unwrap();

        // Append batch
        let (new_state, effects) = apply(
            state,
            Command::append_batch(
                StreamId::new(1),
                vec![Bytes::from("event1"), Bytes::from("event2")],
                Offset::ZERO,
            ),
        )
        .unwrap();

        // State updated
        let stream = new_state.get_stream(&StreamId::new(1)).unwrap();
        assert_eq!(stream.current_offset, Offset::new(2));

        // Effects produced
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::StorageAppend { .. }));
        assert!(matches!(effects[1], Effect::WakeProjection { .. }));
    }

    #[test]
    fn apply_append_wrong_offset_fails() {
        let state = State::new();

        // Create stream
        let (state, _) = apply(
            state,
            Command::create_stream(
                StreamId::new(1),
                "events".to_string(),
                DataClass::Internal,
            ),
        )
        .unwrap();

        // Try to append at wrong offset
        let result = apply(
            state,
            Command::append_batch(
                StreamId::new(1),
                vec![Bytes::from("event")],
                Offset::new(100), // Wrong!
            ),
        );

        assert!(matches!(result, Err(KernelError::OffsetMismatch { .. })));
    }

    #[test]
    fn kernel_is_deterministic() {
        let stream_id = StreamId::new(1);

        // Apply same commands twice
        let state1 = State::new();
        let (state1, effects1) = apply(
            state1,
            Command::create_stream(stream_id, "events".to_string(), DataClass::Internal),
        )
        .unwrap();
        let (state1, effects1_2) = apply(
            state1,
            Command::append_batch(
                stream_id,
                vec![Bytes::from("e1"), Bytes::from("e2")],
                Offset::ZERO,
            ),
        )
        .unwrap();

        let state2 = State::new();
        let (state2, effects2) = apply(
            state2,
            Command::create_stream(stream_id, "events".to_string(), DataClass::Internal),
        )
        .unwrap();
        let (state2, effects2_2) = apply(
            state2,
            Command::append_batch(
                stream_id,
                vec![Bytes::from("e1"), Bytes::from("e2")],
                Offset::ZERO,
            ),
        )
        .unwrap();

        // All results identical
        assert_eq!(state1, state2);
        assert_eq!(effects1, effects2);
        assert_eq!(effects1_2, effects2_2);
    }

    #[test]
    fn multiple_appends_chain() {
        let state = State::new();
        let stream_id = StreamId::new(1);

        // Create stream
        let (state, _) = apply(
            state,
            Command::create_stream(stream_id, "events".to_string(), DataClass::Internal),
        )
        .unwrap();

        // Append batch 1
        let (state, _) = apply(
            state,
            Command::append_batch(stream_id, vec![Bytes::from("e1")], Offset::new(0)),
        )
        .unwrap();

        // Append batch 2
        let (state, _) = apply(
            state,
            Command::append_batch(stream_id, vec![Bytes::from("e2"), Bytes::from("e3")], Offset::new(1)),
        )
        .unwrap();

        // Append batch 3
        let (state, _) = apply(
            state,
            Command::append_batch(stream_id, vec![Bytes::from("e4")], Offset::new(3)),
        )
        .unwrap();

        let stream = state.get_stream(&stream_id).unwrap();
        assert_eq!(stream.current_offset, Offset::new(4));
    }
}
