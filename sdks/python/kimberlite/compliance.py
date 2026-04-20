"""Compliance namespace — GDPR consent + erasure.

Accessed via ``client.compliance``:

    >>> client.compliance.consent.grant("alice", "Marketing")
    >>> client.compliance.consent.check("alice", "Marketing")
    True
    >>> req = client.compliance.erasure.request("alice")
    >>> client.compliance.erasure.complete(req.request_id)
"""

from __future__ import annotations

import ctypes
import functools
import json
from dataclasses import dataclass
from typing import Any, Callable, List, Optional, TypeVar, cast

from .admin import _call_admin  # Reuse the JSON-decoding helper.
from .audit_context import _ffi_audit_attached
from .ffi import _lib, KmbClient
from .errors import KimberliteError

F = TypeVar("F", bound=Callable[..., Any])


def _with_audit(fn: F) -> F:
    """Attach the caller's audit context to the FFI thread-local for
    the duration of `fn`. See :func:`kimberlite.client._with_audit`."""

    @functools.wraps(fn)
    def _wrapped(*args: Any, **kwargs: Any) -> Any:
        with _ffi_audit_attached():
            return fn(*args, **kwargs)

    return cast(F, _wrapped)


# --- Consent types --------------------------------------------------------


@dataclass(frozen=True)
class ConsentRecord:
    consent_id: str
    subject_id: str
    purpose: str
    scope: str
    granted_at_nanos: int
    withdrawn_at_nanos: Optional[int]
    expires_at_nanos: Optional[int]
    notes: Optional[str]


@dataclass(frozen=True)
class ConsentGrantResult:
    consent_id: str
    granted_at_nanos: int


# --- Erasure types --------------------------------------------------------


@dataclass(frozen=True)
class ErasureStatus:
    """Serialised form of ``ErasureStatusTag``.

    The ``kind`` field is one of ``Pending | InProgress | Complete | Failed | Exempt``.
    Other fields are populated only for the relevant variant.
    """

    kind: str
    streams_remaining: Optional[int] = None
    erased_at_nanos: Optional[int] = None
    total_records: Optional[int] = None
    reason: Optional[str] = None
    retry_at_nanos: Optional[int] = None
    basis: Optional[str] = None


@dataclass(frozen=True)
class ErasureRequest:
    request_id: str
    subject_id: str
    requested_at_nanos: int
    deadline_nanos: int
    status: ErasureStatus
    records_erased: int
    streams_affected: List[int]


@dataclass(frozen=True)
class ErasureAuditRecord:
    request_id: str
    subject_id: str
    requested_at_nanos: int
    completed_at_nanos: int
    records_erased: int
    streams_affected: List[int]
    erasure_proof_hex: Optional[str]


# --- Consent sub-namespace ------------------------------------------------


# --- AUDIT-2026-04 S4.3 typed erasure tokens -----------------------------


@dataclass(frozen=True)
class ErasurePending:
    """Erasure request in the 'Pending' state. Callers must call
    :meth:`_ErasureNamespace.mark_progress_typed` before recording
    per-stream progress."""

    _inner: "ErasureRequest"

    @property
    def request_id(self) -> str:
        return self._inner.request_id


@dataclass(frozen=True)
class ErasureInProgress:
    """Erasure request in the 'InProgress' state."""

    _inner: "ErasureRequest"

    @property
    def request_id(self) -> str:
        return self._inner.request_id


@dataclass(frozen=True)
class ErasureRecording:
    """Erasure request with per-stream progress being recorded."""

    _inner: "ErasureRequest"

    @property
    def request_id(self) -> str:
        return self._inner.request_id


