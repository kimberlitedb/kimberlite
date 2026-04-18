"""Connection pool for Kimberlite.

Holds up to ``max_size`` live TCP connections. Callers ``acquire()`` a
:class:`PooledClient` — use it as a context manager for automatic release:

    >>> with Pool(address="127.0.0.1:5432", tenant_id=1) as pool:
    ...     with pool.acquire() as client:
    ...         result = client.query("SELECT 1")
"""

from __future__ import annotations

import ctypes
import threading
from dataclasses import dataclass
from types import TracebackType
from typing import Any, Callable, List, Optional, Type, TypeVar

from .client import Client, ExecuteResult, QueryResult
from .ffi import (
    _check_error,
    _lib,
    KmbClient,
    KmbPool,
    KmbPoolConfig,
    KmbPooledClient,
)
from .types import DataClass, Offset, Placement, StreamId, TenantId
from .value import Value
from .errors import KimberliteError

T = TypeVar("T")


@dataclass(frozen=True)
class PoolStats:
    """Snapshot of pool utilisation."""

    max_size: int
    open: int
    idle: int
    in_use: int
    shutdown: bool


class Pool:
    """A thread-safe pool of :class:`Client` connections.

    Connections are created lazily; the first :meth:`acquire` triggers the
    first TCP connect. Pools are safe to share across threads. Use as a
    context manager to guarantee ``shutdown()`` on exit.

    Args:
        address: Server address as ``"host:port"``.
        tenant_id: Tenant identifier.
        auth_token: Optional bearer token (JWT or API key).
        max_size: Hard cap on concurrent connections (default ``10``).
        acquire_timeout_ms: Max wait before :meth:`acquire` raises
            ``TimeoutError``; ``0`` blocks forever (default ``30_000``).
        idle_timeout_ms: Idle eviction threshold in ms; ``0`` disables
            (default ``300_000``).
    """

    def __init__(
        self,
        address: str,
        tenant_id: int,
        *,
        auth_token: Optional[str] = None,
        max_size: int = 10,
        acquire_timeout_ms: int = 30_000,
        idle_timeout_ms: int = 300_000,
    ) -> None:
        if max_size <= 0:
            raise ValueError("max_size must be > 0")

        addr_bytes = address.encode("utf-8")
        # Keep the C-string buffer alive for the lifetime of the pool.
        self._addr_buf = ctypes.c_char_p(addr_bytes)
        addr_ptrs = (ctypes.c_char_p * 1)(self._addr_buf)

        config = KmbPoolConfig(
            addresses=ctypes.cast(addr_ptrs, ctypes.POINTER(ctypes.c_char_p)),
            address_count=1,
            tenant_id=tenant_id,
            auth_token=auth_token.encode("utf-8") if auth_token else None,
            max_size=max_size,
            acquire_timeout_ms=acquire_timeout_ms,
            idle_timeout_ms=idle_timeout_ms,
        )

        handle = KmbPool()
        err = _lib.kmb_pool_create(ctypes.byref(config), ctypes.byref(handle))
        _check_error(err)

        self._handle: Optional[KmbPool] = handle
        self._closed = False
        self._lock = threading.RLock()

    @property
    def max_size(self) -> int:
        return self.stats().max_size

    def acquire(self) -> "PooledClient":
        """Acquire a client from the pool. Blocks up to ``acquire_timeout_ms``.

        Raises:
            KimberliteError: If the pool is shut down or the acquire times out.
        """
        with self._lock:
            if self._closed or not self._handle:
                raise KimberliteError("Pool is closed")

            pooled = KmbPooledClient()
            err = _lib.kmb_pool_acquire(self._handle, ctypes.byref(pooled))
            _check_error(err)

            return PooledClient(pooled, pool=self)

    def with_client(self, fn: Callable[["PooledClient"], T]) -> T:
        """Run ``fn`` with a checked-out client; always releases the client."""
        client = self.acquire()
        try:
            return fn(client)
        finally:
            client.release()

    def stats(self) -> PoolStats:
        """Return current pool utilisation."""
        with self._lock:
            if self._closed or not self._handle:
                raise KimberliteError("Pool is closed")

            max_size = ctypes.c_size_t()
            open_ = ctypes.c_size_t()
            idle = ctypes.c_size_t()
            in_use = ctypes.c_size_t()
            shutdown = ctypes.c_int()
            err = _lib.kmb_pool_stats(
                self._handle,
                ctypes.byref(max_size),
                ctypes.byref(open_),
                ctypes.byref(idle),
                ctypes.byref(in_use),
                ctypes.byref(shutdown),
            )
            _check_error(err)
            return PoolStats(
                max_size=int(max_size.value),
                open=int(open_.value),
                idle=int(idle.value),
                in_use=int(in_use.value),
                shutdown=bool(shutdown.value),
            )

    def shutdown(self) -> None:
        """Shut the pool down. Idempotent. Use as a last step before drop."""
        with self._lock:
            if self._closed or not self._handle:
                return
            _lib.kmb_pool_destroy(self._handle)
            self._handle = None
            self._closed = True

    def __enter__(self) -> "Pool":
        return self

    def __exit__(
        self,
        exc_type: Optional[type],
        exc_val: Optional[BaseException],
        exc_tb: Optional[TracebackType],
    ) -> None:
        self.shutdown()

    def __del__(self) -> None:
        # Best-effort cleanup on GC.
        try:
            self.shutdown()
        except Exception:
            pass


