//! # Step 2: Commands and Effects
//!
//! **Learning objective:** Understand how to represent operations and side effects as data.
//!
//! ## Key Concepts
//!
//! **Command**: A request to change state. Commands are inputs to the functional core.
//! - Represents WHAT to do (declarative)
//! - Pure data structure (enum or struct)
//! - Serializable (can be sent over network, written to disk)
//!
//! **Effect**: A side effect to execute. Effects are outputs from the functional core.
//! - Represents HOW to interact with the outside world
//! - Pure data structure describing the side effect
//! - Executed by the imperative shell, not the functional core
//!
//! ## Why Use Commands and Effects?
//!
//! 1. **Testability**: Test the core without executing side effects
//! 2. **Replayability**: Re-apply commands to reconstruct state
//! 3. **Introspection**: Log, audit, or modify commands before execution
//! 4. **Separation**: Core logic separate from IO/storage/network
//!
//! ## The Pattern
//!
//! ```text
//! Command (data) → Functional Core → (New State, Vec<Effect>)
//!                                              ↓
//!                                    Imperative Shell
//!                                              ↓
//!                                    Side Effects Executed
//! ```

use bytes::Bytes;
use serde::{Deserialize, Serialize};

// ============================================================================
// Stream Types (Building Blocks)
// ============================================================================

/// Unique identifier for a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StreamId(pub u64);

impl StreamId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Position in a stream (event number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Offset(pub u64);

impl Offset {
    pub const ZERO: Self = Self(0);

    pub fn new(offset: u64) -> Self {
        Self(offset)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    pub fn increment_by(&self, count: u64) -> Self {
        Self(self.0 + count)
    }
}

/// Data classification for compliance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataClass {
    Public,
    Internal,
    Confidential,
    Restricted,
}

// ============================================================================
// Commands: Inputs to the Kernel
// ============================================================================

/// Commands that can be sent to the kernel.
///
/// Each variant represents a different operation on the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Create a new event stream.
    CreateStream {
        stream_id: StreamId,
        stream_name: String,
        data_class: DataClass,
    },

    /// Append events to an existing stream.
    AppendBatch {
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
    },

    /// Read events from a stream (query).
    ReadStream {
        stream_id: StreamId,
        from_offset: Offset,
        max_events: usize,
    },
}

impl Command {
    /// Constructor for CreateStream command.
    pub fn create_stream(stream_id: StreamId, stream_name: String, data_class: DataClass) -> Self {
        Self::CreateStream {
            stream_id,
            stream_name,
            data_class,
        }
    }

    /// Constructor for AppendBatch command.
    pub fn append_batch(
        stream_id: StreamId,
        events: Vec<Bytes>,
        expected_offset: Offset,
    ) -> Self {
        Self::AppendBatch {
            stream_id,
            events,
            expected_offset,
        }
    }

    /// Constructor for ReadStream command.
    pub fn read_stream(stream_id: StreamId, from_offset: Offset, max_events: usize) -> Self {
        Self::ReadStream {
            stream_id,
            from_offset,
            max_events,
        }
    }
}

// ============================================================================
// Effects: Outputs from the Kernel
// ============================================================================

/// Stream metadata to persist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamMetadata {
    pub stream_id: StreamId,
    pub stream_name: String,
    pub data_class: DataClass,
    pub current_offset: Offset,
}

/// Effects produced by the kernel for the runtime to execute.
///
/// These describe side effects but don't execute them.
/// The imperative shell is responsible for execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// Write events to durable storage.
    StorageAppend {
        stream_id: StreamId,
        base_offset: Offset,
        events: Vec<Bytes>,
    },

    /// Persist stream metadata.
    MetadataWrite(StreamMetadata),

    /// Log an audit event.
    AuditLog {
        action: String,
        stream_id: StreamId,
        timestamp_ms: u64,
    },

    /// Wake up projections to process new events.
    WakeProjection {
        stream_id: StreamId,
        from_offset: Offset,
        to_offset: Offset,
    },

    /// Send response to client (for queries).
    SendResponse {
        events: Vec<Bytes>,
        next_offset: Offset,
    },
}

// ============================================================================
// Command Validation (Pure Functions)
// ============================================================================

/// Validation error for commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidStreamName(String),
    EmptyBatch,
    InvalidOffset,
    InvalidMaxEvents,
}

