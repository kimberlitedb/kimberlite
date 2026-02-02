//! # Step 5: Full Kernel
//!
//! **Learning objective:** Understand a production-ready kernel with complete validation.
//!
//! ## What's New in Step 5
//!
//! This step adds production-quality features:
//! - **Rich error handling** with detailed error messages
//! - **Assertion density** (preconditions and postconditions)
//! - **Builder pattern** for state (like production kernel)
//! - **Auto-ID allocation** for streams
//! - **Comprehensive validation**
//!
//! ## Compare to Production
//!
//! This kernel mirrors `crates/kimberlite-kernel/src/kernel.rs`:
//!
//! | This Module | Production Kimberlite |
//! |-------------|----------------------|
//! | `apply_committed()` | `apply_committed()` |
//! | `State` | `crates/kimberlite-kernel/src/state.rs:State` |
//! | `KernelError` | `KernelError` |
//! | Builder pattern | Same pattern (`state.with_stream()`) |
//!
//! ## Key Differences from Step 4
//!
//! 1. **Assertions**: Every state transition has preconditions/postconditions
//! 2. **Error Context**: Errors include helpful diagnostic information
//! 3. **Auto-ID**: Streams can be created with auto-allocated IDs
//! 4. **Invariants**: State invariants are checked and documented
//!
//! ## Assertion Density
//!
//! Notice how every function has multiple assertions:
//! - `assert!` at the start (preconditions)
//! - `assert!` at the end (postconditions)
//!
//! This is a key pattern in Kimberlite for catching bugs early.

use bytes::Bytes;
use std::collections::BTreeMap;

use crate::step2_commands_effects::{DataClass, Offset, StreamId, StreamMetadata};

// ============================================================================
// State (Production-Quality)
// ============================================================================

/// The kernel's in-memory state.
///
/// **Invariants:**
/// - All streams in `streams` map have unique IDs
/// - `next_stream_id` is always greater than any existing stream ID
/// - All offsets are monotonically increasing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    streams: BTreeMap<StreamId, StreamMetadata>,
    next_stream_id: StreamId,
}

impl State {
    /// Creates a new empty state.
    ///
    /// **Postcondition:** State is empty and next ID is 1.
    pub fn new() -> Self {
        let state = Self {
            streams: BTreeMap::new(),
            next_stream_id: StreamId::new(1),
        };

        // Postcondition: empty state
        assert_eq!(state.streams.len(), 0);
        assert_eq!(state.next_stream_id, StreamId::new(1));

        state
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

    /// Adds a stream and returns updated state.
    ///
    /// **Precondition:** Stream doesn't already exist.
    /// **Postcondition:** Stream exists in new state.
    pub(crate) fn with_stream(mut self, meta: StreamMetadata) -> Self {
        // Precondition: stream doesn't exist
        assert!(
            !self.streams.contains_key(&meta.stream_id),
            "Stream {:?} already exists (this is a kernel bug)",
            meta.stream_id
        );

        let stream_id = meta.stream_id;
        self.streams.insert(stream_id, meta);

        // Postcondition: stream now exists
        assert!(self.streams.contains_key(&stream_id));

        self
    }

    /// Updates stream offset and returns updated state.
    ///
    /// **Precondition:** Stream exists.
    /// **Precondition:** New offset >= current offset (monotonic).
    /// **Postcondition:** Offset was updated.
    pub(crate) fn with_updated_offset(mut self, id: StreamId, new_offset: Offset) -> Self {
        // Precondition: stream exists
        let stream = self
            .streams
            .get(&id)
            .expect("Stream must exist to update offset (kernel bug)");

        let old_offset = stream.current_offset;

        // Precondition: offsets increase monotonically
        assert!(
            new_offset >= old_offset,
            "Offset must increase: {:?} -> {:?}",
            old_offset,
            new_offset
        );

        // Update offset
        self.streams.get_mut(&id).unwrap().current_offset = new_offset;

        // Postcondition: offset was updated
        assert!(self.streams.get(&id).unwrap().current_offset >= old_offset);

        self
    }

    /// Allocates next stream ID and increments counter.
    ///
    /// **Postcondition:** Next ID was incremented.
    pub(crate) fn allocate_stream_id(mut self) -> (Self, StreamId) {
        let allocated_id = self.next_stream_id;
        let old_next = self.next_stream_id;

        self.next_stream_id = StreamId::new(self.next_stream_id.0 + 1);

        // Postcondition: ID was incremented
        assert_eq!(self.next_stream_id.0, old_next.0 + 1);

        (self, allocated_id)
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Kernel Error (Production-Quality)
// ============================================================================

/// Errors that can occur in the kernel.
///
/// Notice: Every variant includes context to help debugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    /// Stream already exists (can't create duplicate).
    StreamAlreadyExists {
        stream_id: StreamId,
        existing_name: String,
    },

    /// Stream doesn't exist (required for operation).
    StreamNotFound { stream_id: StreamId },

    /// Offset mismatch (optimistic concurrency control failed).
    OffsetMismatch {
        stream_id: StreamId,
        expected: Offset,
        actual: Offset,
    },

    /// Empty batch (must have at least one event).
    EmptyBatch { stream_id: StreamId },

    /// Invalid stream name.
    InvalidStreamName { reason: String },
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamAlreadyExists {
                stream_id,
                existing_name,
            } => write!(
                f,
                "Stream {:?} already exists with name '{}'",
                stream_id, existing_name
            ),
            Self::StreamNotFound { stream_id } => {
                write!(f, "Stream {:?} not found", stream_id)
            }
            Self::OffsetMismatch {
                stream_id,
                expected,
                actual,
            } => write!(
                f,
                "Offset mismatch for stream {:?}: expected {:?}, actual {:?}",
                stream_id, expected, actual
            ),
            Self::EmptyBatch { stream_id } => {
                write!(f, "Empty batch for stream {:?}", stream_id)
            }
            Self::InvalidStreamName { reason } => write!(f, "Invalid stream name: {}", reason),
        }
    }
}

