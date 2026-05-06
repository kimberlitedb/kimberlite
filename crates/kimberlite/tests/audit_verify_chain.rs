//! Integration tests for the v0.8.0 `audit_log_verify_chain` helper.
//!
//! Mirrors the underlying `ComplianceAuditLog::verify_chain` tests
//! but exercises the `TenantHandle` surface that the wire handler
//! calls through to.

use kimberlite::{Kimberlite, TenantId};
use kimberlite_compliance::audit::{Actor, ComplianceAuditAction};

fn open() -> (tempfile::TempDir, Kimberlite, kimberlite::TenantHandle) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Kimberlite::open(dir.path()).expect("open db");
    let tenant = db.tenant(TenantId::new(0xA1));
    (dir, db, tenant)
}

#[test]
fn verify_chain_returns_zero_head_for_empty_log() {
    let (_dir, _db, tenant) = open();
    let (count, head) = tenant
        .audit_log_verify_chain()
        .expect("empty log must verify");
    assert_eq!(count, 0);
    assert_eq!(head, [0u8; 32]);
}

#[test]
fn verify_chain_returns_event_count_and_head_after_appends() {
    let (_dir, _db, tenant) = open();

    for i in 0..3 {
        tenant
            .audit_log_append_with_actor(
                ComplianceAuditAction::ConsentGranted {
                    subject_id: format!("subject-{i}"),
                    purpose: "Marketing".into(),
                    scope: "*".into(),
                    terms_version: None,
                    accepted: true,
                },
                Actor::Authenticated(format!("alice-{i}")),
            )
            .expect("append");
    }

    let (count, head) = tenant
        .audit_log_verify_chain()
        .expect("chain of 3 must verify");
    assert_eq!(count, 3);
    assert_ne!(head, [0u8; 32], "head must advance after appends");
}

#[test]
fn chain_head_hex_format() {
    let (_dir, _db, tenant) = open();
    // Empty-log head is "00" * 32.
    assert_eq!(tenant.audit_log_chain_head_hex(), "0".repeat(64));

    tenant
        .audit_log_append_with_actor(
            ComplianceAuditAction::ConsentWithdrawn {
                subject_id: "alice".into(),
                consent_id: uuid::Uuid::new_v4(),
            },
            Actor::Authenticated("ops".into()),
        )
        .expect("append");

    let head_hex = tenant.audit_log_chain_head_hex();
    assert_eq!(head_hex.len(), 64, "SHA-256 hex must be 64 chars");
    assert!(
        head_hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "hex must be lowercase: got {head_hex}"
    );
    assert_ne!(head_hex, "0".repeat(64), "head must advance");
}
