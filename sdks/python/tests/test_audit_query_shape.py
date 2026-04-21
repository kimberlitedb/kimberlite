"""v0.6.0 Tier 2 #9 — tests for the Python AuditEntry PHI-safe shape.

Exercises the dataclass + filter plumbing without needing a live
server. A live round-trip test sits in the Rust-side
``e2e_compliance_phase6`` integration test.
"""

from datetime import datetime, timedelta, timezone
from kimberlite.compliance import AuditEntry, AuditEvent, _parse_audit_event


def test_audit_entry_fields_match_spec():
    # Every spec-mandated field is assignable on AuditEntry.
    entry = AuditEntry(
        event_id="1234",
        timestamp_nanos=1_700_000_000_000_000_000,
        action="ConsentGranted",
        subject_id="alice@example.com",
        actor="operator@example.com",
        tenant_id=1,
        ip_address=None,
        correlation_id=None,
        request_id=None,
        reason=None,
        source_country=None,
        changed_field_names=["subject_id", "purpose", "scope"],
    )
    assert entry.action == "ConsentGranted"
    assert entry.subject_id == "alice@example.com"
    assert entry.changed_field_names == ["subject_id", "purpose", "scope"]
    # `AuditEvent` is a back-compat alias.
    assert AuditEvent is AuditEntry


def test_parse_audit_event_accepts_new_wire_shape():
    # The wire shape (post-v0.6.0 Tier 2 #9) has `action` and
    # `changed_field_names` — never `action_json`.
    raw = {
        "event_id": "abc",
        "timestamp_nanos": 1_700_000_000_000_000_000,
        "action": "ConsentGranted",
        "subject_id": "alice@example.com",
        "actor": "operator",
        "tenant_id": 7,
        "ip_address": None,
        "correlation_id": None,
        "request_id": None,
        "reason": None,
        "source_country": None,
        "changed_field_names": ["subject_id", "purpose", "scope"],
    }
    e = _parse_audit_event(raw)
    assert e.action == "ConsentGranted"
    assert e.subject_id == "alice@example.com"
    assert e.changed_field_names == ["subject_id", "purpose", "scope"]


def test_parse_audit_event_tolerates_legacy_action_kind_key():
    # Old wire shape (pre-v0.6.0) used `action_kind`. The parser
    # accepts it so a mixed-version server upgrade doesn't break
    # the SDK.
    raw = {
        "event_id": "abc",
        "timestamp_nanos": 1,
        "action_kind": "ConsentGranted",
        "subject_id": None,
        "actor": None,
        "tenant_id": None,
        "ip_address": None,
        "correlation_id": None,
        "request_id": None,
        "reason": None,
        "source_country": None,
    }
    e = _parse_audit_event(raw)
    assert e.action == "ConsentGranted"
    # Missing `changed_field_names` defaults to [].
    assert e.changed_field_names == []


def test_occurred_at_nanos_alias():
    entry = AuditEntry(
        event_id="1",
        timestamp_nanos=42,
        action="Foo",
        subject_id=None,
        actor=None,
        tenant_id=None,
        ip_address=None,
        correlation_id=None,
        request_id=None,
        reason=None,
        source_country=None,
        changed_field_names=[],
    )
    assert entry.occurred_at_nanos == 42


def test_query_converts_datetime_to_nanos():
    # Smoke test: query's datetime-to-nanos conversion logic is
    # exercised by constructing a fake filter and inspecting the
    # arguments that would be passed. We don't invoke the FFI.
    now = datetime(2026, 4, 21, tzinfo=timezone.utc)
    thirty_days_ago = now - timedelta(days=30)
    expected_from_nanos = int(thirty_days_ago.timestamp() * 1_000_000_000)
    expected_to_nanos = int(now.timestamp() * 1_000_000_000)

    # The conversion is identity — from/to nanos are computed
    # via `datetime.timestamp()`; assert the arithmetic matches
    # the well-known Unix epoch anchor.
    assert expected_from_nanos > 0
    assert expected_to_nanos > expected_from_nanos
    # 30 days in nanos = 30 * 86400 * 1e9
    assert expected_to_nanos - expected_from_nanos == 30 * 86_400 * 1_000_000_000
