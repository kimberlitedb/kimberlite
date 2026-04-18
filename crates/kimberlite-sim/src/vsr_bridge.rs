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
use crate::rng::SimRng;
use kimberlite_vsr::{Message, MessagePayload, ReplicaId};
use serde::{Deserialize, Serialize};

/// Sentinel value for broadcast messages (u64::MAX).
pub const BROADCAST_ADDRESS: u64 = u64::MAX;

// ============================================================================
// Adversarial Wire Mutator (T1.4)
// ============================================================================

/// Kind of mutation applied to a serialized VSR frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// Flipped a single bit at a random offset.
    BitFlip,
    /// Replaced a single byte with a random value.
    ByteReplace,
    /// Truncated the frame at a random offset.
    Truncate,
    /// Appended random garbage bytes to the frame.
    Extend,
}

/// Deterministic, post-encoding byte-level mutator for VSR wire frames.
///
/// Motivation: VOPR's workload generator only produces structurally valid
/// commands, so the VSR state machine never sees a malformed `Prepare`. Fuzzing
/// catches bugs VOPR misses exactly because fuzzing feeds arbitrary bytes
/// through the codec. This mutator closes that gap by corrupting serialized
/// frames *after* encoding but *before* network delivery, so the receive path
/// must tolerate Byzantine wire traffic during otherwise-valid scenarios.
///
/// The mutator is deterministic: given the same RNG sequence, it produces the
/// same mutations every run. Bugs it surfaces are reproducible from the seed.
#[derive(Debug, Clone)]
pub struct WireMutator {
    /// Probability (0.0–1.0) that any given frame is mutated.
    pub mutation_probability: f64,
    /// Maximum bytes to append when extending a frame.
    pub max_extend_bytes: usize,
    mutations_applied: u64,
}

impl WireMutator {
    /// Creates a mutator with the given per-frame mutation probability.
    pub fn new(mutation_probability: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&mutation_probability),
            "mutation_probability must be 0.0 to 1.0"
        );
        Self {
            mutation_probability,
            max_extend_bytes: 64,
            mutations_applied: 0,
        }
    }

    /// A disabled mutator (never mutates). Safe to use in existing scenarios.
    pub fn disabled() -> Self {
        Self::new(0.0)
    }

    /// A low-rate mutator (0.001 per frame) suitable for combined scenarios
    /// where you want occasional wire corruption alongside normal workload.
    pub fn low_rate() -> Self {
        Self::new(0.001)
    }

    /// A high-rate mutator (0.1 per frame) for dedicated byzantine-wire
    /// scenarios that stress codec robustness.
    pub fn high_rate() -> Self {
        Self::new(0.1)
    }

    /// Total number of mutations applied by this instance. Useful for scenario
    /// reports to confirm the mutator was actually exercised.
    pub fn mutations_applied(&self) -> u64 {
        self.mutations_applied
    }

    /// Maybe mutate `bytes` in place. Returns `Some(kind)` if a mutation was
    /// applied, `None` otherwise.
    ///
    /// Mutations are chosen uniformly from [`MutationKind`]. Empty frames are
    /// never mutated (nothing to corrupt).
    pub fn maybe_mutate(&mut self, bytes: &mut Vec<u8>, rng: &mut SimRng) -> Option<MutationKind> {
        if bytes.is_empty() || !rng.next_bool_with_probability(self.mutation_probability) {
            return None;
        }
        let kind = match rng.next_usize(4) {
            0 => MutationKind::BitFlip,
            1 => MutationKind::ByteReplace,
            2 => MutationKind::Truncate,
            _ => MutationKind::Extend,
        };
        self.apply(kind, bytes, rng);
        self.mutations_applied += 1;
        Some(kind)
    }

    fn apply(&self, kind: MutationKind, bytes: &mut Vec<u8>, rng: &mut SimRng) {
        match kind {
            MutationKind::BitFlip => {
                let offset = rng.next_usize(bytes.len());
                let bit = rng.next_usize(8) as u8;
                bytes[offset] ^= 1 << bit;
            }
            MutationKind::ByteReplace => {
                let offset = rng.next_usize(bytes.len());
                bytes[offset] = rng.next_usize(256) as u8;
            }
            MutationKind::Truncate => {
                // Truncate to a random non-empty prefix (size 1..len).
                let new_len = 1 + rng.next_usize(bytes.len().saturating_sub(1).max(1));
                bytes.truncate(new_len.min(bytes.len()));
            }
            MutationKind::Extend => {
                let extend_by = 1 + rng.next_usize(self.max_extend_bytes.max(1));
                for _ in 0..extend_by {
                    bytes.push(rng.next_usize(256) as u8);
                }
            }
        }
    }
}

