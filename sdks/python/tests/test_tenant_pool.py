"""Tests for :mod:`kimberlite.tenant_pool`.

AUDIT-2026-04 S2.4 — uses a stub factory + controllable clock so
the LRU / idle eviction / dedup paths run without a live server.
"""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from typing import Optional

import pytest

from kimberlite.tenant_pool import TenantPool


@dataclass
class FakeClient:
    """Stand-in for :class:`kimberlite.Client`. Only needs
    ``disconnect()`` for the pool to treat it as a client."""

    tenant_id: int
    closed: bool = field(default=False)

    def disconnect(self) -> None:
        self.closed = True


class _Clock:
    """Monotonically-increasing test clock. Milliseconds."""

    def __init__(self) -> None:
        self.t = 0

    def __call__(self) -> int:
        return self.t


def _pool(max_size: int = 8, idle_timeout_ms: int = 0) -> tuple[TenantPool, list[FakeClient], _Clock]:
    clients: list[FakeClient] = []
    clock = _Clock()

    def factory(tid: int) -> FakeClient:
        c = FakeClient(tenant_id=tid)
        clients.append(c)
        return c  # type: ignore[return-value]

    pool = TenantPool(
        factory=factory,  # type: ignore[arg-type]
        max_size=max_size,
        idle_timeout_ms=idle_timeout_ms,
        now_ms=clock,
    )
    return pool, clients, clock


def test_acquire_creates_new_client_on_first_call():
    pool, clients, _ = _pool()
    c = pool.acquire(1)
    assert c.tenant_id == 1
    assert pool.stats().size == 1
    assert pool.stats().misses == 1
    assert pool.stats().hits == 0


def test_acquire_returns_cached_client_on_second_call():
    pool, clients, _ = _pool()
    a = pool.acquire(1)
    b = pool.acquire(1)
    assert a is b
    assert len(clients) == 1
    assert pool.stats().hits == 1


def test_separate_tenants_get_separate_clients():
    pool, clients, _ = _pool()
    a = pool.acquire(1)
    b = pool.acquire(2)
    assert a is not b
    assert pool.stats().size == 2


def test_lru_evicts_least_recently_used_when_full():
    pool, clients, clock = _pool(max_size=2, idle_timeout_ms=0)

    pool.acquire(1)
    clock.t = 10
    pool.acquire(2)
    clock.t = 20
    pool.acquire(1)  # touch tenant 1 → LRU = tenant 2

    clock.t = 30
    pool.acquire(3)  # evicts tenant 2

    by_tid = {c.tenant_id: c for c in clients}
    assert by_tid[2].closed is True
    assert by_tid[1].closed is False
    assert by_tid[3].closed is False
    assert pool.stats().size == 2
    assert pool.stats().evictions == 1


def test_idle_eviction_drops_stale_clients():
    pool, clients, clock = _pool(max_size=8, idle_timeout_ms=100)

    pool.acquire(1)       # t=0
    clock.t = 80
    pool.acquire(2)       # t=80 (inside idle window)

    clock.t = 150         # cutoff=50 → tenant 1 (t=0) evicted
    pool.acquire(3)

    by_tid = {c.tenant_id: c for c in clients}
    assert by_tid[1].closed is True
    assert by_tid[2].closed is False
    assert pool.stats().idle_evictions == 1


def test_idle_timeout_zero_disables_eviction():
    pool, clients, clock = _pool(max_size=8, idle_timeout_ms=0)
    pool.acquire(1)
    clock.t = 1_000_000
    pool.acquire(2)
    by_tid = {c.tenant_id: c for c in clients}
    assert by_tid[1].closed is False
    assert pool.stats().idle_evictions == 0


def test_with_client_runs_callable_with_acquired_client():
    pool, _, _ = _pool()
    got = pool.with_client(7, lambda c: c.tenant_id)
    assert got == 7


def test_close_disconnects_all_clients_and_resets_size():
    pool, clients, _ = _pool()
    pool.acquire(1)
    pool.acquire(2)
    pool.close()
    assert all(c.closed for c in clients)
    assert pool.stats().size == 0


def test_concurrent_acquires_for_same_tenant_dedupe():
    """Two threads hitting the same uncached tenant must share
    the single factory() call — only one slow connect fires.
    """

    factory_calls = 0

    def slow_factory(tid: int) -> FakeClient:
        nonlocal factory_calls
        factory_calls += 1
        # Sleep long enough that the second thread has a chance
        # to enter the pool and block on the inflight event
        # before the first thread's factory finishes. 50 ms is
        # comfortably larger than the 10 ms head-start sleep in
        # the test harness.
        time.sleep(0.05)
        return FakeClient(tenant_id=tid)

    pool = TenantPool(
        factory=slow_factory,  # type: ignore[arg-type]
        max_size=8,
        idle_timeout_ms=0,
    )

    results: list[FakeClient] = []
    errors: list[BaseException] = []

    def worker():
        try:
            results.append(pool.acquire(42))
        except BaseException as e:  # pragma: no cover
            errors.append(e)

    t1 = threading.Thread(target=worker)
    t2 = threading.Thread(target=worker)
    t1.start()
    # Head-start so t1 enters the pool first, reserves
    # tenant 42 as inflight, then enters slow_factory.
    time.sleep(0.01)
    t2.start()
    t1.join(timeout=2.0)
    t2.join(timeout=2.0)

    assert not errors, f"worker raised: {errors}"
    assert len(results) == 2
    assert results[0] is results[1]  # same cached client
    assert factory_calls == 1
