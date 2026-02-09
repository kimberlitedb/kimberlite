//! Bridge between VSR messages and simulation network.
//!
//! This module provides serialization/deserialization for VSR messages
//! to enable protocol-level Byzantine testing. Messages are serialized
//! to bytes for transmission through the SimNetwork, then deserialized
//! at the destination.
//!
//! ## Broadcast Addressing
//!
//! VSR messages can be unicast (to a specific replica) or broadcast (to all).
//! The bridge uses `u64::MAX` as a sentinel value for broadcast messages.
//!
//! ## Usage
//!
//! ```ignore
//! // Serialize a VSR message
//! let bytes = serialize_vsr_message(&vsr_msg)?;
//!
//! // Deserialize back
//! let vsr_msg = deserialize_vsr_message(&bytes)?;
//! ```

use crate::SimError;
use kimberlite_vsr::{Message, MessagePayload, ReplicaId};
use serde::{Deserialize, Serialize};

/// Sentinel value for broadcast messages (u64::MAX).
pub const BROADCAST_ADDRESS: u64 = u64::MAX;

// ============================================================================
// VSR Message Wrapper for Serialization
// ============================================================================

/// Internal wrapper for VSR messages to enable serialization.
///
/// This wrapper exists because we need to serialize the entire Message
/// including its from/to/payload structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VsrMessageWire {
    from: u8,
    to: Option<u8>,
    payload: MessagePayload,
}

impl From<&Message> for VsrMessageWire {
    fn from(msg: &Message) -> Self {
        Self {
            from: msg.from.as_u8(),
            to: msg.to.map(|r| r.as_u8()),
            payload: msg.payload.clone(),
        }
    }
}

impl VsrMessageWire {
    fn into_message(self) -> Message {
        Message {
            from: ReplicaId::new(self.from),
            to: self.to.map(ReplicaId::new),
            payload: self.payload,
            signature: None, // Simulation messages are unsigned by default
        }
    }
}

// ============================================================================
// Serialization/Deserialization
// ============================================================================

/// Serializes a VSR message to bytes using postcard.
///
/// Returns the serialized bytes, or an error if serialization fails.
pub fn serialize_vsr_message(message: &Message) -> Result<Vec<u8>, SimError> {
    let wire_msg = VsrMessageWire::from(message);
    postcard::to_allocvec(&wire_msg).map_err(|e| SimError::Serialization(format!("{}", e)))
}

/// Deserializes bytes back to a VSR message using postcard.
///
/// Returns the deserialized message, or an error if deserialization fails.
pub fn deserialize_vsr_message(bytes: &[u8]) -> Result<Message, SimError> {
    let wire_msg: VsrMessageWire =
        postcard::from_bytes(bytes).map_err(|e| SimError::Deserialization(format!("{}", e)))?;
    Ok(wire_msg.into_message())
}

/// Converts a VSR ReplicaId to a network node ID.
///
/// Broadcast messages (to = None) are converted to BROADCAST_ADDRESS.
pub fn replica_to_network_id(replica: Option<ReplicaId>) -> u64 {
    match replica {
        Some(r) => r.as_u8() as u64,
        None => BROADCAST_ADDRESS,
    }
}

/// Converts a network node ID back to a VSR ReplicaId.
///
/// BROADCAST_ADDRESS is converted to None (broadcast).
pub fn network_id_to_replica(node_id: u64) -> Option<ReplicaId> {
    if node_id == BROADCAST_ADDRESS {
        None
    } else {
        Some(ReplicaId::new(node_id as u8))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_kernel::Command;
    use kimberlite_types::{DataClass, Placement, Region, StreamId, StreamName, TenantId};
    use kimberlite_vsr::{CommitNumber, DoViewChange, OpNumber, ViewNumber};

    #[test]
    fn test_serialize_deserialize_do_view_change() {
        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(100),
            commit_number: CommitNumber::new(OpNumber::new(50)),
            log_tail: vec![],
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc.clone()),
            signature: None,
        };

        // Serialize
        let bytes = serialize_vsr_message(&message).expect("serialization should succeed");

        // Deserialize
        let deserialized = deserialize_vsr_message(&bytes).expect("deserialization should succeed");

        // Verify
        assert_eq!(deserialized.from, message.from);
        assert_eq!(deserialized.to, message.to);

        if let MessagePayload::DoViewChange(deserialized_dvc) = deserialized.payload {
            assert_eq!(deserialized_dvc.view, dvc.view);
            assert_eq!(deserialized_dvc.commit_number, dvc.commit_number);
        } else {
            panic!("Expected DoViewChange payload");
        }
    }

    #[test]
    fn test_broadcast_addressing() {
        let message = Message {
            from: ReplicaId::new(0),
            to: None, // Broadcast
            payload: MessagePayload::DoViewChange(DoViewChange {
                view: ViewNumber::from(2),
                last_normal_view: ViewNumber::from(1),
                op_number: OpNumber::new(100),
                commit_number: CommitNumber::new(OpNumber::new(50)),
                log_tail: vec![],
                replica: ReplicaId::new(0),
                reconfig_state: None,
            }),
            signature: None,
        };

        // Convert to network ID
        let network_id = replica_to_network_id(message.to);
        assert_eq!(network_id, BROADCAST_ADDRESS);

        // Convert back
        let replica_id = network_id_to_replica(network_id);
        assert_eq!(replica_id, None);
    }

    #[test]
    fn test_unicast_addressing() {
        let replica = ReplicaId::new(5);

        // Convert to network ID
        let network_id = replica_to_network_id(Some(replica));
        assert_eq!(network_id, 5);

        // Convert back
        let converted = network_id_to_replica(network_id);
        assert_eq!(converted, Some(replica));
    }

    #[test]
    fn test_roundtrip_with_command() {
        let command = Command::CreateStream {
            stream_id: StreamId::from_tenant_and_local(TenantId::new(1), 1),
            stream_name: StreamName::from("test_stream"),
            data_class: DataClass::PHI,
            placement: Placement::Region(Region::USEast1),
        };

        // Create a DoViewChange with a log entry containing the command
        let log_entry = kimberlite_vsr::LogEntry {
            op_number: OpNumber::new(10),
            view: ViewNumber::from(1),
            command,
            idempotency_id: None,
            client_id: None,
            request_number: None,
            checksum: 12345,
        };

        let dvc = DoViewChange {
            view: ViewNumber::from(2),
            last_normal_view: ViewNumber::from(1),
            op_number: OpNumber::new(10),
            commit_number: CommitNumber::new(OpNumber::new(10)),
            log_tail: vec![log_entry],
            replica: ReplicaId::new(0),
            reconfig_state: None,
        };

        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc.clone()),
            signature: None,
        };

        // Roundtrip
        let bytes = serialize_vsr_message(&message).expect("serialization should succeed");
        let deserialized = deserialize_vsr_message(&bytes).expect("deserialization should succeed");

        // Verify log tail
        if let MessagePayload::DoViewChange(deserialized_dvc) = deserialized.payload {
            assert_eq!(deserialized_dvc.log_tail.len(), 1);
            assert_eq!(
                deserialized_dvc.log_tail[0].op_number,
                dvc.log_tail[0].op_number
            );
        } else {
            panic!("Expected DoViewChange payload");
        }
    }
}
