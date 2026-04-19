"""Auto-reconnect tests for Client.

AUDIT-2026-04 S2.2 — mirrors the TypeScript SDK's auto-reconnect
behaviour in the Python client. These tests use the
``_invoke_with_reconnect`` primitive directly so they can exercise
the retry path without a live Kimberlite server.
"""

import pytest

from kimberlite.client import Client
from kimberlite.errors import ConnectionError as KimberliteConnectionError
from kimberlite.errors import KimberliteError


def _make_client_with_fake_config() -> Client:
    """Build a Client directly from a fake handle + dummy config.

    Used by tests that only exercise the reconnect helper — no FFI
    work happens. The FFI handle is never called because we stub
    ``_connect_native`` on the class.
    """
    # Use a NULL-valued ctypes pointer so `__del__` can call
    # kmb_client_disconnect without a type error — the native side
    # treats NULL as a no-op.
    import ctypes as _ctypes

    fake_handle = _ctypes.c_void_p(0)
    config = {
        "addresses": ["localhost:5432"],
        "tenant_id": 1,
        "auth_token": None,
        "client_name": "kimberlite-python",
        "client_version": "0.1.0",
    }
    return Client(handle=fake_handle, connect_config=config, auto_reconnect=True)


def test_reconnect_count_starts_at_zero():
    c = _make_client_with_fake_config()
    assert c.reconnect_count == 0


def test_reconnect_without_config_raises():
    # Constructed without config — reconnect cannot rebuild a handle.
    import ctypes as _ctypes

    fake_handle = _ctypes.c_void_p(0)
    c = Client(handle=fake_handle, connect_config=None, auto_reconnect=True)
    with pytest.raises(KimberliteError) as exc_info:
        c.reconnect()
    assert "saved config" in str(exc_info.value)


def test_reconnect_increments_count_and_swaps_handle(monkeypatch):
    """A manual `reconnect()` call replaces the native handle and
    bumps the counter."""

    c = _make_client_with_fake_config()
    original_handle = c._handle

    # Stub `Client._connect_native` so no FFI call happens. Return a
    # distinct sentinel so we can assert the handle was replaced.
    new_handle_sentinel = object()
    monkeypatch.setattr(
        Client, "_connect_native", staticmethod(lambda cfg: new_handle_sentinel)
    )
    # Also stub the disconnect of the old handle so we don't touch
    # the FFI for the fake original handle.
    import kimberlite.ffi as ffi_module

    monkeypatch.setattr(
        ffi_module._lib, "kmb_client_disconnect", lambda h: None
    )

    c.reconnect()

    assert c.reconnect_count == 1
    assert c._handle is new_handle_sentinel
    assert c._handle is not original_handle


def test_reconnect_failure_preserves_old_handle(monkeypatch):
    """If opening a new handle fails, the existing connection is
    preserved (not torn down)."""

    c = _make_client_with_fake_config()
    original_handle = c._handle

    def failing_connect(_cfg):
        raise KimberliteConnectionError("simulated connect failure", code=3)

    monkeypatch.setattr(Client, "_connect_native", staticmethod(failing_connect))

    with pytest.raises(KimberliteConnectionError):
        c.reconnect()

    # Old handle is still live; reconnect_count did not advance.
    assert c._handle is original_handle
    assert c.reconnect_count == 0


def test_invoke_with_reconnect_retries_once_on_connection_error(monkeypatch):
    """A `ConnectionError` on the first call triggers one reconnect
    + retry. The second attempt's result is returned to the caller.
    """

    c = _make_client_with_fake_config()
    import ctypes as _ctypes

    monkeypatch.setattr(
        Client,
        "_connect_native",
        staticmethod(lambda cfg: _ctypes.c_void_p(0)),
    )
    import kimberlite.ffi as ffi_module

    monkeypatch.setattr(
        ffi_module._lib, "kmb_client_disconnect", lambda h: None
    )

    call_count = {"n": 0}

    def flaky_fn():
        call_count["n"] += 1
        if call_count["n"] == 1:
            raise KimberliteConnectionError("broken pipe", code=3)
        return "recovered"

    result = c._invoke_with_reconnect(flaky_fn)
    assert result == "recovered"
    assert call_count["n"] == 2
    assert c.reconnect_count == 1


def test_invoke_with_reconnect_respects_auto_reconnect_false():
    """When ``auto_reconnect=False``, the connection error is
    propagated verbatim without a retry or reconnect attempt."""

    import ctypes as _ctypes

    fake_handle = _ctypes.c_void_p(0)
    config = {
        "addresses": ["localhost:5432"],
        "tenant_id": 1,
        "auth_token": None,
        "client_name": "kimberlite-python",
        "client_version": "0.1.0",
    }
    c = Client(handle=fake_handle, connect_config=config, auto_reconnect=False)

    def always_fails():
        raise KimberliteConnectionError("server down", code=3)

    with pytest.raises(KimberliteConnectionError):
        c._invoke_with_reconnect(always_fails)
    # Counter stays at 0 — no reconnect was attempted.
    assert c.reconnect_count == 0


def test_invoke_with_reconnect_propagates_second_error(monkeypatch):
    """If the retry itself fails (with any error, including a second
    ConnectionError), the second error is surfaced — we do NOT loop."""

    c = _make_client_with_fake_config()
    import ctypes as _ctypes

    monkeypatch.setattr(
        Client,
        "_connect_native",
        staticmethod(lambda cfg: _ctypes.c_void_p(0)),
    )
    import kimberlite.ffi as ffi_module

    monkeypatch.setattr(
        ffi_module._lib, "kmb_client_disconnect", lambda h: None
    )

    call_count = {"n": 0}

    def always_fails():
        call_count["n"] += 1
        raise KimberliteConnectionError(
            f"fail attempt {call_count['n']}", code=3
        )

    with pytest.raises(KimberliteConnectionError) as exc_info:
        c._invoke_with_reconnect(always_fails)
    # Exactly two attempts — the first + one retry. No loop.
    assert call_count["n"] == 2
    assert "attempt 2" in str(exc_info.value)
    # Reconnect counter advanced once because reconnect succeeded
    # before the retry fn failed.
    assert c.reconnect_count == 1


def test_invoke_with_reconnect_passes_through_non_connection_errors():
    """Non-connection errors don't trigger reconnect — they bubble up
    immediately."""

    c = _make_client_with_fake_config()

    def non_connection_error():
        raise KimberliteError("query syntax error", code=8)

    with pytest.raises(KimberliteError) as exc_info:
        c._invoke_with_reconnect(non_connection_error)
    assert "query syntax" in str(exc_info.value)
    # Counter stays at 0 — we never touched reconnect.
    assert c.reconnect_count == 0
