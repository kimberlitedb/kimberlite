"""Tests for :class:`kimberlite.compliance.AuditReport`.

Exercises the Markdown renderer without needing a live server —
constructs the report directly.
"""

from __future__ import annotations

from kimberlite.compliance import AuditEvent, AuditReport


def _event(kind: str, actor: str | None, ts: int = 100) -> AuditEvent:
    return AuditEvent(
        event_id=f"e-{kind}",
        timestamp_nanos=ts,
        action=kind,
        subject_id=None,
        actor=actor,
        tenant_id=1,
        ip_address=None,
        correlation_id=None,
        request_id=None,
        reason=None,
        source_country=None,
        changed_field_names=[],
    )


def test_to_markdown_includes_header_and_totals():
    r = AuditReport(
        from_nanos=100,
        to_nanos=200,
        subject_id="alice",
        total_events=3,
        by_action_kind={"ConsentGranted": 2, "ErasureCompleted": 1},
        by_actor={"admin@example.com": 3},
        events=[],
    )
    md = r.to_markdown()
    assert "# Compliance Audit Report" in md
    assert "Window: `100` → `200`" in md
    assert "Subject: `alice`" in md
    assert "Total events: **3**" in md


def test_to_markdown_omits_subject_line_when_none():
    r = AuditReport(
        from_nanos=0,
        to_nanos=0,
        subject_id=None,
        total_events=0,
        by_action_kind={},
        by_actor={},
        events=[],
    )
    md = r.to_markdown()
    assert "Subject:" not in md


def test_to_markdown_sorts_action_kinds_alphabetically():
    # Regulator-facing output should be stable-ordered.
    r = AuditReport(
        from_nanos=0,
        to_nanos=0,
        subject_id=None,
        total_events=3,
        by_action_kind={"Zeta": 1, "Alpha": 1, "Mu": 1},
        by_actor={},
        events=[],
    )
    md = r.to_markdown()
    # Alpha should appear before Mu before Zeta.
    assert md.index("Alpha") < md.index("Mu") < md.index("Zeta")


def test_audit_event_dataclass_preserves_optional_fields():
    e = _event("ConsentGranted", None)
    assert e.actor is None
    e2 = _event("ErasureCompleted", "admin@example.com")
    assert e2.actor == "admin@example.com"
