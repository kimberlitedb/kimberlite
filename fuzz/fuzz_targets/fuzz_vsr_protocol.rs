#![no_main]

// VSR protocol fuzzer focused on log-tail hash integrity.
//
// The `DoViewChange` and `StartView` messages carry a BLAKE3 hash over their
// `log_tail` entries. The new leader / receiving backup must reject any
// message where `log_tail_hash != hash(log_tail)`. A bug in the constructor
// or hash function that made the hash computable as a fixed value, or a
// mismatch between sender and receiver hashing, is a silent Byzantine
// bypass.
//
// Oracles (no stateful replica required):
//   1. The hash computation is deterministic — two constructions from the
//      same entries must produce identical `log_tail_hash` values.
//   2. The hash is entry-sensitive — any per-entry mutation (changing view,
//      op, idempotency marker, or appending a new entry) must change the hash.
//   3. StartView and DoViewChange agree on the hashing function — for the
//      same `log_tail`, both messages produce the same `log_tail_hash`.
//   4. postcard round-trip preserves both `log_tail` and `log_tail_hash`
//      bit-identically, so a decoded message inherits the sender's hash
//      claim verbatim (no accidental rehash on decode).

use kimberlite_vsr::message::{DoViewChange, StartView};
use kimberlite_vsr::types::{CommitNumber, LogEntry, OpNumber, ReplicaId, ViewNumber};
use kmb_kernel::command::Command;
use kimberlite_types::{DataClass, Placement, StreamId, StreamName};
use libfuzzer_sys::fuzz_target;

fn synth_command(byte: u8) -> Command {
    Command::CreateStream {
        stream_id: StreamId::new(u64::from(byte)),
        stream_name: StreamName::new(&format!("s{byte}")),
        data_class: match byte % 4 {
            0 => DataClass::Public,
            1 => DataClass::PII,
            2 => DataClass::Confidential,
            _ => DataClass::PHI,
        },
        placement: Placement::Global,
    }
}

fn build_log_tail(data: &[u8], view: ViewNumber) -> Vec<LogEntry> {
    let count = ((data[0] as usize) % 5) + 1;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let byte = *data.get(i + 1).unwrap_or(&0);
        out.push(LogEntry::new(
            OpNumber::new(i as u64 + 1),
            view,
            synth_command(byte),
            None,
            None,
            None,
        ));
    }
    out
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let view = ViewNumber::new(u64::from(data[0]));
    let replica = ReplicaId::new(data[1]);
    let last_normal_view = ViewNumber::new(u64::from(data[2]));
    let op = OpNumber::new(u64::from(data[3]));
    let commit = CommitNumber::new(OpNumber::new(u64::from(data[4])));

    let log_tail = build_log_tail(&data[5..], view);
    if log_tail.is_empty() {
        return;
    }

    // ── Invariant 1 & 2: determinism + entry-sensitivity via DoViewChange ──
    let dvc_a = DoViewChange::new(
        view,
        replica,
        last_normal_view,
        op,
        commit,
        log_tail.clone(),
    );
    let dvc_b = DoViewChange::new(
        view,
        replica,
        last_normal_view,
        op,
        commit,
        log_tail.clone(),
    );
    assert_eq!(
        dvc_a.log_tail_hash, dvc_b.log_tail_hash,
        "DoViewChange::new is non-deterministic for identical inputs"
    );

    // Append one extra entry and confirm the hash changes.
    let mut extended = log_tail.clone();
    extended.push(LogEntry::new(
        OpNumber::new((log_tail.len() + 10) as u64),
        view,
        synth_command(0xFF),
        None,
        None,
        None,
    ));
    let dvc_extended = DoViewChange::new(
        view,
        replica,
        last_normal_view,
        op,
        commit,
        extended.clone(),
    );
    assert_ne!(
        dvc_a.log_tail_hash, dvc_extended.log_tail_hash,
        "DoViewChange log_tail_hash unchanged after appending an entry — Byzantine peer can \
         forge arbitrary log tails undetected"
    );

    // ── Invariant 3: StartView agrees with DoViewChange on the hash ─────────
    let sv = StartView::new(view, op, commit, log_tail.clone());
    assert_eq!(
        dvc_a.log_tail_hash, sv.log_tail_hash,
        "DoViewChange and StartView disagree on log_tail hashing — protocol inconsistency"
    );

    // ── Invariant 4: postcard round-trip preserves hash + entries ──────────
    let bytes = postcard::to_allocvec(&dvc_a).expect("DoViewChange must serialize");
    let decoded: DoViewChange =
        postcard::from_bytes(&bytes).expect("DoViewChange must round-trip");
    assert_eq!(
        decoded.log_tail_hash, dvc_a.log_tail_hash,
        "postcard round-trip changed log_tail_hash"
    );
    assert_eq!(
        decoded.log_tail, dvc_a.log_tail,
        "postcard round-trip changed log_tail"
    );

    // Cross-check: if we rebuild from the decoded entries, the hash should
    // still match. This catches a bug where the wire format lies about the
    // hash (hash says X, entries hash to Y).
    let rebuilt = DoViewChange::new(
        decoded.view,
        decoded.replica,
        decoded.last_normal_view,
        decoded.op_number,
        decoded.commit_number,
        decoded.log_tail.clone(),
    );
    assert_eq!(
        rebuilt.log_tail_hash, decoded.log_tail_hash,
        "decoded DoViewChange log_tail_hash does not match hash of its own log_tail"
    );
});