class _ConsentNamespace:
    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    @_with_audit
    def grant(self, subject_id: str, purpose: str) -> ConsentGrantResult:
        """Grant consent. ``purpose`` matches the ``ConsentPurpose`` variant name."""
        data = _call_admin(
            _lib.kmb_compliance_consent_grant,
            self._handle,
            subject_id.encode("utf-8"),
            purpose.encode("utf-8"),
        )
        return ConsentGrantResult(
            consent_id=data["consent_id"],
            granted_at_nanos=int(data["granted_at_nanos"]),
        )

    @_with_audit
    def withdraw(self, consent_id: str) -> int:
        """Withdraw consent; returns the withdrawal timestamp in Unix nanos."""
        data = _call_admin(
            _lib.kmb_compliance_consent_withdraw,
            self._handle,
            consent_id.encode("utf-8"),
        )
        return int(data["withdrawn_at_nanos"])

    @_with_audit
    def check(self, subject_id: str, purpose: str) -> bool:
        data = _call_admin(
            _lib.kmb_compliance_consent_check,
            self._handle,
            subject_id.encode("utf-8"),
            purpose.encode("utf-8"),
        )
        return bool(data["is_valid"])

    @_with_audit
    def list(self, subject_id: str, valid_only: bool = False) -> List[ConsentRecord]:
        data = _call_admin(
            _lib.kmb_compliance_consent_list,
            self._handle,
            subject_id.encode("utf-8"),
            1 if valid_only else 0,
        )
        return [_parse_consent_record(r) for r in data.get("consents", [])]


def _parse_consent_record(raw: dict) -> ConsentRecord:
    return ConsentRecord(
        consent_id=raw["consent_id"],
        subject_id=raw["subject_id"],
        purpose=raw["purpose"],
        scope=raw["scope"],
        granted_at_nanos=int(raw["granted_at_nanos"]),
        withdrawn_at_nanos=raw.get("withdrawn_at_nanos"),
        expires_at_nanos=raw.get("expires_at_nanos"),
        notes=raw.get("notes"),
    )


# --- Erasure sub-namespace ------------------------------------------------


class _ErasureNamespace:
    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    @_with_audit
    def request(self, subject_id: str) -> ErasureRequest:
        data = _call_admin(
            _lib.kmb_compliance_erasure_request,
            self._handle,
            subject_id.encode("utf-8"),
        )
        return _parse_erasure_request(data)

    @_with_audit
    def status(self, request_id: str) -> ErasureRequest:
        data = _call_admin(
            _lib.kmb_compliance_erasure_status,
            self._handle,
            request_id.encode("utf-8"),
        )
        return _parse_erasure_request(data)

    @_with_audit
    def mark_stream_erased(
        self,
        request_id: str,
        stream_id: int,
        records_erased: int,
    ) -> ErasureRequest:
        """Record per-stream progress on an in-flight erasure request.

        Mirrors :js:meth:`compliance.erasure.markStreamErased` in the
        TypeScript SDK. Call once per affected stream between
        :meth:`request` and :meth:`complete`.

        Args:
            request_id: UUID string returned by :meth:`request`.
            stream_id: 64-bit stream handle that was erased.
            records_erased: Number of records erased on this stream.

        Returns:
            The updated :class:`ErasureRequest` reflecting progress
            (``records_erased`` running total, ``streams_remaining``
            decremented in the ``InProgress`` status).
        """
        data = _call_admin(
            _lib.kmb_compliance_erasure_mark_stream_erased,
            self._handle,
            request_id.encode("utf-8"),
            ctypes.c_uint64(stream_id),
            ctypes.c_uint64(records_erased),
        )
        return _parse_erasure_request(data)

    @_with_audit
    def complete(self, request_id: str) -> ErasureAuditRecord:
        data = _call_admin(
            _lib.kmb_compliance_erasure_complete,
            self._handle,
            request_id.encode("utf-8"),
        )
        return _parse_erasure_audit(data)

    @_with_audit
    def exempt(self, request_id: str, basis: str) -> ErasureRequest:
        """Mark request as exempt. ``basis`` matches the ``ExemptionBasis`` variant."""
        data = _call_admin(
            _lib.kmb_compliance_erasure_exempt,
            self._handle,
            request_id.encode("utf-8"),
            basis.encode("utf-8"),
        )
        return _parse_erasure_request(data)

    # --- AUDIT-2026-04 S4.3 typed state-machine surface ------------------

    def request_typed(self, subject_id: str) -> "ErasurePending":
        req = self.request(subject_id)
        return ErasurePending(_inner=req)

    def mark_progress_typed(
        self,
        token: "ErasurePending",
        stream_ids: List[int],
    ) -> "ErasureInProgress":
        # mark_progress lives on the TS surface today; in Python the
        # transition is implicit via per-stream mark_stream_erased.
        # We keep the typed surface so callers can express intent.
        _ = stream_ids
        return ErasureInProgress(_inner=token._inner)

    def mark_stream_erased_typed(
        self,
        token: "ErasureInProgress | ErasureRecording",
        stream_id: int,
        records_erased: int,
    ) -> "ErasureRecording":
        updated = self.mark_stream_erased(
            token._inner.request_id,
            stream_id,
            records_erased,
        )
        return ErasureRecording(_inner=updated)

    def complete_typed(
        self,
        token: "ErasureInProgress | ErasureRecording",
    ) -> ErasureAuditRecord:
        return self.complete(token._inner.request_id)

    @_with_audit
    def erase_subject(
        self,
        subject_id: str,
        on_stream: Optional[Any] = None,
    ) -> ErasureAuditRecord:
        """AUDIT-2026-04 S4.4 — one-call orchestrator. Opens the
        erasure, walks every affected stream (optionally invoking
        ``on_stream(stream_id)`` to do the actual redaction, which
        must return the records-erased count), and completes."""
        pending = self.request_typed(subject_id)
        in_progress = self.mark_progress_typed(
            pending, list(pending._inner.streams_affected)
        )
        recording: Any = in_progress
        for sid in pending._inner.streams_affected:
            erased = on_stream(sid) if on_stream else 0
            recording = self.mark_stream_erased_typed(recording, sid, erased)
        return self.complete_typed(recording)

    @_with_audit
    def list(self) -> List[ErasureAuditRecord]:
        data = _call_admin(_lib.kmb_compliance_erasure_list, self._handle)
        return [_parse_erasure_audit(a) for a in data.get("audit", [])]