class PooledClient:
    """Pool-borrowed client. Mirrors :class:`Client` but belongs to a pool.

    Use as a context manager for guaranteed release:

        >>> with pool.acquire() as client:
        ...     client.query("SELECT 1")
    """

    def __init__(self, handle: KmbPooledClient, pool: Pool) -> None:
        self._handle: Optional[KmbPooledClient] = handle
        self._pool = pool
        self._released = False
        self._lock = threading.RLock()

    def release(self) -> None:
        """Return the connection to the pool. Idempotent."""
        with self._lock:
            if not self._released and self._handle is not None:
                _lib.kmb_pool_release(self._handle)
                self._handle = None
                self._released = True

    def discard(self) -> None:
        """Close the underlying connection instead of returning it to the pool.

        Use after an unrecoverable protocol error.
        """
        with self._lock:
            if not self._released and self._handle is not None:
                _lib.kmb_pool_discard(self._handle)
                self._handle = None
                self._released = True

    def __enter__(self) -> "PooledClient":
        return self

    def __exit__(
        self,
        exc_type: Optional[type],
        exc_val: Optional[BaseException],
        exc_tb: Optional[TracebackType],
    ) -> None:
        self.release()

    def __del__(self) -> None:
        try:
            self.release()
        except Exception:
            pass

    # ----- Delegated operations ------------------------------------------
    # PooledClient exposes the same public surface as Client. Each call
    # converts the pooled handle to a borrowed KmbClient* and reuses the
    # existing FFI entry points.

    def _borrowed_client(self) -> KmbClient:
        with self._lock:
            if self._released or self._handle is None:
                raise KimberliteError("PooledClient has been released")
            client_ptr = _lib.kmb_pooled_client_as_client(self._handle)
            if not client_ptr:
                raise KimberliteError("Pool handed out an invalid client pointer")
            return KmbClient(client_ptr)

    def _as_client(self) -> Client:
        """Wrap the borrowed pointer in a non-owning Client shim."""
        return _BorrowedClient(self._borrowed_client())

    @property
    def tenant_id(self) -> TenantId:
        return self._as_client().tenant_id

    @property
    def last_request_id(self) -> Optional[int]:
        return self._as_client().last_request_id

    def create_stream(
        self,
        name: str,
        data_class: DataClass,
        placement: Placement = Placement.GLOBAL,
        custom_region: Optional[str] = None,
    ) -> StreamId:
        return self._as_client().create_stream(
            name, data_class, placement=placement, custom_region=custom_region
        )

    def append(
        self,
        stream_id: StreamId,
        events: List[bytes],
        expected_offset: Offset = Offset(0),
    ) -> Offset:
        return self._as_client().append(stream_id, events, expected_offset)

    def read(
        self,
        stream_id: StreamId,
        from_offset: Offset = Offset(0),
        max_bytes: int = 1024 * 1024,
    ) -> List[Any]:
        return self._as_client().read(stream_id, from_offset, max_bytes)

    def query(self, sql: str, params: Optional[List[Value]] = None) -> QueryResult:
        return self._as_client().query(sql, params)

    def query_at(
        self, sql: str, params: Optional[List[Value]], position: Offset
    ) -> QueryResult:
        return self._as_client().query_at(sql, params, position)

    def execute(
        self, sql: str, params: Optional[List[Value]] = None
    ) -> ExecuteResult:
        return self._as_client().execute(sql, params)

    def query_rows(
        self,
        sql: str,
        params: Optional[List[Value]],
        mapper: Callable[[List[Value], List[str]], T],
    ) -> List[T]:
        return self._as_client().query_rows(sql, params, mapper)

    def query_model(
        self,
        sql: str,
        params: Optional[List[Value]],
        model: Type[T],
    ) -> List[T]:
        return self._as_client().query_model(sql, params, model)


class _BorrowedClient(Client):
    """A non-owning Client that does NOT disconnect on drop.

    The pool owns the lifecycle; this shim lets us reuse every
    ``Client.*`` method without accidentally calling
    ``kmb_client_disconnect`` when the ``PooledClient`` is released.
    """

    def __init__(self, handle: KmbClient) -> None:
        # Skip Client.__init__ to avoid allocating a fresh RLock; we don't
        # own this handle and mustn't disconnect.
        self._handle = handle
        self._closed = False
        self._lock = threading.RLock()

    def disconnect(self) -> None:  # type: ignore[override]
        # No-op: the pool owns the connection.
        pass

    def __del__(self) -> None:  # type: ignore[override]
        # No-op: prevent Client.__del__ from calling disconnect.
        pass
