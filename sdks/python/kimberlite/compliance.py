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
import json
from dataclasses import dataclass
from typing import Any, List, Optional

from .admin import _call_admin  # Reuse the JSON-decoding helper.
from .ffi import _lib, KmbClient
from .errors import KimberliteError


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


class _ConsentNamespace:
    def __init__(self, handle: KmbClient) -> None:
        self._handle = handle

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

    def withdraw(self, consent_id: str) -> int:
        """Withdraw consent; returns the withdrawal timestamp in Unix nanos."""
        data = _call_admin(
            _lib.kmb_compliance_consent_withdraw,
            self._handle,
            consent_id.encode("utf-8"),
        )
        return int(data["withdrawn_at_nanos"])

    def check(self, subject_id: str, purpose: str) -> bool:
        data = _call_admin(
            _lib.kmb_compliance_consent_check,
            self._handle,
            subject_id.encode("utf-8"),
            purpose.encode("utf-8"),
        )
        return bool(data["is_valid"])

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

    def request(self, subject_id: str) -> ErasureRequest:
        data = _call_admin(
            _lib.kmb_compliance_erasure_request,
            self._handle,
            subject_id.encode("utf-8"),
        )
        return _parse_erasure_request(data)

    def status(self, request_id: str) -> ErasureRequest:
        data = _call_admin(
            _lib.kmb_compliance_erasure_status,
            self._handle,
            request_id.encode("utf-8"),
        )
        return _parse_erasure_request(data)

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

    def complete(self, request_id: str) -> ErasureAuditRecord:
        data = _call_admin(
            _lib.kmb_compliance_erasure_complete,
            self._handle,
            request_id.encode("utf-8"),
        )
        return _parse_erasure_audit(data)

    def exempt(self, request_id: str, basis: str) -> ErasureRequest:
        """Mark request as exempt. ``basis`` matches the ``ExemptionBasis`` variant."""
        data = _call_admin(
            _lib.kmb_compliance_erasure_exempt,
            self._handle,
            request_id.encode("utf-8"),
            basis.encode("utf-8"),
        )
        return _parse_erasure_request(data)

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


# --- Top-level namespace --------------------------------------------------


class ComplianceNamespace:
    """Compliance operations — GDPR consent + erasure."""

    def __init__(self, handle: KmbClient) -> None:
        self.consent = _ConsentNamespace(handle)
        self.erasure = _ErasureNamespace(handle)