def _parse_erasure_request(raw: dict) -> ErasureRequest:
    status = raw["status"]
    fields = status.get("fields", {}) or {}
    status_obj = ErasureStatus(
        kind=status["kind"],
        streams_remaining=fields.get("streams_remaining"),
        erased_at_nanos=fields.get("erased_at_nanos"),
        total_records=fields.get("total_records"),
        reason=fields.get("reason"),
        retry_at_nanos=fields.get("retry_at_nanos"),
        basis=fields.get("basis"),
    )
    return ErasureRequest(
        request_id=raw["request_id"],
        subject_id=raw["subject_id"],
        requested_at_nanos=int(raw["requested_at_nanos"]),
        deadline_nanos=int(raw["deadline_nanos"]),
        status=status_obj,
        records_erased=int(raw["records_erased"]),
        streams_affected=[int(s) for s in raw.get("streams_affected", [])],
    )


def _parse_erasure_audit(raw: dict) -> ErasureAuditRecord:
    return ErasureAuditRecord(
        request_id=raw["request_id"],
        subject_id=raw["subject_id"],
        requested_at_nanos=int(raw["requested_at_nanos"]),
        completed_at_nanos=int(raw["completed_at_nanos"]),
        records_erased=int(raw["records_erased"]),
        streams_affected=[int(s) for s in raw.get("streams_affected", [])],
        erasure_proof_hex=raw.get("erasure_proof_hex"),
    )


# --- Audit-log query types (AUDIT-2026-04 S3.6) --------------------------


@dataclass(frozen=True)
class AuditEvent:
    """A single compliance audit event.

    Mirrors the server's ``AuditEventInfo`` wire shape. Every
    field is optional-null-safe. ``action_json`` is a JSON string
    the caller can further parse for action-specific fields.
    """

    event_id: str
    timestamp_nanos: int
    action_kind: str
    action_json: str
    actor: Optional[str]
    tenant_id: Optional[int]
    ip_address: Optional[str]
    correlation_id: Optional[str]
    source_country: Optional[str]


