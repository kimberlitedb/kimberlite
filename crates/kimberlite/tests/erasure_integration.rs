//! AUDIT-2026-04 C-1 integration test:
//! verify that `TenantHandle::erase_subject` (backed by
//! `KernelBackedErasureExecutor`) actually deletes the subject's rows
//! from the projection and produces a non-zero, non-deterministic
//! `key_shred_digest` proving cryptographic destruction.
//!
//! Workflow per test:
//!   1. open a fresh tenant + create a `phi` table with `subject_id`
//!   2. insert 10 rows for the same subject + 2 rows for an unrelated subject
//!   3. invoke `erase_subject(subject)` with a generated `AttestationKey`
//!   4. assert: the subject's rows are gone, the unrelated rows remain
//!   5. assert: the audit record's signed proof verifies and binds to a
//!      non-zero key-shred digest distinct from the pre-shred sentinel
//!
//! No mocks — the executor walks the real projection store and drives
//! real `Command::Delete` traffic through the kernel.

use kimberlite::{Kimberlite, TenantId, Value};
use kimberlite_compliance::erasure::AttestationKey;

const SUBJECT: &str = "patient-42@example.com";
const OTHER_SUBJECT: &str = "patient-99@example.com";
const SUBJECT_ROW_COUNT: usize = 10;
const OTHER_ROW_COUNT: usize = 2;

fn open_tenant_with_phi() -> (tempfile::TempDir, Kimberlite, kimberlite::TenantHandle) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    let tenant = db.tenant(TenantId::new(7));

    tenant
        .execute(
            "CREATE TABLE phi (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, payload TEXT)",
            &[],
        )
        .expect("create phi table");

    for i in 0..SUBJECT_ROW_COUNT {
        tenant
            .execute(
                "INSERT INTO phi (id, subject_id, payload) VALUES ($1, $2, $3)",
                &[
                    Value::BigInt(i as i64),
                    Value::Text(SUBJECT.into()),
                    Value::Text(format!("phi-record-{i}")),
                ],
            )
            .expect("insert phi row");
    }
    for i in 0..OTHER_ROW_COUNT {
        tenant
            .execute(
                "INSERT INTO phi (id, subject_id, payload) VALUES ($1, $2, $3)",
                &[
                    Value::BigInt(100 + i as i64),
                    Value::Text(OTHER_SUBJECT.into()),
                    Value::Text(format!("other-{i}")),
                ],
            )
            .expect("insert other row");
    }

    (dir, db, tenant)
}

#[test]
fn erase_subject_deletes_phi_rows_and_returns_signed_proof() {
    let (_dir, _db, tenant) = open_tenant_with_phi();
    let key = AttestationKey::generate();

    let pre = tenant
        .query(
            "SELECT id FROM phi WHERE subject_id = $1",
            &[Value::Text(SUBJECT.into())],
        )
        .expect("pre-erasure query");
    assert_eq!(
        pre.rows.len(),
        SUBJECT_ROW_COUNT,
        "precondition: 10 PHI rows for subject must be present"
    );

    let audit = tenant
        .erase_subject(SUBJECT, &key)
        .expect("erase_subject must succeed");

    // Subject rows: gone.
    let post = tenant
        .query(
            "SELECT id FROM phi WHERE subject_id = $1",
            &[Value::Text(SUBJECT.into())],
        )
        .expect("post-erasure query");
    assert!(
        post.rows.is_empty(),
        "no rows for the erased subject should remain; got {}",
        post.rows.len()
    );

    // Other subject's rows: untouched.
    let other = tenant
        .query(
            "SELECT id FROM phi WHERE subject_id = $1",
            &[Value::Text(OTHER_SUBJECT.into())],
        )
        .expect("post-erasure other-subject query");
    assert_eq!(
        other.rows.len(),
        OTHER_ROW_COUNT,
        "rows for unrelated subjects must not be erased"
    );

    // Audit record: signed proof verifies, key-shred digest non-zero
    // and distinct from the pre-shred sentinel ([0; 32]).
    assert_eq!(
        audit.records_erased, SUBJECT_ROW_COUNT as u64,
        "audit must report exactly the erased row count"
    );
    let proof = audit
        .signed_proof
        .as_ref()
        .expect("signed proof must be attached");
    proof
        .verify(&key.verifying_key())
        .expect("Ed25519 signature must verify under the attestation key");
    assert!(!proof.witnesses.is_empty(), "at least one stream witness");

    let pre_shred_sentinel = [0u8; 32];
    for w in &proof.witnesses {
        assert_ne!(
            w.key_shred_digest, pre_shred_sentinel,
            "key_shred_digest must commit to real key destruction"
        );
        // The pre-erasure root may legitimately be the sentinel for an
        // empty backing stream, but for our table with 10+ inserts the
        // chain head should also be non-zero.
        assert_ne!(
            w.pre_erasure_merkle_root, pre_shred_sentinel,
            "pre-erasure chain head must reflect a populated stream"
        );
    }
}

#[test]
fn erase_subject_with_no_tables_returns_clear_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    let tenant = db.tenant(TenantId::new(8));
    let key = AttestationKey::generate();

    let err = tenant
        .erase_subject("nobody@example.com", &key)
        .expect_err("erase_subject with no tables must fail clearly");
    let msg = err.to_string();
    assert!(
        msg.contains("no tables") && msg.contains("nothing to erase"),
        "error must explain the empty-tenant case; got: {msg}",
    );
}

#[test]
fn erase_subject_two_invocations_produce_distinct_key_shred_digests() {
    // The executor mints a *fresh* ephemeral DEK per shred call, so
    // even when the (stream_id, subject_id) tuple is identical the
    // resulting digest must change. Proves the digest is not a
    // deterministic function of the inputs alone — it actually
    // commits to destroyed key material.
    let (_dir1, _db1, tenant1) = open_tenant_with_phi();
    let (_dir2, _db2, tenant2) = open_tenant_with_phi();
    let key = AttestationKey::generate();

    let audit1 = tenant1.erase_subject(SUBJECT, &key).expect("erase 1");
    let audit2 = tenant2.erase_subject(SUBJECT, &key).expect("erase 2");

    let p1 = audit1.signed_proof.expect("proof 1");
    let p2 = audit2.signed_proof.expect("proof 2");

    let digests1: Vec<[u8; 32]> = p1.witnesses.iter().map(|w| w.key_shred_digest).collect();
    let digests2: Vec<[u8; 32]> = p2.witnesses.iter().map(|w| w.key_shred_digest).collect();
    assert_ne!(
        digests1, digests2,
        "fresh-DEK shreds must produce distinct digests across runs"
    );
}
