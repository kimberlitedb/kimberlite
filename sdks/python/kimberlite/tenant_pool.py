"""Per-tenant :class:`Client` cache.

AUDIT-2026-04 S2.4 — lifts notebar's LRU-per-tenant adapter out
of ``packages/kimberlite-client/src/adapter.ts`` into the SDK so
every multi-tenant Python app (FastAPI, Django, Flask) gets the
same pattern.

Typical FastAPI use — one pool per process, ``.acquire(tenant_id)``
in each handler::

    from kimberlite import Client
    from kimberlite.tenant_pool import TenantPool

    pool = TenantPool(
        factory=lambda tid: Client.connect(
            addresses=["localhost:5432"], tenant_id=tid, auth_token=TOKEN,
        ),
        max_size=128,
        idle_timeout_ms=5 * 60_000,
    )

    @app.get("/charts/{patient_id}")
    def chart(patient_id: int, tenant_id: int = Depends(current_tenant)):
        client = pool.acquire(tenant_id)
        return client.query("SELECT ...")
"""

from __future__ import annotations

import threading
import time
from collections import OrderedDict
from dataclasses import dataclass
from typing import Callable, Optional

from .client import Client


@dataclass(frozen=True)
class TenantPoolStats:
    """Runtime stats snapshot. Useful for dashboards + tests."""

    size: int
    hits: int
    misses: int
    evictions: int
    idle_evictions: int


class TenantPool:
    """LRU-per-tenant :class:`Client` cache.

    Thread-safe: all mutations hold an internal lock so concurrent
    FastAPI handlers can call :meth:`acquire` without racing.
    Concurrent acquires for the same ``tenant_id`` deduplicate —
    only one ``factory()`` call fires.
    """

    def __init__(
        self,
        factory: Callable[[int], Client],
        max_size: int = 128,
        idle_timeout_ms: int = 5 * 60_000,
        *,
        now_ms: Callable[[], int] = lambda: int(time.time() * 1000),
    ) -> None:
        """Create a new pool.

        Args:
            factory: Callable that opens a fresh :class:`Client`
                bound to the given ``tenant_id``. Typically
                ``lambda tid: Client.connect(addresses, tid, ...)``.
            max_size: Maximum concurrent cached tenants. LRU-evicted
                above this.
            idle_timeout_ms: Idle-timeout in milliseconds. Clients
                untouched for this long are disconnected + evicted on
                the next :meth:`acquire` call. ``0`` disables idle
                eviction.
            now_ms: Injectable clock (for deterministic tests). Returns
                milliseconds.
        """
        self._factory = factory
        self._max_size = max_size
        self._idle_timeout_ms = idle_timeout_ms
        self._now_ms = now_ms
        # OrderedDict preserves insertion order + O(1) move-to-end
        # for LRU tracking.
        self._cache: "OrderedDict[int, tuple[Client, int]]" = OrderedDict()
        self._lock = threading.RLock()
        self._inflight: dict[int, threading.Event] = {}
        self._hits = 0
        self._misses = 0
        self._evictions = 0
        self._idle_evictions = 0

    def acquire(self, tenant_id: int) -> Client:
        """Return the cached :class:`Client` for ``tenant_id``,
        creating one via the factory if absent.

        Updates the LRU recency stamp. Concurrent calls for the same
        ``tenant_id`` block until the first acquirer finishes
        connecting.
        """
        with self._lock:
            self._expire_idle()
            entry = self._cache.get(tenant_id)
            if entry is not None:
                client, _ = entry
                now = self._now_ms()
                self._cache.move_to_end(tenant_id)
                self._cache[tenant_id] = (client, now)
                self._hits += 1
                return client

            # Inflight dedup: if another caller is already
            # connecting for this tenant, wait for them.
            inflight_event = self._inflight.get(tenant_id)
            if inflight_event is not None:
                self._hits += 1
                # Release lock while waiting — the other caller
                # holds it to insert the result.
                self._lock.release()
                try:
                    inflight_event.wait()
                finally:
                    self._lock.acquire()
                entry = self._cache.get(tenant_id)
                if entry is not None:
                    self._cache.move_to_end(tenant_id)
                    return entry[0]
                # Fall through to re-create if the inflight acquirer
                # failed. Rare — typically the inflight insert
                # succeeds and we return above.

            self._misses += 1
            event = threading.Event()
            self._inflight[tenant_id] = event

        # Drop the lock while the factory runs — it may do network
        # I/O. The inflight event guarantees dedup.
        try:
            client = self._factory(tenant_id)
        except Exception:
            with self._lock:
                self._inflight.pop(tenant_id, None)
            event.set()
            raise

        with self._lock:
            self._evict_if_full()
            self._cache[tenant_id] = (client, self._now_ms())
            self._inflight.pop(tenant_id, None)
        event.set()
        return client

    def with_client(
        self, tenant_id: int, fn: Callable[[Client], object]
    ) -> object:
        """Convenience — :meth:`acquire` + execute. The client is
        never surfaced to the caller past ``fn``'s scope.
        """
        client = self.acquire(tenant_id)
        return fn(client)

    def close(self) -> None:
        """Drop all cached clients, disconnecting each one.

        Subsequent :meth:`acquire` calls reconnect via the factory.
        """
        with self._lock:
            to_close = list(self._cache.values())
            self._cache.clear()
        for client, _ in to_close:
            try:
                client.disconnect()
            except Exception:  # pragma: no cover — best-effort
                pass

    def stats(self) -> TenantPoolStats:
        """Runtime stats snapshot."""
        with self._lock:
            return TenantPoolStats(
                size=len(self._cache),
                hits=self._hits,
                misses=self._misses,
                evictions=self._evictions,
                idle_evictions=self._idle_evictions,
            )

    # ------------------------------------------------------------------

    def _evict_if_full(self) -> None:
        """Evict the least-recently-used entry when the cache is
        at or above ``max_size``. Caller must hold ``self._lock``.
        """
        while len(self._cache) >= self._max_size:
            # popitem(last=False) removes the OLDEST entry —
            # head of the ordered dict.
            oldest_id, (client, _) = self._cache.popitem(last=False)
            try:
                client.disconnect()
            except Exception:  # pragma: no cover
                pass
            self._evictions += 1

    def _expire_idle(self) -> None:
        """Evict entries whose ``last_used_at`` is older than the
        idle cutoff. Caller must hold ``self._lock``.
        """
        if self._idle_timeout_ms == 0:
            return
        cutoff = self._now_ms() - self._idle_timeout_ms
        stale = [
            tid for tid, (_, last) in self._cache.items() if last < cutoff
        ]
        for tid in stale:
            client, _ = self._cache.pop(tid)
            try:
                client.disconnect()
            except Exception:  # pragma: no cover
                pass
            self._idle_evictions += 1
