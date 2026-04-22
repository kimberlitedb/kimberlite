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
use kimberlite_types::DataClass;

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

    // v0.6.0 Tier 2 #8 — tag the backing stream as PHI so the
    // auto-discovery walk in `erase_subject` picks it up. CREATE
    // TABLE defaults data_class to `Public`; production callers flip
    // this via `tag_table_data_class` (or a dedicated `RetagStream`
    // DDL that's on ROADMAP v0.7.0).
    tenant
        .tag_table_data_class("phi", DataClass::PHI)
        .expect("tag phi table");

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
        msg.contains("no PHI/PII streams") && msg.contains("nothing to erase"),
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

// =============================================================================
// v0.6.0 Tier 2 #8 — auto-discovery + idempotence
// =============================================================================

/// **Auto-discovery doc-test.** `erase_subject` with no explicit stream
/// list walks the tenant catalog, filters to PHI/PII/Sensitive streams
/// with a `subject_id` column, and erases them. Verifies 3+ streams.
#[test]
fn erase_subject_auto_discovers_three_phi_pii_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open");
    let tenant = db.tenant(TenantId::new(12));

    // Three PHI/PII/Sensitive tables with subject_id columns.
    tenant
        .execute(
            "CREATE TABLE medical_records \
             (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, diagnosis TEXT)",
            &[],
        )
        .expect("create medical_records");
    tenant
        .tag_table_data_class("medical_records", DataClass::PHI)
        .expect("tag medical_records PHI");

    tenant
        .execute(
            "CREATE TABLE contact_info \
             (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, email TEXT)",
            &[],
        )
        .expect("create contact_info");
    tenant
        .tag_table_data_class("contact_info", DataClass::PII)
        .expect("tag contact_info PII");

    tenant
        .execute(
            "CREATE TABLE biometrics \
             (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, fingerprint TEXT)",
            &[],
        )
        .expect("create biometrics");
    tenant
        .tag_table_data_class("biometrics", DataClass::Sensitive)
        .expect("tag biometrics Sensitive");

    // A non-PHI/PII table that must NOT be auto-discovered, even
    // though it has a subject_id column.
    tenant
        .execute(
            "CREATE TABLE public_metrics \
             (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, score BIGINT)",
            &[],
        )
        .expect("create public_metrics");
    // Leave public_metrics at DataClass::Public (default).

    // A PHI table with NO subject_id column — must also be skipped
    // (no way to identify the subject's rows).
    tenant
        .execute(
            "CREATE TABLE anonymous_logs \
             (id BIGINT PRIMARY KEY, event_type TEXT)",
            &[],
        )
        .expect("create anonymous_logs");
    tenant
        .tag_table_data_class("anonymous_logs", DataClass::PHI)
        .expect("tag anonymous_logs PHI");

    // Insert the subject into the three PHI/PII streams + the
    // public_metrics table.
    let subject = "subject-auto-discovery";
    for (tbl, col_val) in [
        ("medical_records", "acute bronchitis"),
        ("contact_info", "subject@example.com"),
        ("biometrics", "fpr-0xDEADBEEF"),
        ("public_metrics", "999"),
    ] {
        for i in 0..3 {
            // Choose the correct third-column type: public_metrics
            // wants a BigInt.
            let third: Value = if tbl == "public_metrics" {
                Value::BigInt(col_val.parse().unwrap_or(0))
            } else {
                Value::Text(col_val.into())
            };
            tenant
                .execute(
                    &format!(
                        "INSERT INTO {tbl} (id, subject_id, {third_col}) VALUES ($1, $2, $3)",
                        third_col = match tbl {
                            "medical_records" => "diagnosis",
                            "contact_info" => "email",
                            "biometrics" => "fingerprint",
                            "public_metrics" => "score",
                            _ => unreachable!(),
                        }
                    ),
                    &[Value::BigInt(i), Value::Text(subject.into()), third],
                )
                .unwrap_or_else(|e| panic!("insert into {tbl}: {e}"));
        }
    }

    let key = AttestationKey::generate();
    let audit = tenant
        .erase_subject(subject, &key)
        .expect("auto-discovery erase must succeed");

    // Proof must cover exactly the three auto-discovered streams.
    let proof = audit.signed_proof.as_ref().expect("signed proof");
    assert_eq!(
        proof.witnesses.len(),
        3,
        "auto-discovery must yield 3 PHI/PII/Sensitive streams with subject_id columns; got {}",
        proof.witnesses.len()
    );
    assert_eq!(
        audit.records_erased, 9,
        "3 tables × 3 rows each = 9 records erased"
    );
    proof
        .verify(&key.verifying_key())
        .expect("attestation must verify");
    assert!(!audit.is_noop_replay);

    // Subject rows in public_metrics must survive — it's Public, not PHI/PII.
    let surviving = tenant
        .query(
            "SELECT id FROM public_metrics WHERE subject_id = $1",
            &[Value::Text(subject.into())],
        )
        .expect("post query");
    assert_eq!(
        surviving.rows.len(),
        3,
        "Public-tagged tables must NOT be auto-erased"
    );
}

