//! The kernel - pure functional core of VerityDB.
//!
//! The kernel applies committed commands to produce new state and effects.
//! It is completely pure: no IO, no clocks, no randomness. This makes it
//! deterministic and easy to test.
//!
//! # Example
//!
//! ```ignore
//! let state = State::new();
//! let cmd = Command::create_stream(...);
//!
//! let (new_state, effects) = apply_committed(state, cmd)?;
//! // Runtime executes effects...
//! ```

use vdb_types::{AuditAction, BatchPayload, Offset, StreamId, StreamMetadata};

use crate::command::Command;
use crate::effects::Effect;
use crate::state::State;

/// Applies a committed command to the state, producing new state and effects.
///
/// This is the heart of the kernel. Commands have already been validated and
/// committed through VSR consensus before reaching this function.
///
/// # Arguments
///
/// * `state` - The current kernel state
/// * `cmd` - The command to apply
///
/// # Returns
///
/// A tuple of (new_state, effects) on success, or a [`KernelError`] on failure.
///
/// # Errors
///
/// * [`KernelError::StreamIdUniqueConstraint`] - Stream already exists (CreateStream)
/// * [`KernelError::StreamNotFound`] - Stream doesn't exist (AppendBatch)
/// * [`KernelError::UnexpectedStreamOffset`] - Optimistic concurrency failure (AppendBatch)
pub fn apply_committed(state: State, cmd: Command) -> Result<(State, Vec<Effect>), KernelError> {
    let mut effects: Vec<Effect> = Vec::new();

    match cmd {
        Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
            placement,
        } => {
            // Validate: stream must not already exist
            if state.stream_exists(&stream_id) {
                return Err(KernelError::StreamIdUniqueConstraint(stream_id));
            }

            // Create metadata with initial offset of 0
            let meta = StreamMetadata::new(
                stream_id,
                stream_name.clone(),
                data_class,
                placement.clone(),
            );

            // Produce effects
            effects.push(Effect::StreamMetadataWrite(meta.clone()));
            effects.push(Effect::AuditLogAppend(AuditAction::StreamCreated {
                stream_id,
                stream_name,
                data_class,
                placement,
            }));

            Ok((state.with_stream(meta), effects))
        }

        Command::AppendBatch(BatchPayload {
            stream_id,
            events,
            expected_offset,
        }) => {
            // Validate: stream must exist
            let stream = state
                .get_stream(&stream_id)
                .ok_or(KernelError::StreamNotFound(stream_id))?;

            // Validate: optimistic concurrency check
            if stream.current_offset != expected_offset {
                return Err(KernelError::UnexpectedStreamOffset {
                    stream_id,
                    expected: expected_offset,
                    actual: stream.current_offset,
                });
            }

            // Calculate new offset
            let event_count = events.len() as u64;
            let new_offset = stream.current_offset + Offset::from(event_count);

            // Produce effects
            effects.push(Effect::StorageAppend {
                stream_id,
                base_offset: stream.current_offset,
                events: events.clone(),
            });

            effects.push(Effect::WakeProjection {
                stream_id,
                from_offset: stream.current_offset,
                to_offset: new_offset,
            });

            effects.push(Effect::AuditLogAppend(AuditAction::EventsAppended {
                stream_id,
                count: events.len() as u32,
                from_offset: stream.current_offset,
            }));

            Ok((state.with_updated_offset(&stream_id, new_offset), effects))
        }
    }
}

/// Errors that can occur when applying commands to the kernel.
#[derive(thiserror::Error, Debug)]
pub enum KernelError {
    /// Attempted to create a stream with an ID that already exists.
    #[error("stream with id {0} already exists")]
    StreamIdUniqueConstraint(StreamId),

    /// Attempted to operate on a stream that doesn't exist.
    #[error("stream with id {0} not found")]
    StreamNotFound(StreamId),

    /// The expected offset didn't match the stream's current offset.
    /// This indicates a concurrent modification (optimistic concurrency failure).
    #[error("offset mismatch for stream {stream_id}: expected {expected}, actual {actual}")]
    UnexpectedStreamOffset {
        stream_id: StreamId,
        expected: Offset,
        actual: Offset,
    },
}