/// Validates a CreateStream command.
///
/// PURE: No side effects, deterministic validation.
pub fn validate_create_stream(stream_name: &str) -> Result<(), ValidationError> {
    if stream_name.is_empty() {
        return Err(ValidationError::InvalidStreamName(
            "Stream name cannot be empty".to_string(),
        ));
    }

    if stream_name.len() > 255 {
        return Err(ValidationError::InvalidStreamName(
            "Stream name too long (max 255 chars)".to_string(),
        ));
    }

    Ok(())
}

/// Validates an AppendBatch command.
///
/// PURE: No side effects, deterministic validation.
pub fn validate_append_batch(events: &[Bytes]) -> Result<(), ValidationError> {
    if events.is_empty() {
        return Err(ValidationError::EmptyBatch);
    }

    Ok(())
}

/// Validates a ReadStream command.
///
/// PURE: No side effects, deterministic validation.
pub fn validate_read_stream(max_events: usize) -> Result<(), ValidationError> {
    if max_events == 0 {
        return Err(ValidationError::InvalidMaxEvents);
    }

    Ok(())
}

// ============================================================================
// Example: Command Transformation (No State Yet)
// ============================================================================

/// Transforms a command into effects WITHOUT state.
///
/// This is a simplified example to show the pattern.
/// Step 4 will add state management.
pub fn command_to_effects(cmd: Command, timestamp_ms: u64) -> Vec<Effect> {
    match cmd {
        Command::CreateStream {
            stream_id,
            stream_name,
            data_class,
        } => vec![
            Effect::MetadataWrite(StreamMetadata {
                stream_id,
                stream_name: stream_name.clone(),
                data_class,
                current_offset: Offset::ZERO,
            }),
            Effect::AuditLog {
                action: format!("CreateStream: {}", stream_name),
                stream_id,
                timestamp_ms,
            },
        ],

        Command::AppendBatch {
            stream_id,
            events,
            expected_offset,
        } => {
            let to_offset = expected_offset.increment_by(events.len() as u64);
            vec![
                Effect::StorageAppend {
                    stream_id,
                    base_offset: expected_offset,
                    events: events.clone(),
                },
                Effect::WakeProjection {
                    stream_id,
                    from_offset: expected_offset,
                    to_offset,
                },
                Effect::AuditLog {
                    action: format!("AppendBatch: {} events", events.len()),
                    stream_id,
                    timestamp_ms,
                },
            ]
        }

        Command::ReadStream {
            stream_id: _,
            from_offset,
            max_events: _,
        } => {
            // Simplified: return empty response
            vec![Effect::SendResponse {
                events: vec![],
                next_offset: from_offset,
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_stream_command_serialization() {
        let cmd = Command::create_stream(
            StreamId::new(1),
            "events".to_string(),
            DataClass::Internal,
        );

        // Commands can be serialized (sent over network)
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: Command = serde_json::from_str(&json).unwrap();

        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn validation_rejects_invalid_commands() {
        assert!(validate_create_stream("").is_err());
        assert!(validate_create_stream("a".repeat(256).as_str()).is_err());
        assert!(validate_append_batch(&[]).is_err());
        assert!(validate_read_stream(0).is_err());
    }

    #[test]
    fn validation_accepts_valid_commands() {
        assert!(validate_create_stream("events").is_ok());
        assert!(validate_append_batch(&[Bytes::from("event")]).is_ok());
        assert!(validate_read_stream(100).is_ok());
    }

    #[test]
    fn create_stream_produces_effects() {
        let cmd = Command::create_stream(
            StreamId::new(1),
            "events".to_string(),
            DataClass::Internal,
        );

        let effects = command_to_effects(cmd, 1000);

        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::MetadataWrite(_)));
        assert!(matches!(effects[1], Effect::AuditLog { .. }));
    }

    #[test]
    fn append_batch_produces_effects() {
        let cmd = Command::append_batch(
            StreamId::new(1),
            vec![Bytes::from("event1"), Bytes::from("event2")],
            Offset::ZERO,
        );

        let effects = command_to_effects(cmd, 1000);

        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::StorageAppend { .. }));
        assert!(matches!(effects[1], Effect::WakeProjection { .. }));
        assert!(matches!(effects[2], Effect::AuditLog { .. }));
    }

    #[test]
    fn effects_are_deterministic() {
        let cmd = Command::create_stream(
            StreamId::new(42),
            "test".to_string(),
            DataClass::Public,
        );

        let timestamp = 5000;

        // Call twice with same inputs
        let effects1 = command_to_effects(cmd.clone(), timestamp);
        let effects2 = command_to_effects(cmd, timestamp);

        // Results must be identical
        assert_eq!(effects1, effects2);
    }
}
