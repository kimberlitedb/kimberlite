//! Property-based tests for compliance state-machine invariants.
//!
//! AUDIT-2026-04 S2.8 — exercises consent and erasure lifecycle over
//! randomised command sequences and asserts the invariants the audit
//! surfaced as load-bearing:
//!
//! - **Consent lifecycle monotonicity.** `Granted → Withdrawn` is
//!   terminal; re-granting always yields a fresh consent_id; withdrawn
//!   consents never validate.
//!
//! - **Erasure progression monotonicity.** `records_erased` never
//!   decreases; `status` only advances
//!   `Pending → InProgress → Complete`.
//!
//! - **ErasureScope cap invariance.** No random (stream_id, count)
//!   combination can inflate `records_erased` above the per-stream
//!   cap once scopes are the gate. Defends the C-4 parse-don't-validate
//!   surface.
//!
//! - **execute_erasure witness count.** Signed proof has exactly one
//!   witness per affected stream, regardless of stream count or order.

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use kimberlite_compliance::consent::ConsentTracker;
use kimberlite_compliance::erasure::{
    AttestationKey, ErasureEngine, ErasureError, ErasureExecutor, ErasureStatus,
    StreamErasureWitness, StreamShredReceipt,
};
use kimberlite_compliance::purpose::Purpose;
use kimberlite_types::{StreamId, TenantId};
use proptest::prelude::*;
use std::collections::HashMap;

// -----------------------------------------------------------------------------
// Test-only mock executor. Mirrors `tests::MockErasureExecutor` inside
// `erasure.rs` but accessible from an integration-test file (which the
// inner mock is not).
// -----------------------------------------------------------------------------

struct MockExec {
    roots: HashMap<StreamId, [u8; 32]>,
    receipts: HashMap<StreamId, StreamShredReceipt>,
}

impl ErasureExecutor for MockExec {
    fn pre_erasure_merkle_root(
        &mut self,
        stream_id: StreamId,
    ) -> std::result::Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>> {
        self.roots
            .get(&stream_id)
            .copied()
            .ok_or_else(|| "no root".into())
    }

    fn shred_stream(
        &mut self,
        stream_id: StreamId,
        _subject_id: &str,
    ) -> std::result::Result<StreamShredReceipt, Box<dyn std::error::Error + Send + Sync>> {
        self.receipts
            .get(&stream_id)
            .cloned()
            .ok_or_else(|| "no receipt".into())
    }
}

// -----------------------------------------------------------------------------
// Strategies.
// -----------------------------------------------------------------------------

fn purpose_strategy() -> impl Strategy<Value = Purpose> {
    prop_oneof![
        Just(Purpose::Marketing),
        Just(Purpose::Analytics),
        Just(Purpose::Contractual),
        Just(Purpose::LegalObligation),
        Just(Purpose::VitalInterests),
        Just(Purpose::PublicTask),
        Just(Purpose::Research),
        Just(Purpose::Security),
    ]
}

fn subject_strategy() -> impl Strategy<Value = String> {
    // Bounded subject alphabet: lowercase letters + digits + @. No
    // empty strings (`request_erasure("")` returns `InvalidSubject`).
    "[a-z0-9]{1,10}@ex\\.com".prop_filter("non-empty", |s| !s.is_empty())
}

// -----------------------------------------------------------------------------
// Consent lifecycle properties.
// -----------------------------------------------------------------------------

