"""Tests for error types."""

import pytest
from kimberlite.errors import (
    KimberliteError,
    ConnectionError,
    StreamNotFoundError,
    PermissionDeniedError,
    AuthenticationError,
    TimeoutError,
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
