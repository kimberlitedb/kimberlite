"""Tests for :mod:`kimberlite.domain_errors`.

AUDIT-2026-04 S2.4 — covers the wire-code → domain-error mapping
and the ``as_result`` wrapper.
"""

import pytest

from kimberlite.domain_errors import (
    ConcurrentModification,
    Conflict,
    Err,
    Forbidden,
    NotFound,
    Ok,
    RateLimited,
    Timeout,
    Unavailable,
    Validation,
    as_result,
    map_kimberlite_error,
)
from kimberlite.errors import (
    AuthenticationError,
    ConnectionError as KmbConnectionError,
    KimberliteError,
    OffsetOutOfRangeError,
    StreamNotFoundError,
    TimeoutError as KmbTimeoutError,
    ClusterUnavailableError,
)


# Table of (kimberlite error → expected domain variant class).
_TABLE = [
    (StreamNotFoundError("gone", code=4), NotFound),
    (KimberliteError("no tenant", code=10), NotFound),
    (AuthenticationError("bad token", code=11), Forbidden),
    (KmbTimeoutError("slow server", code=12), Timeout),
    (ClusterUnavailableError("no leader", code=14), RateLimited),
    (OffsetOutOfRangeError("mismatch", code=7), ConcurrentModification),
    (KimberliteError("bad SQL", code=8), Validation),
    (KimberliteError("exec fail", code=9), Validation),
    (KimberliteError("bad class", code=6), Validation),
    (KimberliteError("boom", code=13), Unavailable),
    (KimberliteError("unknown", code=15), Unavailable),
]


@pytest.mark.parametrize("err,expected", _TABLE)
def test_map_kimberlite_error_wire_code_dispatch(err, expected):
    mapped = map_kimberlite_error(err)
    assert isinstance(mapped, expected), (
        f"{err.code} expected {expected.__name__}, got {type(mapped).__name__}"
    )


def test_validation_carries_message():
    err = KimberliteError("unexpected token", code=8)
    mapped = map_kimberlite_error(err)
    assert isinstance(mapped, Validation)
    assert mapped.message == "unexpected token"


def test_unavailable_carries_message_on_internal():
    err = KimberliteError("disk full", code=13)
    mapped = map_kimberlite_error(err)
    assert isinstance(mapped, Unavailable)
    assert "disk full" in mapped.message


def test_non_kimberlite_exception_maps_to_unavailable():
    mapped = map_kimberlite_error(ValueError("app bug"))
    assert isinstance(mapped, Unavailable)
    assert "app bug" in mapped.message


def test_primitive_thrown_value_maps_to_unavailable():
    mapped = map_kimberlite_error("string error")
    assert isinstance(mapped, Unavailable)


def test_as_result_ok_wraps_successful_callable():
    result = as_result(lambda: 42)
    assert isinstance(result, Ok)
    assert result.value == 42
    assert result.ok is True


def test_as_result_err_wraps_kimberlite_error():
    def boom():
        raise StreamNotFoundError("gone", code=4)

    result = as_result(boom)
    assert isinstance(result, Err)
    assert isinstance(result.err, NotFound)
    assert result.ok is False


def test_as_result_err_wraps_non_kimberlite_error():
    def boom():
        raise ValueError("app")

    result = as_result(boom)
    assert isinstance(result, Err)
    assert isinstance(result.err, Unavailable)
    assert "app" in result.err.message


def test_as_result_allows_pattern_matching():
    """Smoke test — the docstring's pattern-matching idiom actually
    works with the exported types."""

    def fake():
        raise KmbConnectionError("connection dropped", code=3)

    result = as_result(fake)
    if isinstance(result, Err):
        # Connection error → code=3 is not in the _CODE_RATE_LIMITED_LIKE
        # set (that's 14 = CLUSTER_UNAVAILABLE), so it falls through to
        # Unavailable via the generic catch-all path.
        assert isinstance(result.err, Unavailable)
    else:
        pytest.fail("expected Err")


def test_conflict_reasons_default_empty():
    c = Conflict()
    assert c.reasons == []


def test_conflict_reasons_accepted():
    c = Conflict(reasons=["stream foo exists"])
    assert c.reasons == ["stream foo exists"]
