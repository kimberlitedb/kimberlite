"""Tests for FFI bindings."""

import pytest
from kimberlite.ffi import _lib, KMB_OK, _check_error
from kimberlite.errors import KimberliteError


def test_error_message():
    """Test kmb_error_message returns valid string."""
    msg = _lib.kmb_error_message(KMB_OK)
    assert msg is not None
    assert b"Success" in msg


def test_error_is_retryable():
    """Test kmb_error_is_retryable."""
    # Timeout should be retryable
    assert _lib.kmb_error_is_retryable(12) == 1

    # Connection error should not be retryable
    assert _lib.kmb_error_is_retryable(3) == 0


def test_check_error_success():
    """Test _check_error with success code."""
    # Should not raise
    _check_error(KMB_OK)


def test_check_error_failure():
    """Test _check_error with error code."""
    with pytest.raises(KimberliteError):
        _check_error(1)  # NULL pointer error