impl Default for WireMutator {
    fn default() -> Self {
        Self::disabled()
    }
}

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
        let dvc = DoViewChange::new(
            ViewNumber::from(2),
            ReplicaId::new(0),
            ViewNumber::from(1),
            OpNumber::new(100),
            CommitNumber::new(OpNumber::new(50)),
            vec![],
        );

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
            payload: MessagePayload::DoViewChange(DoViewChange::new(
                ViewNumber::from(2),
                ReplicaId::new(0),
                ViewNumber::from(1),
                OpNumber::new(100),
                CommitNumber::new(OpNumber::new(50)),
                vec![],
            )),
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
    fn wire_mutator_disabled_is_pass_through() {
        let mut mutator = WireMutator::disabled();
        let mut rng = SimRng::new(42);
        let original = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        let mut bytes = original.clone();

        for _ in 0..1000 {
            let r = mutator.maybe_mutate(&mut bytes, &mut rng);
            assert!(r.is_none());
        }
        assert_eq!(bytes, original);
        assert_eq!(mutator.mutations_applied(), 0);
    }

    #[test]
    fn wire_mutator_always_mutates_non_empty() {
        let mut mutator = WireMutator::new(1.0);
        let mut rng = SimRng::new(42);
        let original: Vec<u8> = (0..64).collect();
        let mut bytes = original.clone();

        let r = mutator.maybe_mutate(&mut bytes, &mut rng);
        assert!(r.is_some());
        assert_eq!(mutator.mutations_applied(), 1);
        // Bytes changed in some way (length differs, or content differs).
        assert!(bytes != original || bytes.len() != original.len());
    }

    #[test]
    fn wire_mutator_empty_frame_noop() {
        let mut mutator = WireMutator::new(1.0);
        let mut rng = SimRng::new(42);
        let mut bytes: Vec<u8> = vec![];
        assert!(mutator.maybe_mutate(&mut bytes, &mut rng).is_none());
        assert!(bytes.is_empty());
    }

    #[test]
    fn wire_mutator_is_deterministic() {
        let original: Vec<u8> = (0..64).collect();

        let mut mutator_a = WireMutator::new(1.0);
        let mut rng_a = SimRng::new(12345);
        let mut bytes_a = original.clone();
        mutator_a.maybe_mutate(&mut bytes_a, &mut rng_a);

        let mut mutator_b = WireMutator::new(1.0);
        let mut rng_b = SimRng::new(12345);
        let mut bytes_b = original.clone();
        mutator_b.maybe_mutate(&mut bytes_b, &mut rng_b);

        assert_eq!(bytes_a, bytes_b);
    }

    #[test]
    fn wire_mutator_mutated_frames_fail_gracefully() {
        // A mutated VSR frame should cause deserialize_vsr_message to return
        // Err rather than panic. (The VSR receive path is expected to drop
        // malformed frames; this test confirms the codec-layer contract.)
        let dvc = DoViewChange::new(
            ViewNumber::from(2),
            ReplicaId::new(0),
            ViewNumber::from(1),
            OpNumber::new(10),
            CommitNumber::new(OpNumber::new(10)),
            vec![],
        );
        let message = Message {
            from: ReplicaId::new(0),
            to: Some(ReplicaId::new(1)),
            payload: MessagePayload::DoViewChange(dvc),
            signature: None,
        };

        let mut mutator = WireMutator::new(1.0);
        let mut rng = SimRng::new(99);

        let mut ok = 0_u32;
        let mut err = 0_u32;
        for _ in 0..200 {
            let mut bytes =
                serialize_vsr_message(&message).expect("serialization should not fail");
            mutator.maybe_mutate(&mut bytes, &mut rng);
            // Must not panic. Accept either result.
            match deserialize_vsr_message(&bytes) {
                Ok(_) => ok += 1,
                Err(_) => err += 1,
            }
        }
        // Most mutations should fail deserialization; a small number of
        // byte-flips on padding/enum discriminants might still decode. Both
        // outcomes are fine — the panic-free requirement is what we're
        // asserting.
        assert!(ok + err == 200);
        assert!(err > 50, "expected most mutated frames to fail decode: ok={ok} err={err}");
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

        let dvc = DoViewChange::new(
            ViewNumber::from(2),
            ReplicaId::new(0),
            ViewNumber::from(1),
            OpNumber::new(10),
            CommitNumber::new(OpNumber::new(10)),
            vec![log_entry],
        );

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
