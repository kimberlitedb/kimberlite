"""Tests for the Python Pool wrapper.

These cover input validation and the wrapper's bookkeeping (idempotent
release, context-manager behaviour). They do NOT require a running server —
they instantiate a Pool against a free port that nothing's listening on and
verify that the pool surface is callable, and that config validation works.
"""

import pytest
from dataclasses import dataclass

from kimberlite import Pool, PoolStats, DataClass, Placement
from kimberlite.errors import KimberliteError


def test_pool_rejects_zero_max_size():
    with pytest.raises(ValueError, match="max_size"):
        Pool(address="127.0.0.1:1", tenant_id=1, max_size=0)


def test_pool_stats_shape():
    """PoolStats is a frozen dataclass with the expected fields."""
    s = PoolStats(max_size=10, open=2, idle=1, in_use=1, shutdown=False)
    assert s.max_size == 10
    assert s.open == 2
    assert s.idle == 1
    assert s.in_use == 1
    assert s.shutdown is False
    # Immutability
    with pytest.raises(Exception):
        s.max_size = 99  # type: ignore[misc]


def test_pool_context_manager_calls_shutdown():
    """The Pool context manager calls shutdown() on exit, idempotently."""
    pool = Pool(address="127.0.0.1:1", tenant_id=1, max_size=5)
    with pool:
        # stats() should work before shutdown.
        s = pool.stats()
        assert s.max_size == 5
    # After __exit__ the pool is closed — stats should now fail.
    with pytest.raises(KimberliteError, match="closed"):
        pool.stats()


def test_pool_shutdown_is_idempotent():
    pool = Pool(address="127.0.0.1:1", tenant_id=1, max_size=3)
    pool.shutdown()
    pool.shutdown()  # should not raise


def test_pool_acquire_after_shutdown_raises():
    pool = Pool(address="127.0.0.1:1", tenant_id=1, max_size=3)
    pool.shutdown()
    with pytest.raises(KimberliteError, match="closed"):
        pool.acquire()


def test_pool_accepts_placement_and_data_class_arguments():
    """Smoke-check that Placement and DataClass values pass through typing."""
    # These are enum values we'd use in create_stream; just verify they're
    # accessible via the top-level package.
    assert Placement.GLOBAL is not None
    assert DataClass.PUBLIC is not None


def test_pool_config_propagates_timeouts():
    """Pool constructor accepts acquire/idle timeout kwargs."""
    # This doesn't start a real connection — just verifies the constructor
    # accepts the kwargs without error.
    pool = Pool(
        address="127.0.0.1:1",
        tenant_id=1,
        max_size=4,
        acquire_timeout_ms=100,
        idle_timeout_ms=1000,
    )
    assert pool.max_size == 4
    pool.shutdown()
