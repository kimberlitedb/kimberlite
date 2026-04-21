//! ROADMAP v0.5.0 item C — end-to-end integration test for Phase 6
//! compliance endpoints:
//!
//!   * `audit_query`        — retrieve hash-chained audit events with filters
//!   * `export_subject`     — GDPR Article 20 portability export (subject's data)
//!   * `verify_export`      — recompute SHA-256 of returned body, compare
//!   * `breach_report_indicator` — HIPAA §164.404 / GDPR Art. 33 indicator
//!   * `breach_query_status` — fetch the report for a detected breach
//!   * `breach_confirm`     — transition Detected → Confirmed, stamp notification
//!   * `breach_resolve`     — close the breach with a remediation note
//!
//! Every call goes over the binary wire protocol against a real
//! in-process server (no mocks). Validates that the server-side
//! handlers landed in v0.5.0 replace the prior `InternalError` stubs.

use std::net::SocketAddr;

use base64::Engine;
use kimberlite_client::Client;
use kimberlite_test_harness::TestKimberlite;
use kimberlite_types::DataClass;
use kimberlite_wire::{BreachIndicatorPayload, BreachStatusTag, ExportFormat, QueryParam};

const TENANT: u64 = 42_001;
const SUBJECT: &str = "patient-mrn-phase6-42";
const OTHER_SUBJECT: &str = "patient-mrn-phase6-99";

/// ROADMAP v0.5.1 — this thin shim keeps per-test call-sites stable
/// (`TestServer::start().addr`) while delegating spin-up + shutdown to
/// the shared `kimberlite-test-harness` crate. The previous hand-
/// rolled `TestServer` is preserved in git history — prior to v0.5.1
/// every e2e file had its own copy of the same ~60-line struct +
/// Drop.
struct TestServer {
    addr: SocketAddr,
    _harness: TestKimberlite,
}

impl TestServer {
    fn start() -> Self {
        let harness = TestKimberlite::builder()
            .tenant(TENANT)
            .build()
            .expect("harness build");
        Self {
            addr: harness.addr(),
            _harness: harness,
        }
    }
}

fn sync_client(addr: SocketAddr) -> Client {
    Client::connect(
        addr,
        kimberlite_types::TenantId::new(TENANT),
        kimberlite_client::ClientConfig::default(),
    )
    .expect("sync connect")
}

fn seed_phi(client: &mut Client) {
    // Create table + insert both SUBJECT rows and an unrelated control subject.
    client
        .execute(
            "CREATE TABLE phi (id BIGINT PRIMARY KEY, subject_id TEXT NOT NULL, note TEXT)",
            &[],
        )
        .expect("create phi");
    for i in 0..5u64 {
        client
            .execute(
                "INSERT INTO phi (id, subject_id, note) VALUES ($1, $2, $3)",
                &[
                    QueryParam::BigInt(i as i64),
                    QueryParam::Text(SUBJECT.into()),
                    QueryParam::Text(format!("note-{i}")),
                ],
            )
            .expect("insert subject row");
    }
    for i in 0..2u64 {
        client
            .execute(
                "INSERT INTO phi (id, subject_id, note) VALUES ($1, $2, $3)",
                &[
                    QueryParam::BigInt(100 + i as i64),
                    QueryParam::Text(OTHER_SUBJECT.into()),
                    QueryParam::Text(format!("other-{i}")),
                ],
            )
            .expect("insert control row");
    }
}

#[test]
fn phase6_export_subject_and_verify_roundtrip() {
    let server = TestServer::start();
    let mut client = sync_client(server.addr);
    seed_phi(&mut client);

    // Export the subject's PHI as JSON.
    let export = client
        .export_subject(SUBJECT, "operator-ops", ExportFormat::Json, vec![], 0)
        .expect("export_subject must succeed post-v0.5.0 (handler no longer stubbed)");

    assert_eq!(export.subject_id, SUBJECT);
    assert_eq!(export.requester_id, "operator-ops");
    assert_eq!(
        export.record_count, 5,
        "export should include exactly the 5 subject rows, not the 2 control rows"
    );
    assert!(!export.body_base64.is_empty(), "body must be populated");
    assert!(
        !export.content_hash_hex.is_empty(),
        "content hash must be set"
    );

    // Verify: recompute hash from returned body.
    let v = client
        .verify_export(&export.export_id, &export.body_base64)
        .expect("verify_export must succeed");
    assert!(v.valid, "recomputed hash must match recorded content_hash");

    // Tampered body → valid = false.
    let tampered = base64::engine::general_purpose::STANDARD.encode(b"tampered bytes");
    let v = client
        .verify_export(&export.export_id, &tampered)
        .expect("verify_export still succeeds at the wire layer");
    assert!(!v.valid, "tampered body must not verify");
}

