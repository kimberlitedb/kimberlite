"""Asyncio wrapper over the sync ``Client``.

The Python FFI client is synchronous — every call blocks the Python thread.
This module wraps each method in ``asyncio.to_thread`` so callers running
under an event loop (FastAPI, aiohttp, quart, etc.) can ``await`` the SDK
without blocking their event loop.

**Note**: This is a convenience layer, not a concurrency boost. Each
request still executes serially on the underlying sync client because the
FFI handle is not thread-safe. If you need concurrent requests, use
:class:`kimberlite.Pool` and acquire one pooled client per task.

Example:

    import asyncio
    from kimberlite.aio import AsyncClient
    from kimberlite import DataClass

    async def main():
        client = await AsyncClient.connect(
            addresses=["localhost:5432"], tenant_id=1
        )
        try:
            stream = await client.create_stream("events", DataClass.PHI)
            offset = await client.append(stream, [b"hello"])
        finally:
            await client.disconnect()

    asyncio.run(main())
"""

from __future__ import annotations

import asyncio
from typing import Any, List, Optional

from .client import Client, ExecuteResult, QueryResult
from .types import DataClass, Offset, Placement, StreamId, TenantId
from .value import Value


class AsyncClient:
    """Asyncio-friendly wrapper over :class:`kimberlite.Client`.

    Every method dispatches the underlying synchronous call through
    ``asyncio.to_thread``. The wrapped client instance is stored verbatim —
    `AsyncClient` doesn't clone or pool connections internally. Use
    :class:`kimberlite.Pool` from a thread pool if you need concurrency.
    """

    def __init__(self, inner: Client) -> None:
        self._inner = inner

    @classmethod
    async def connect(
        cls,
        addresses: List[str],
        tenant_id: int,
        auth_token: Optional[str] = None,
    ) -> "AsyncClient":
        client = await asyncio.to_thread(
            Client.connect,
            addresses=addresses,
            tenant_id=tenant_id,
            auth_token=auth_token,
        )
        return cls(client)

    async def disconnect(self) -> None:
        await asyncio.to_thread(self._inner.disconnect)

    async def create_stream(
        self,
        name: str,
        data_class: DataClass,
        placement: Placement = Placement.GLOBAL,
        custom_region: Optional[str] = None,
    ) -> StreamId:
        return await asyncio.to_thread(
            self._inner.create_stream,
            name,
            data_class,
            placement=placement,
            custom_region=custom_region,
        )

    async def append(
        self,
        stream_id: StreamId,
        events: List[bytes],
        expected_offset: Offset = Offset(0),
    ) -> Offset:
        return await asyncio.to_thread(
            self._inner.append, stream_id, events, expected_offset
        )

    async def read(
        self,
        stream_id: StreamId,
        from_offset: Offset = Offset(0),
        max_bytes: int = 1024 * 1024,
    ) -> List[Any]:
        return await asyncio.to_thread(
            self._inner.read, stream_id, from_offset, max_bytes
        )

    async def query(
        self, sql: str, params: Optional[List[Value]] = None
    ) -> QueryResult:
        return await asyncio.to_thread(self._inner.query, sql, params)

    async def execute(
        self, sql: str, params: Optional[List[Value]] = None
    ) -> ExecuteResult:
        return await asyncio.to_thread(self._inner.execute, sql, params)

    async def sync(self) -> None:
        await asyncio.to_thread(self._inner.sync)

    @property
    def tenant_id(self) -> TenantId:
        return self._inner.tenant_id

    @property
    def last_request_id(self) -> Optional[int]:
        return self._inner.last_request_id

    async def __aenter__(self) -> "AsyncClient":
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb) -> None:
        await self.disconnect()
