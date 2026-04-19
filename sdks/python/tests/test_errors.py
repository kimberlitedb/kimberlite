"""Tests for error types."""

import pytest
from kimberlite.errors import (
    KimberliteError,
    ConnectionError,
    StreamNotFoundError,
    PermissionDeniedError,
    AuthenticationError,
    TimeoutError,
    OffsetOutOfRangeError,
    ClusterUnavailableError,
    ERROR_MAP,
)


def test_base_error():
    """Test base KimberliteError."""
    err = KimberliteError("test message", code=42)
    assert err.message == "test message"
    assert err.code == 42
    assert str(err) == "test message"


def test_connection_error():
    """Test ConnectionError inherits from KimberliteError."""
    err = ConnectionError("connection failed", code=3)
    assert isinstance(err, KimberliteError)
    assert err.message == "connection failed"
    assert err.code == 3


def test_error_map_coverage():
    """Test error map covers all error codes."""
    # Error codes 1-15 should be mapped
    for code in range(1, 16):
        assert code in ERROR_MAP, f"Error code {code} not mapped"

    # Code 0 (success) should not be in map
    assert 0 not in ERROR_MAP


def test_error_map_creates_correct_types():
    """Test error map creates correct exception types."""
    assert isinstance(ERROR_MAP[3]("test"), ConnectionError)
    assert isinstance(ERROR_MAP[4]("test"), StreamNotFoundError)
    assert isinstance(ERROR_MAP[5]("test"), PermissionDeniedError)
    assert isinstance(ERROR_MAP[11]("test"), AuthenticationError)
    assert isinstance(ERROR_MAP[12]("test"), TimeoutError)


# -----------------------------------------------------------------------
# Classification predicate tests — mirrors crates/kimberlite-client/src/
# error.rs. See S2.3 in
# docs-internal/audit/AUDIT-2026-04.md follow-up plan.
# -----------------------------------------------------------------------


# (code, expected: {predicate_name -> bool})
_PREDICATE_TABLE = [
    (
        3,  # ConnectionError
        {
            "is_retryable": True,
            "is_not_found": False,
            "is_auth_failed": False,
            "is_not_leader": False,
            "is_rate_limited": False,
            "is_offset_mismatch": False,
            "is_permission_denied": False,
        },
    ),
    (
        4,  # StreamNotFound
        {
            "is_retryable": False,
            "is_not_found": True,
            "is_auth_failed": False,
            "is_offset_mismatch": False,
        },
    ),
    (
        5,  # PermissionDenied
        {
            "is_retryable": False,
            "is_not_found": False,
            "is_permission_denied": True,
        },
    ),
    (
        7,  # OffsetOutOfRange (also surfaces OffsetMismatch)
        {
            "is_retryable": False,
            "is_offset_mismatch": True,
        },
    ),
    (
        10,  # TenantNotFound
        {
            "is_not_found": True,
            "is_retryable": False,
        },
    ),
    (
        11,  # AuthFailed
        {
            "is_auth_failed": True,
            "is_retryable": False,
        },
    ),
    (
        12,  # Timeout
        {
            "is_retryable": True,
            "is_not_found": False,
        },
    ),
    (
        14,  # ClusterUnavailable (covers NotLeader + RateLimited at FFI)
        {
            "is_retryable": True,
            "is_not_leader": True,
            "is_rate_limited": True,
        },
    ),
]


@pytest.mark.parametrize("code,expected", _PREDICATE_TABLE)
def test_error_predicates(code: int, expected: dict):
    """Classification predicates match Rust client's ClientError::is_* semantics."""
    err = ERROR_MAP[code]("test message")
    for predicate_name, expected_value in expected.items():
        actual = getattr(err, predicate_name)()
        assert actual is expected_value, (
            f"code={code}, {predicate_name}() expected {expected_value}, got {actual}"
        )


def test_predicates_return_false_when_code_missing():
    """Synthesised errors without a code never classify as any known shape."""
    err = KimberliteError("synthesised", code=None)
    assert err.is_retryable() is False
    assert err.is_not_found() is False
    assert err.is_auth_failed() is False
    assert err.is_not_leader() is False
    assert err.is_rate_limited() is False
    assert err.is_offset_mismatch() is False
    assert err.is_permission_denied() is False


def test_request_id_attached():
    """KimberliteError surfaces a request_id for correlation with server logs."""
    err = KimberliteError("boom", code=13, request_id=0xDEADBEEF)
    assert err.request_id == 0xDEADBEEF

    # Default is None when the caller doesn't supply one.
    err2 = KimberliteError("boom2", code=13)
    assert err2.request_id is None


def test_raise_for_error_code_propagates_request_id():
    """raise_for_error_code threads request_id onto the raised exception."""
    from kimberlite.errors import raise_for_error_code

    with pytest.raises(ConnectionError) as exc_info:
        raise_for_error_code(3, request_id=0xABCDEF)
    assert exc_info.value.request_id == 0xABCDEF
    assert exc_info.value.code == 3


def test_retryable_disjoint_from_terminal():
    """Cross-sanity: no single error is both retryable AND auth_failed.

    A retryable error is one the caller should back off and retry;
    an auth-failed error is terminal (re-auth required before any
    retry makes sense). Conflating them at the SDK layer would cause
    infinite retry loops on a revoked token.
    """
    for code in range(1, 16):
        err = ERROR_MAP[code]("test")
        if err.is_auth_failed():
            assert not err.is_retryable(), (
                f"code={code} is BOTH auth_failed and retryable — "
                "this would loop forever on a revoked token"
            )