#[test]
fn phase6_breach_full_workflow_indicator_query_confirm_resolve() {
    let server = TestServer::start();
    let mut client = sync_client(server.addr);

    // Report an indicator large enough to cross the mass-export threshold.
    let event = client
        .breach_report_indicator(BreachIndicatorPayload::MassDataExport {
            records: 500_000,
            data_classes: vec![DataClass::PHI],
        })
        .expect("breach_report_indicator succeeds post-v0.5.0");
    let event = event.expect("500k PHI records crosses mass-export threshold");
    let event_id = event.event_id.clone();

    // Query status — should be Detected (the initial state).
    let report = client
        .breach_query_status(&event_id)
        .expect("breach_query_status succeeds");
    assert!(
        matches!(
            report.event.status,
            BreachStatusTag::Detected | BreachStatusTag::UnderInvestigation,
        ),
        "fresh breach should be Detected or UnderInvestigation, got {:?}",
        report.event.status,
    );

    // Confirm → 72-hour notification deadline flow. Needs UnderInvestigation
    // first; escalate directly isn't on the wire surface so call query_status
    // again which doesn't transition — confirm should still work if the
    // native BreachDetector accepts Detected → Confirmed. If it doesn't,
    // this call surfaces an error we want the test to catch.
    let confirm_response = client
        .breach_confirm(&event_id)
        .map_err(|_| "confirm_breach should accept Detected → Confirmed today")
        .expect("breach_confirm must succeed");
    assert!(
        confirm_response.notification_sent_at_nanos > 0,
        "notification timestamp must be populated"
    );

    // Resolve with a remediation note.
    let resolved = client
        .breach_resolve(
            &event_id,
            "Rotated compromised API keys; enabled export rate limit",
        )
        .expect("breach_resolve succeeds");
    assert!(
        resolved.resolved_at_nanos > 0,
        "resolved timestamp must be populated"
    );

    // Final query — status should reflect Resolved.
    let final_report = client
        .breach_query_status(&event_id)
        .expect("query after resolve");
    assert!(
        matches!(final_report.event.status, BreachStatusTag::Resolved { .. }),
        "post-resolve status must be Resolved, got {:?}",
        final_report.event.status,
    );
}

#[test]
fn phase6_audit_query_returns_phase6_events() {
    let server = TestServer::start();
    let mut client = sync_client(server.addr);
    seed_phi(&mut client);

    // Drive an export so the audit log gets a Phase 6 entry.
    let _ = client
        .export_subject(SUBJECT, "operator-audit", ExportFormat::Json, vec![], 0)
        .expect("export to seed audit log");

    // Query the audit log with no filter — must return >=1 event.
    let events = client
        .audit_query(None, None, None, None, None, Some(100))
        .expect("audit_query must succeed post-v0.5.0 (handler no longer stubbed)");
    assert!(
        !events.is_empty(),
        "audit log must contain at least one event after driving an export"
    );
    assert!(
        events.iter().any(|e| e.action == "DataExported"),
        "audit log should include the DataExported event; saw: {:?}",
        events.iter().map(|e| &e.action).collect::<Vec<_>>(),
    );
}

#[test]
fn phase6_export_nonexistent_subject_returns_clean_error() {
    let server = TestServer::start();
    let mut client = sync_client(server.addr);
    seed_phi(&mut client);

    let err = client
        .export_subject(
            "subject-that-does-not-exist",
            "operator",
            ExportFormat::Json,
            vec![],
            0,
        )
        .expect_err("export of nonexistent subject must surface an error, not a zero-row success");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("no data") || msg.to_lowercase().contains("not found"),
        "error should indicate no data found, got: {msg}",
    );
}

#[test]
fn phase6_breach_query_nonexistent_returns_breach_not_found() {
    let server = TestServer::start();
    let mut client = sync_client(server.addr);

    let bogus = uuid::Uuid::new_v4().to_string();
    let err = client
        .breach_query_status(&bogus)
        .expect_err("unknown breach id must surface an error");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("breach") || msg.to_lowercase().contains("not found"),
        "error should mention missing breach, got: {msg}",
    );
}