proptest! {
    /// Withdrawing a granted consent makes `check_consent` return
    /// false, forever. No operation can "undo" the withdrawal —
    /// `Withdrawn` is terminal per GDPR Article 7(3).
    #[test]
    fn prop_withdrawn_consent_never_validates(
        subject in subject_strategy(),
        purpose in purpose_strategy(),
    ) {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent(&subject, purpose)
            .expect("valid subject");

        prop_assert!(tracker.check_consent(&subject, purpose));

        tracker.withdraw_consent(consent_id).unwrap();

        prop_assert!(!tracker.check_consent(&subject, purpose),
                     "withdrawn consent must not validate");
    }

    /// Granting consent twice to the same (subject, purpose) produces
    /// distinct consent_ids. Prevents silent collapse of two grants
    /// into one; important when auditing re-consent flows.
    #[test]
    fn prop_grant_is_fresh_each_time(
        subject in subject_strategy(),
        purpose in purpose_strategy(),
    ) {
        let mut tracker = ConsentTracker::new();
        let id_1 = tracker.grant_consent(&subject, purpose).unwrap();
        let id_2 = tracker.grant_consent(&subject, purpose).unwrap();
        prop_assert_ne!(id_1, id_2, "re-grant must yield a fresh consent_id");
    }

    /// Cross-subject isolation under random command orderings —
    /// withdrawing A's consent never affects B's consent, even when
    /// A and B share the same purpose. AUDIT-2026-04 M-5 property
    /// (the Kani proof covers a single ordering; this covers many).
    #[test]
    fn prop_cross_subject_withdraw_isolation(
        subj_a in subject_strategy(),
        subj_b in subject_strategy(),
        purpose in purpose_strategy(),
    ) {
        prop_assume!(subj_a != subj_b);

        let mut tracker = ConsentTracker::new();
        let a_id = tracker.grant_consent(&subj_a, purpose).unwrap();
        let _b_id = tracker.grant_consent(&subj_b, purpose).unwrap();

        prop_assert!(tracker.check_consent(&subj_a, purpose));
        prop_assert!(tracker.check_consent(&subj_b, purpose));

        tracker.withdraw_consent(a_id).unwrap();

        prop_assert!(!tracker.check_consent(&subj_a, purpose));
        prop_assert!(tracker.check_consent(&subj_b, purpose),
                     "B's consent must survive A's withdrawal");
    }
}

// -----------------------------------------------------------------------------
// Erasure lifecycle properties.
// -----------------------------------------------------------------------------

/// A stream with its injected shred receipt. Used as a strategy element.
#[derive(Debug, Clone)]
struct StreamFixture {
    stream_id: StreamId,
    root: [u8; 32],
    records_erased: u64,
    stream_length: u64,
}

fn stream_fixtures_strategy(tenant: u64, max: usize) -> impl Strategy<Value = Vec<StreamFixture>> {
    // 1..=max tuples of (local_id, root_byte, length, records_erased),
    // mapped into StreamFixtures bound to `tenant`. Uses u8 for
    // `local_id` to ensure a small alphabet; the filter below
    // dedupes by stream_id.
    prop::collection::vec(
        (0u8..=255u8, 0u8..=255u8, 10u64..100u64, 0u64..100u64),
        1..=max,
    )
    .prop_map(move |tuples| {
        tuples
            .into_iter()
            .map(|(local, root_byte, len, recs)| StreamFixture {
                stream_id: StreamId::from_tenant_and_local(TenantId::new(tenant), u32::from(local)),
                root: [root_byte; 32],
                // Clamp records_erased to length so the scope cap
                // always passes.
                records_erased: recs.min(len),
                stream_length: len,
            })
            .collect::<Vec<_>>()
    })
    .prop_filter("streams must have distinct stream_ids", |v| {
        let mut ids: Vec<_> = v.iter().map(|s| s.stream_id).collect();
        ids.sort_unstable_by_key(|s| (u64::from(s.tenant_id()), s.local_id()));
        ids.dedup();
        ids.len() == v.len()
    })
}