impl std::error::Error for KernelError {}

// ============================================================================
// Commands (Extended)
// ============================================================================

/// Commands that can be sent to the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Create stream with explicit ID.
    CreateStream {
        stream_id: StreamId,
        stream_name: String,
        data_class: DataClass,
    },

    /// Create stream with auto-allocated ID.
    CreateStreamAuto {
        stream_name: String,
        data_class: DataClass,
    },

    /// Append batch of events.
    AppendBatch {
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
    },
}

// ============================================================================
// Effects (Extended)
// ============================================================================

/// Effects produced by the kernel.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Write stream metadata.
    MetadataWrite(StreamMetadata),

    /// Append events to storage.
    StorageAppend {
        stream_id: StreamId,
        base_offset: Offset,
        events: Vec<Bytes>,
    },

    /// Wake projections to process events.
    WakeProjection {
        stream_id: StreamId,
        from_offset: Offset,
        to_offset: Offset,
    },

    /// Log audit event.
    AuditLog { action: String, stream_id: StreamId },
}

// ============================================================================
// The Kernel: apply_committed()
// ============================================================================

/// Applies a committed command to the state.
///
/// **This is the functional core - the heart of the database.**
///
/// PURE FUNCTION:
/// - No IO, no clocks, no randomness
/// - Deterministic: same input → same output
/// - All side effects returned as `Effect` values
///
/// Returns:
/// - `Ok((new_state, effects))` if command is valid
/// - `Err(error)` if command cannot be applied
///
/// **Invariants:**
/// - If `Ok`, state transition is valid and effects are executable
/// - If `Err`, state is unchanged (transaction semantics)
pub fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>), KernelError> {
    match cmd {
        Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
        } => apply_create_stream(state, Some(stream_id), stream_name, data_class),

        Command::CreateStreamAuto {
            stream_name,
            data_class,
        } => apply_create_stream(state, None, stream_name, data_class),

        Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        } => apply_append_batch(state, stream_id, events, expected_offset),
    }
}

/// Applies CreateStream command.
fn apply_create_stream(
    state: State,
    stream_id_opt: Option<StreamId>,
    stream_name: String,
    data_class: DataClass,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Validate stream name
    if stream_name.is_empty() {
        return Err(KernelError::InvalidStreamName {
            reason: "Stream name cannot be empty".to_string(),
        });
    }
    if stream_name.len() > 255 {
        return Err(KernelError::InvalidStreamName {
            reason: format!("Stream name too long: {} chars (max 255)", stream_name.len()),
        });
    }

    // Allocate or use provided ID
    let (state, stream_id) = match stream_id_opt {
        Some(id) => {
            // Validate: stream doesn't exist
            if let Some(existing) = state.get_stream(&id) {
                return Err(KernelError::StreamAlreadyExists {
                    stream_id: id,
                    existing_name: existing.stream_name.clone(),
                });
            }
            (state, id)
        }
        None => {
            // Auto-allocate ID
            state.allocate_stream_id()
        }
    };

    // Create metadata
    let meta = StreamMetadata {
        stream_id,
        stream_name: stream_name.clone(),
        data_class,
        current_offset: Offset::ZERO,
    };

    // State transition
    let new_state = state.with_stream(meta.clone());

    // Postcondition: stream exists
    assert!(new_state.stream_exists(&stream_id));

    // Effects
    let effects = vec![
        Effect::MetadataWrite(meta),
        Effect::AuditLog {
            action: format!("CreateStream: {}", stream_name),
            stream_id,
        },
    ];

    Ok((new_state, effects))
}