def _parse_audit_event(raw: dict) -> AuditEvent:
    return AuditEvent(
        event_id=raw["event_id"],
        timestamp_nanos=int(raw["timestamp_nanos"]),
        action_kind=raw["action_kind"],
        action_json=raw["action_json"],
        actor=raw.get("actor"),
        tenant_id=raw.get("tenant_id"),
        ip_address=raw.get("ip_address"),
        correlation_id=raw.get("correlation_id"),
        source_country=raw.get("source_country"),
    )


@dataclass(frozen=True)
class PortabilityExport:
    """GDPR Article 20 portability export result."""

    export_id: str
    subject_id: str
    requester_id: str
    requested_at_nanos: int
    completed_at_nanos: int
    format: str  # "Json" | "Csv"
    streams_included: List[int]
    record_count: int
    content_hash_hex: str
    signature_hex: Optional[str]
    body_base64: str


def _parse_portability_export(raw: dict) -> PortabilityExport:
    return PortabilityExport(
        export_id=raw["export_id"],
        subject_id=raw["subject_id"],
        requester_id=raw["requester_id"],
        requested_at_nanos=int(raw["requested_at_nanos"]),
        completed_at_nanos=int(raw["completed_at_nanos"]),
        format=raw["format"],
        streams_included=[int(s) for s in raw.get("streams_included", [])],
        record_count=int(raw["record_count"]),
        content_hash_hex=raw["content_hash_hex"],
        signature_hex=raw.get("signature_hex"),
        body_base64=raw["body_base64"],
    )


class _AuditNamespace:
    """Query the compliance audit log.

    AUDIT-2026-04 S3.6 — mirrors the TS ``client.compliance.audit``
    and Rust ``client.compliance().audit()`` sub-namespaces.
    """

    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    def generate_report(
        self,
        from_nanos: int,
        to_nanos: int,
        subject_id: Optional[str] = None,
    ) -> "AuditReport":
        """AUDIT-2026-04 S3.6 — generate a structured audit
        report over a time window.

        Wraps :meth:`query` and pre-aggregates counts by
        ``action_kind`` and ``actor``. See
        :class:`AuditReport.to_markdown` for a regulator-friendly
        renderer. Mirrors the Rust SDK's
        ``client.compliance().audit().generate_report(...)``.
        """
        events = self.query(
            subject_id=subject_id,
            time_from_nanos=from_nanos,
            time_to_nanos=to_nanos,
        )
        by_action_kind: "dict[str, int]" = {}
        by_actor: "dict[str, int]" = {}
        for e in events:
            by_action_kind[e.action_kind] = by_action_kind.get(e.action_kind, 0) + 1
            if e.actor is not None:
                by_actor[e.actor] = by_actor.get(e.actor, 0) + 1
        return AuditReport(
            from_nanos=from_nanos,
            to_nanos=to_nanos,
            subject_id=subject_id,
            total_events=len(events),
            by_action_kind=by_action_kind,
            by_actor=by_actor,
            events=events,
        )

    def query(
        self,
        *,
        subject_id: Optional[str] = None,
        action_type: Optional[str] = None,
        time_from_nanos: Optional[int] = None,
        time_to_nanos: Optional[int] = None,
        actor: Optional[str] = None,
        limit: Optional[int] = None,
    ) -> List[AuditEvent]:
        """Query audit events with optional filters.

        Any omitted filter is unconstrained. Returns events in
        server-defined order (typically by timestamp descending).
        """
        out = KmbAdminJson()
        err = _lib.kmb_compliance_audit_query(
            self._handle,
            subject_id.encode("utf-8") if subject_id else None,
            action_type.encode("utf-8") if action_type else None,
            ctypes.c_uint64(time_from_nanos or 0),
            ctypes.c_uint64(time_to_nanos or 0),
            actor.encode("utf-8") if actor else None,
            ctypes.c_uint32(limit or 0),
            ctypes.byref(out),
        )
        from .errors import raise_for_error_code

        raise_for_error_code(err)
        try:
            s = ctypes.string_at(out.json).decode("utf-8")
        finally:
            _lib.kmb_admin_json_free(ctypes.byref(out))
        data = json.loads(s)
        return [_parse_audit_event(e) for e in data.get("events", [])]