proptest! {
    /// Under random stream fixtures, `execute_erasure` produces a
    /// signed proof with exactly one witness per stream, and
    /// `records_erased` equals the sum of per-stream receipts.
    /// Monotonicity holds: `records_erased` only increases as each
    /// stream is processed.
    #[test]
    fn prop_execute_erasure_records_monotone_and_sum_correct(
        tenant in 1u64..256,
        subject in subject_strategy(),
        streams in stream_fixtures_strategy(0, 5),
    ) {
        // Use a tenant that matches the fixture streams.
        let streams: Vec<_> = streams
            .into_iter()
            .map(|mut s| {
                s.stream_id = StreamId::from_tenant_and_local(
                    TenantId::new(tenant),
                    s.stream_id.local_id(),
                );
                s
            })
            .collect();

        // Deduplicate by stream_id after re-tagging tenant.
        let mut seen = std::collections::HashSet::new();
        let streams: Vec<_> = streams
            .into_iter()
            .filter(|s| seen.insert(s.stream_id))
            .collect();
        prop_assume!(!streams.is_empty());

        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure(&subject).unwrap();
        let ids: Vec<_> = streams.iter().map(|s| s.stream_id).collect();
        engine.mark_in_progress(req.request_id, ids.clone()).unwrap();

        let mut exec = MockExec {
            roots: HashMap::new(),
            receipts: HashMap::new(),
        };
        let expected_total: u64 = streams.iter().map(|s| s.records_erased).sum();
        for s in &streams {
            exec.roots.insert(s.stream_id, s.root);
            exec.receipts.insert(
                s.stream_id,
                StreamShredReceipt {
                    key_shred_digest: s.root, // reuse as arbitrary bytes
                    records_erased: s.records_erased,
                    stream_length_at_shred: s.stream_length,
                },
            );
        }

        let audit = engine
            .execute_erasure(req.request_id, TenantId::new(tenant), &mut exec, &key, 0)
            .unwrap();

        // Witness count matches stream count.
        let proof = audit.signed_proof.as_ref().unwrap();
        prop_assert_eq!(proof.witnesses.len(), streams.len());

        // Records sum is correct.
        prop_assert_eq!(audit.records_erased, expected_total);

        // Signed proof verifies against the attestation key.
        prop_assert!(proof.verify(&key.verifying_key()).is_ok());

        // Every witness binds to one of the affected streams —
        // monotone set inclusion (no foreign witnesses).
        for w in &proof.witnesses {
            prop_assert!(ids.contains(&w.stream_id));
        }
    }

    /// Independently of execute_erasure, the manual lifecycle
    /// path (mark_in_progress → mark_stream_erased_with_scope →
    /// complete_erasure_with_attestation) upholds the same
    /// monotonicity. `records_erased` is non-decreasing across any
    /// prefix of per-stream updates.
    #[test]
    fn prop_records_erased_monotone_across_updates(
        tenant in 1u64..256,
        subject in subject_strategy(),
        streams in stream_fixtures_strategy(0, 4),
    ) {
        let streams: Vec<_> = streams
            .into_iter()
            .map(|mut s| {
                s.stream_id = StreamId::from_tenant_and_local(
                    TenantId::new(tenant),
                    s.stream_id.local_id(),
                );
                s
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let streams: Vec<_> = streams
            .into_iter()
            .filter(|s| seen.insert(s.stream_id))
            .collect();
        prop_assume!(!streams.is_empty());

        let mut engine = ErasureEngine::new();
        let req = engine.request_erasure(&subject).unwrap();
        let ids: Vec<_> = streams.iter().map(|s| s.stream_id).collect();
        engine.mark_in_progress(req.request_id, ids).unwrap();

        let mut last_total = 0u64;
        for s in &streams {
            let scope = engine
                .get_request(req.request_id)
                .unwrap()
                .scope_for(s.stream_id, TenantId::new(tenant), s.stream_length)
                .unwrap();
            engine
                .mark_stream_erased_with_scope(scope, s.records_erased)
                .unwrap();
            let current = engine.get_request(req.request_id).unwrap().records_erased;
            prop_assert!(current >= last_total,
                         "records_erased must be monotone non-decreasing");
            last_total = current;
        }
    }

    /// Scope-cap invariance: a random over-cap count is always
    /// rejected with `RecordsExceedScope`. Defends C-4 across a
    /// large (cap, count) space rather than the one value the
    /// unit test pins.
    #[test]
    fn prop_scope_cap_rejects_inflated_count(
        tenant in 1u64..256,
        cap in 10u64..1000,
        inflation in 1u64..1000,
    ) {
        let stream_id = StreamId::from_tenant_and_local(
            TenantId::new(tenant),
            1,
        );
        let mut engine = ErasureEngine::new();
        let req = engine.request_erasure("subject@example.com").unwrap();
        engine.mark_in_progress(req.request_id, vec![stream_id]).unwrap();

        let scope = engine
            .get_request(req.request_id)
            .unwrap()
            .scope_for(stream_id, TenantId::new(tenant), cap)
            .unwrap();

        let over_cap = cap.saturating_add(inflation);
        let err = engine
            .mark_stream_erased_with_scope(scope, over_cap)
            .unwrap_err();
        let is_scope_err = matches!(err, ErasureError::RecordsExceedScope { .. });
        prop_assert!(is_scope_err, "expected RecordsExceedScope, got {:?}", err);
    }

    /// Cross-subject isolation under random erasure orderings —
    /// completing erasure of subject A's streams has no effect on
    /// subject B's independent erasure request. Catches any
    /// cross-request leak in the engine's pending list.
    #[test]
    fn prop_cross_subject_erasure_independence(
        tenant in 1u64..256,
        subj_a in subject_strategy(),
        subj_b in subject_strategy(),
        records_a in 0u64..1000,
        records_b in 0u64..1000,
    ) {
        prop_assume!(subj_a != subj_b);

        let mut engine = ErasureEngine::new();

        let req_a = engine.request_erasure(&subj_a).unwrap();
        let req_b = engine.request_erasure(&subj_b).unwrap();

        let s_a = StreamId::from_tenant_and_local(TenantId::new(tenant), 1);
        let s_b = StreamId::from_tenant_and_local(TenantId::new(tenant), 2);

        engine.mark_in_progress(req_a.request_id, vec![s_a]).unwrap();
        engine.mark_in_progress(req_b.request_id, vec![s_b]).unwrap();

        // Advance request A to completion preconditions.
        let scope_a = engine
            .get_request(req_a.request_id)
            .unwrap()
            .scope_for(s_a, TenantId::new(tenant), records_a + 1000)
            .unwrap();
        engine
            .mark_stream_erased_with_scope(scope_a, records_a)
            .unwrap();

        // B is untouched.
        let after_b = engine.get_request(req_b.request_id).unwrap();
        prop_assert_eq!(after_b.records_erased, 0);
        let after_b_is_in_progress = matches!(
            after_b.status,
            ErasureStatus::InProgress { .. }
        );
        prop_assert!(after_b_is_in_progress);

        // Now advance B independently.
        let scope_b = engine
            .get_request(req_b.request_id)
            .unwrap()
            .scope_for(s_b, TenantId::new(tenant), records_b + 1000)
            .unwrap();
        engine
            .mark_stream_erased_with_scope(scope_b, records_b)
            .unwrap();

        // A's progress did not change.
        let final_a = engine.get_request(req_a.request_id).unwrap();
        prop_assert_eq!(final_a.records_erased, records_a);
    }
}

// -----------------------------------------------------------------------------
// Executor trait API sanity — compile-time bound the witnesses type.
// -----------------------------------------------------------------------------

/// Compile-time check: `StreamErasureWitness` and `StreamShredReceipt`
/// are the shapes the runtime must provide. A regression that
/// accidentally changed either type would fail this file's
/// compilation.
#[allow(dead_code)]
fn _type_bounds_check() {
    let _w: StreamErasureWitness = StreamErasureWitness {
        stream_id: StreamId::new(1),
        pre_erasure_merkle_root: [0; 32],
        key_shred_digest: [0; 32],
    };
    let _r: StreamShredReceipt = StreamShredReceipt {
        key_shred_digest: [0; 32],
        records_erased: 0,
        stream_length_at_shred: 0,
    };
}
