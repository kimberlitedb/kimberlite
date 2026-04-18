"""Real-time stream subscriptions (protocol v2).

Usage:

    >>> sub = client.subscribe(stream_id, initial_credits=128)
    >>> for event in sub:
    ...     print(event.offset, event.data)

Subscriptions auto-replenish credits when the balance drops below a
low-water mark; override via ``initial_credits``/``low_water``/``refill``
arguments to :meth:`Client.subscribe`.

The Python wrapper is synchronous — iteration blocks the calling thread.
An asyncio-friendly variant is on the roadmap for Phase 7.
"""

from __future__ import annotations

import ctypes
import threading
from dataclasses import dataclass
from types import TracebackType
from typing import Iterator, Optional

from .ffi import (
    _check_error,
    _lib,
    KmbClient,
    KmbSubscribeResult,
    KmbSubscriptionEvent,
    close_reason_name,
)
from .types import Offset
from .errors import KimberliteError


@dataclass(frozen=True)
class SubscriptionEvent:
    """A single event delivered on a subscription."""

    offset: Offset
    data: bytes


class SubscriptionClosedError(KimberliteError):
    """The subscription ended. ``reason`` is the close-reason string."""

    def __init__(self, subscription_id: int, reason: str):
        super().__init__(
            f"subscription {subscription_id} closed: {reason}"
        )
        self.subscription_id = subscription_id
        self.reason = reason


class Subscription:
    """Iterator over real-time stream events.

    Construct via :meth:`kimberlite.Client.subscribe`; do not instantiate
    directly.

    The subscription is *not* thread-safe — iterate from a single thread at
    a time. If you need concurrent consumers, open multiple subscriptions.
    """

    def __init__(
        self,
        handle: KmbClient,
        subscription_id: int,
        start_offset: Offset,
        initial_credits: int,
        low_water: Optional[int] = None,
        refill: Optional[int] = None,
    ) -> None:
        self._handle = handle
        self._subscription_id = subscription_id
        self._credits = initial_credits
        self._low_water = low_water if low_water is not None else max(initial_credits // 4, 1)
        self._refill = refill if refill is not None else max(initial_credits, 1)
        self._closed = False
        self._close_reason: Optional[str] = None
        self._lock = threading.RLock()

    @property
    def id(self) -> int:
        return self._subscription_id

    @property
    def credits(self) -> int:
        return self._credits

    @property
    def closed(self) -> bool:
        return self._closed

    @property
    def close_reason(self) -> Optional[str]:
        return self._close_reason

    def grant_credits(self, additional: int) -> int:
        """Grant ``additional`` credits; returns the new server-side balance."""
        if additional <= 0:
            raise ValueError("additional must be > 0")
        new_balance = ctypes.c_uint32(0)
        err = _lib.kmb_subscription_grant_credits(
            self._handle,
            self._subscription_id,
            additional,
            ctypes.byref(new_balance),
        )
        _check_error(err)
        self._credits = int(new_balance.value)
        return self._credits

    def unsubscribe(self) -> None:
        """Cancel the subscription. Idempotent."""
        with self._lock:
            if self._closed:
                return
            self._closed = True
            self._close_reason = "ClientCancelled"
            try:
                err = _lib.kmb_subscription_unsubscribe(
                    self._handle, self._subscription_id
                )
                # Tolerate already-closed subscriptions — makes unsubscribe idempotent.
                if err != 0:
                    from .errors import ERROR_MAP
                    # Only surface non-"not found" errors.
                    if err in ERROR_MAP and "not found" not in str(ERROR_MAP[err]("")):
                        _check_error(err)
            except KimberliteError:
                # Idempotent: swallow already-closed errors silently.
                pass

    def next_event(self) -> Optional[SubscriptionEvent]:
        """Block until the next event arrives. Returns ``None`` on close."""
        if self._closed:
            return None

        self._maybe_auto_refill()

        event = KmbSubscriptionEvent()
        err = _lib.kmb_subscription_next(
            self._handle, self._subscription_id, ctypes.byref(event)
        )
        _check_error(err)

        try:
            if event.closed:
                self._closed = True
                self._close_reason = close_reason_name(event.close_reason)
                return None

            if self._credits > 0:
                self._credits -= 1

            # Copy the event payload into a managed bytes object.
            length = int(event.data_len)
            if length > 0 and event.data:
                data = bytes(
                    ctypes.cast(
                        event.data, ctypes.POINTER(ctypes.c_uint8 * length)
                    ).contents
                )
            else:
                data = b""

            return SubscriptionEvent(offset=Offset(int(event.offset)), data=data)
        finally:
            _lib.kmb_subscription_event_free(ctypes.byref(event))

    def __iter__(self) -> Iterator[SubscriptionEvent]:
        return self

    def __next__(self) -> SubscriptionEvent:
        ev = self.next_event()
        if ev is None:
            raise StopIteration
        return ev

    def __enter__(self) -> "Subscription":
        return self

    def __exit__(
        self,
        exc_type: Optional[type],
        exc_val: Optional[BaseException],
        exc_tb: Optional[TracebackType],
    ) -> None:
        self.unsubscribe()

    def _maybe_auto_refill(self) -> None:
        if self._credits <= self._low_water and not self._closed:
            try:
                self.grant_credits(self._refill)
            except KimberliteError:
                # If the grant fails (e.g. subscription already closed), let
                # the next_event call surface the underlying error.
                pass