/// Applies AppendBatch command.
fn apply_append_batch(
    state: State,
    stream_id: StreamId,
    events: Vec<Bytes>,
    expected_offset: Offset,
) -> Result<(State, Vec<Effect>), KernelError> {
    // Validate: batch not empty
    if events.is_empty() {
        return Err(KernelError::EmptyBatch { stream_id });
    }

    // Validate: stream exists
    let stream = state
        .get_stream(&stream_id)
        .ok_or(KernelError::StreamNotFound { stream_id })?;

    // Validate: offset matches (optimistic concurrency)
    if stream.current_offset != expected_offset {
        return Err(KernelError::OffsetMismatch {
            stream_id,
            expected: expected_offset,
            actual: stream.current_offset,
        });
    }

    // Compute new offset
    let event_count = events.len() as u64;
    let new_offset = expected_offset.increment_by(event_count);

    // Precondition: new offset > old offset
    assert!(new_offset > expected_offset);

    // State transition
    let new_state = state.with_updated_offset(stream_id, new_offset);

    // Postcondition: offset was updated
    assert_eq!(
        new_state.get_stream(&stream_id).unwrap().current_offset,
        new_offset
    );

    // Effects
    let effects = vec![
        Effect::StorageAppend {
            stream_id,
            base_offset: expected_offset,
            events: events.clone(),
        },
        Effect::WakeProjection {
            stream_id,
            from_offset: expected_offset,
            to_offset: new_offset,
        },
        Effect::AuditLog {
            action: format!("AppendBatch: {} events", event_count),
            stream_id,
        },
    ];

    Ok((new_state, effects))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_stream_with_explicit_id() {
        let state = State::new();
        let cmd = Command::CreateStream {
            stream_id: StreamId::new(42),
            stream_name: "events".to_string(),
            data_class: DataClass::Internal,
        };

        let (new_state, effects) = apply_committed(state, cmd).unwrap();

        assert!(new_state.stream_exists(&StreamId::new(42)));
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn create_stream_with_auto_id() {
        let state = State::new();
        let cmd = Command::CreateStreamAuto {
            stream_name: "events".to_string(),
            data_class: DataClass::Internal,
        };

        let (new_state, effects) = apply_committed(state, cmd).unwrap();

        // Should have allocated ID 1
        assert!(new_state.stream_exists(&StreamId::new(1)));
        assert_eq!(new_state.next_stream_id, StreamId::new(2));
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn create_duplicate_stream_fails() {
        let state = State::new();

        // Create first stream
        let (state, _) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: "events".to_string(),
                data_class: DataClass::Internal,
            },
        )
        .unwrap();

        // Try to create duplicate
        let result = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: "duplicate".to_string(),
                data_class: DataClass::Public,
            },
        );

        assert!(matches!(
            result,
            Err(KernelError::StreamAlreadyExists { .. })
        ));
    }

    #[test]
    fn append_batch_updates_offset() {
        let state = State::new();

        // Create stream
        let (state, _) = apply_committed(
            state,
            Command::CreateStream {
                stream_id: StreamId::new(1),
                stream_name: "events".to_string(),
                data_class: DataClass::Internal,
            },
        )
        .unwrap();

        // Append batch
        let (new_state, effects) = apply_committed(
            state,
            Command::AppendBatch {
                stream_id: StreamId::new(1),
                events: vec![Bytes::from("e1"), Bytes::from("e2"), Bytes::from("e3")],
                expected_offset: Offset::ZERO,
            },
        )
        .unwrap();

        // Check state
        let stream = new_state.get_stream(&StreamId::new(1)).unwrap();
        assert_eq!(stream.current_offset, Offset::new(3));

        // Check effects
        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::StorageAppend { .. }));
        assert!(matches!(effects[1], Effect::WakeProjection { .. }));
        assert!(matches!(effects[2], Effect::AuditLog { .. }));
    }

    #[test]
    fn determinism_test() {
        // Run same sequence twice
        let run = || {
            let state = State::new();

            let (state, _) = apply_committed(
                state,
                Command::CreateStream {
                    stream_id: StreamId::new(1),
                    stream_name: "events".to_string(),
                    data_class: DataClass::Internal,
                },
            )
            .unwrap();

            apply_committed(
                state,
                Command::AppendBatch {
                    stream_id: StreamId::new(1),
                    events: vec![Bytes::from("e1"), Bytes::from("e2")],
                    expected_offset: Offset::ZERO,
                },
            )
            .unwrap()
        };

        let (state1, effects1) = run();
        let (state2, effects2) = run();

        // Must be identical
        assert_eq!(state1, state2);
        assert_eq!(effects1, effects2);
    }

    #[test]
    fn error_messages_are_helpful() {
        let err = KernelError::StreamAlreadyExists {
            stream_id: StreamId::new(1),
            existing_name: "events".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("Stream"));
        assert!(msg.contains("events"));
    }
}