/// **Override path preserved.** `erase_subject_with_streams` skips
/// auto-discovery entirely and uses the caller's explicit list.
#[test]
fn erase_subject_with_streams_override_bypasses_autodiscovery() {
    let (_dir, _db, tenant) = open_tenant_with_phi();
    let key = AttestationKey::generate();

    // Discover the phi table's stream_id and pass it explicitly.
    let streams = tenant
        .discover_phi_pii_streams()
        .expect("discover must succeed");
    assert_eq!(streams.len(), 1, "one PHI table in the fixture");

    let audit = tenant
        .erase_subject_with_streams(SUBJECT, streams, &key)
        .expect("override-path erase must succeed");
    let proof = audit.signed_proof.expect("signed proof");
    assert_eq!(proof.witnesses.len(), 1);
    assert_eq!(audit.records_erased, SUBJECT_ROW_COUNT as u64);
}

/// **Idempotence test.** A second `erase_subject` call for the same
/// subject returns a noop-replay audit record with the original
/// request_id and signed_proof verbatim — no new shred event.
#[test]
fn erase_subject_second_call_returns_noop_replay_receipt() {
    let (_dir, _db, tenant) = open_tenant_with_phi();
    let key = AttestationKey::generate();

    let first = tenant.erase_subject(SUBJECT, &key).expect("first erase");
    assert!(!first.is_noop_replay);
    let first_proof = first.signed_proof.clone().expect("first proof");

    let second = tenant.erase_subject(SUBJECT, &key).expect("second erase");
    assert!(
        second.is_noop_replay,
        "second call must be flagged as noop replay"
    );
    assert_eq!(
        second.request_id, first.request_id,
        "noop replay preserves the original request_id"
    );
    assert_eq!(
        second.records_erased, first.records_erased,
        "noop replay preserves the records_erased count"
    );
    assert_eq!(
        second.streams_affected, first.streams_affected,
        "noop replay preserves the streams_affected list"
    );
    let second_proof = second.signed_proof.expect("second proof");
    assert_eq!(
        second_proof, first_proof,
        "noop replay preserves the exact signed proof — same cryptographic commitment"
    );

    // No new rows were written to the underlying projection — the
    // noop replay should not have removed anything (there's nothing
    // to remove). Confirm the other-subject rows are still intact.
    let other = tenant
        .query(
            "SELECT id FROM phi WHERE subject_id = $1",
            &[Value::Text(OTHER_SUBJECT.into())],
        )
        .expect("post-noop query");
    assert_eq!(
        other.rows.len(),
        OTHER_ROW_COUNT,
        "noop replay must not touch unrelated subjects"
    );

    // Audit trail contains both records — the original and the noop.
    let trail = tenant
        .erasure_audit_trail()
        .expect("audit trail accessible");
    assert_eq!(trail.len(), 2, "trail holds original + noop");
    assert!(!trail[0].is_noop_replay);
    assert!(trail[1].is_noop_replay);
}