@dataclass(frozen=True)
class AuditReport:
    """Structured compliance audit report.

    AUDIT-2026-04 S3.6 — produced by
    :meth:`_AuditNamespace.generate_report` as a regulator-ready
    summary. ``by_action_kind`` and ``by_actor`` are pre-aggregated
    counts; ``events`` retains the raw wire events for detail
    rendering.
    """

    from_nanos: int
    to_nanos: int
    subject_id: Optional[str]
    total_events: int
    by_action_kind: "dict[str, int]"
    by_actor: "dict[str, int]"
    events: List[AuditEvent]

    def to_markdown(self) -> str:
        """Render the report as a regulator-friendly Markdown string.

        Mirrors the Rust SDK's ``AuditReport::to_markdown`` so
        cross-language reports are byte-identical modulo
        dict-ordering (both use sorted keys).
        """
        lines: list[str] = [
            "# Compliance Audit Report",
            "",
            f"- Window: `{self.from_nanos}` → `{self.to_nanos}` (Unix ns)",
        ]
        if self.subject_id is not None:
            lines.append(f"- Subject: `{self.subject_id}`")
        lines.extend([
            f"- Total events: **{self.total_events}**",
            "",
            "## Events by action kind",
        ])
        for kind in sorted(self.by_action_kind):
            lines.append(f"- `{kind}`: {self.by_action_kind[kind]}")
        lines.extend(["", "## Events by actor"])
        for actor in sorted(self.by_actor):
            lines.append(f"- `{actor}`: {self.by_actor[actor]}")
        return "\n".join(lines) + "\n"


class _ExportNamespace:
    """GDPR Article 20 portability exports."""

    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

    def for_subject(
        self,
        subject_id: str,
        requester_id: str,
        *,
        format: str = "Json",
        stream_ids: Optional[List[int]] = None,
        max_records_per_stream: int = 0,
    ) -> PortabilityExport:
        """Produce a signed portability export for a subject.

        Args:
            subject_id: The data subject.
            requester_id: Who requested the export — appears in
                the audit trail.
            format: ``"Json"`` (default) or ``"Csv"``.
            stream_ids: Specific stream IDs to include, or ``None``
                for "every stream the caller can see".
            max_records_per_stream: Per-stream cap. ``0`` uses the
                server's default (bounded to prevent memory blowup).
        """
        out = KmbAdminJson()
        stream_ids_json: Optional[bytes] = None
        if stream_ids is not None:
            stream_ids_json = json.dumps(stream_ids).encode("utf-8")
        err = _lib.kmb_compliance_export_subject(
            self._handle,
            subject_id.encode("utf-8"),
            requester_id.encode("utf-8"),
            format.encode("utf-8"),
            stream_ids_json,
            ctypes.c_uint64(max_records_per_stream),
            ctypes.byref(out),
        )
        from .errors import raise_for_error_code

        raise_for_error_code(err)
        try:
            s = ctypes.string_at(out.json).decode("utf-8")
        finally:
            _lib.kmb_admin_json_free(ctypes.byref(out))
        return _parse_portability_export(json.loads(s))


# --- Top-level namespace --------------------------------------------------


class ComplianceNamespace:
    """Compliance operations — GDPR consent + erasure + audit + export."""

    def __init__(self, handle: KmbClient) -> None:
        self.consent = _ConsentNamespace(handle)
        self.erasure = _ErasureNamespace(handle)
        self.audit = _AuditNamespace(handle)
        self.export = _ExportNamespace(handle)
