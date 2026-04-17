#![no_main]
//! Structure-aware fuzzing of VSR protocol messages.
//!
//! Complements `fuzz_vsr_protocol` (byte-level) by synthesising structurally
//! valid `MessagePayload` variants and exercising the serialisation round-trip
//! + the `view()` / `name()` / `is_broadcast()` accessors that handlers call
//! before dispatch.
//!
//! Scope: simple payload variants that don't require a valid log to construct.
//! Variants carrying `LogEntry` or `VersionInfo` or computed hashes (Prepare,
//! PrepareOk, Heartbeat, DoViewChange, StartView, RepairResponse,
//! RecoveryResponse, StateTransferResponse, WriteReorderGapResponse) are
//! deferred — they require a valid log to construct, which is out of scope
//! for structure-aware mutation and is handled by `fuzz_vsr_protocol`.

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct FuzzableVsrMessage {
    from: u8,
    to: Option<u8>,
    payload: FuzzablePayload,
}

// Nonce bytes stored as an array so Arbitrary can generate them byte-by-byte.
#[derive(Debug, Arbitrary)]
enum FuzzablePayload {
    StartViewChange {
        view: u64,
        replica: u8,
    },
    RecoveryRequest {
        replica: u8,
        nonce_bytes: [u8; 16],
        known_op_number: u64,
    },
    Commit {
        view: u64,
        commit_number: u64,
    },
    Nack {
        replica: u8,
        nonce_bytes: [u8; 16],
        reason: u8,
        highest_seen: u64,
    },
    RepairRequest {
        replica: u8,
        nonce_bytes: [u8; 16],
        op_range_start: u64,
        op_range_end: u64,
    },
    StateTransferRequest {
        replica: u8,
        nonce_bytes: [u8; 16],
        known_checkpoint: u64,
    },
    WriteReorderGapRequest {
        from_replica: u8,
        nonce_bytes: [u8; 16],
        missing_ops: Vec<u64>,
    },
}

fn try_build_message(f: FuzzableVsrMessage) -> Option<kimberlite_vsr::Message> {
    use kimberlite_vsr::message::{
        Commit, Nack, RecoveryRequest, RepairRequest, StartViewChange, StateTransferRequest,
        WriteReorderGapRequest,
    };
    use kimberlite_vsr::types::{CommitNumber, Nonce, OpNumber, ReplicaId, ViewNumber};
    use kimberlite_vsr::{Message, MessagePayload, NackReason};

    let from = ReplicaId::new(f.from);

    let payload = match f.payload {
        FuzzablePayload::StartViewChange { view, replica } => {
            MessagePayload::StartViewChange(StartViewChange {
                view: ViewNumber::new(view),
                replica: ReplicaId::new(replica),
            })
        }
        FuzzablePayload::RecoveryRequest {
            replica,
            nonce_bytes,
            known_op_number,
        } => MessagePayload::RecoveryRequest(RecoveryRequest {
            replica: ReplicaId::new(replica),
            nonce: Nonce::from_bytes(nonce_bytes),
            known_op_number: OpNumber::new(known_op_number),
        }),
        FuzzablePayload::Commit {
            view,
            commit_number,
        } => MessagePayload::Commit(Commit {
            view: ViewNumber::new(view),
            commit_number: CommitNumber::new(OpNumber::new(commit_number)),
        }),
        FuzzablePayload::Nack {
            replica,
            nonce_bytes,
            reason,
            highest_seen,
        } => {
            let reason = match reason % 3 {
                0 => NackReason::NotSeen,
                1 => NackReason::SeenButCorrupt,
                _ => NackReason::Recovering,
            };
            MessagePayload::Nack(Nack {
                replica: ReplicaId::new(replica),
                nonce: Nonce::from_bytes(nonce_bytes),
                reason,
                highest_seen: OpNumber::new(highest_seen),
            })
        }
        FuzzablePayload::RepairRequest {
            replica,
            nonce_bytes,
            op_range_start,
            op_range_end,
        } => {
            // RepairRequest invariants reject empty ranges at construction
            // via debug_assert — preserve that at the fuzz boundary.
            if op_range_start >= op_range_end {
                return None;
            }
            MessagePayload::RepairRequest(RepairRequest {
                replica: ReplicaId::new(replica),
                nonce: Nonce::from_bytes(nonce_bytes),
                op_range_start: OpNumber::new(op_range_start),
                op_range_end: OpNumber::new(op_range_end),
            })
        }
        FuzzablePayload::StateTransferRequest {
            replica,
            nonce_bytes,
            known_checkpoint,
        } => MessagePayload::StateTransferRequest(StateTransferRequest {
            replica: ReplicaId::new(replica),
            nonce: Nonce::from_bytes(nonce_bytes),
            known_checkpoint: OpNumber::new(known_checkpoint),
        }),
        FuzzablePayload::WriteReorderGapRequest {
            from_replica,
            nonce_bytes,
            missing_ops,
        } => {
            // WriteReorderGapRequest::new debug_asserts non-empty ops; skip
            // empty inputs rather than trip the debug assert.
            if missing_ops.is_empty() {
                return None;
            }
            MessagePayload::WriteReorderGapRequest(WriteReorderGapRequest {
                from: ReplicaId::new(from_replica),
                nonce: Nonce::from_bytes(nonce_bytes),
                missing_ops: missing_ops.into_iter().map(OpNumber::new).collect(),
            })
        }
    };

    Some(match f.to {
        Some(to) => Message::targeted(from, ReplicaId::new(to), payload),
        None => Message::broadcast(from, payload),
    })
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(fuzzable) = FuzzableVsrMessage::arbitrary(&mut u) else {
        return;
    };
    let Some(msg) = try_build_message(fuzzable) else {
        return;
    };

    // 1. Accessors must never panic.
    let _ = msg.payload.view();
    let _ = msg.payload.name();
    let _ = msg.is_broadcast();
    let _ = msg.is_targeted();

    // 2. Serialisation round-trip (postcard).
    let encoded = postcard::to_allocvec(&msg).expect("VSR message serialisation must not fail");
    let decoded: kimberlite_vsr::Message =
        postcard::from_bytes(&encoded).expect("round-trip must be stable");

    // 3. Deterministic re-encoding.
    let reencoded = postcard::to_allocvec(&decoded).expect("re-encoding must succeed");
    assert_eq!(
        encoded, reencoded,
        "VSR message encoding must be deterministic"
    );
});
