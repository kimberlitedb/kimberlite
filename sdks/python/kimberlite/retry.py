"""Retry helper for Kimberlite operations.

AUDIT-2026-04 S2.4 — ports notebar's retry idiom into the Python
SDK so every app gets identical backoff semantics without
hand-rolling a wrapper. Mirrors the TypeScript SDK's ``withRetry``
and the Rust SDK's ``with_retry``.

Usage:

    from kimberlite import Client
    from kimberlite.retry import with_retry, DEFAULT_RETRY

    client = Client.connect(...)
    rows = with_retry(lambda: client.query("SELECT * FROM patients"))
"""

from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Callable, TypeVar

from .errors import KimberliteError

T = TypeVar("T")


@dataclass(frozen=True)
class RetryPolicy:
    """Exponential-backoff retry policy.

    Attributes:
        max_attempts: Total attempts INCLUDING the initial call.
            A value of 1 disables retries.
        base_delay_ms: Delay before the first retry, in milliseconds.
        cap_delay_ms: Upper bound on the delay between attempts.
    """

    max_attempts: int
    base_delay_ms: int
    cap_delay_ms: int


#: Sensible default: four attempts, 50 ms → 100 ms → 200 ms → 400 ms.
#: Total worst-case wall-clock overhead is ~750 ms before the
#: final error surfaces, matching the TS/Rust SDKs.
DEFAULT_RETRY = RetryPolicy(
    max_attempts=4,
    base_delay_ms=50,
    cap_delay_ms=800,
)


def _can_retry(err: object) -> bool:
    """Return True if ``err`` exposes ``is_retryable()`` returning True.

    Non-Kimberlite exceptions (e.g. a bare ``ValueError`` from app
    code) are never retried — they are programming errors that
    wouldn't go away on retry.
    """
    predicate = getattr(err, "is_retryable", None)
    if not callable(predicate):
        return False
    try:
        return bool(predicate())
    except Exception:  # pragma: no cover — defensive
        return False


def with_retry(
    op: Callable[[], T],
    policy: RetryPolicy = DEFAULT_RETRY,
    *,
    sleep: Callable[[float], None] = time.sleep,
) -> T:
    """Run ``op`` with exponential-backoff retries for retryable errors.

    Non-retryable errors propagate immediately. Giving up at
    ``policy.max_attempts`` re-raises the most recent error.

    Args:
        op: Zero-arg callable to execute. Typically a ``lambda`` that
            wraps a Kimberlite SDK call.
        policy: Retry policy. Defaults to :data:`DEFAULT_RETRY`.
        sleep: Injectable sleep function — tests pass a no-op so
            the suite doesn't hit the wall clock. Production code
            should rely on the default (``time.sleep``).

    Returns:
        Whatever ``op`` returns on the first non-throwing attempt.

    Raises:
        KimberliteError: When ``op`` raises a non-retryable error, or
            when ``max_attempts`` is exhausted.
    """
    attempt = 0
    while True:
        try:
            return op()
        except KimberliteError as e:
            attempt += 1
            if attempt >= policy.max_attempts or not _can_retry(e):
                raise
            # Exponential backoff, doubled each attempt, capped.
            # attempt==1: base, attempt==2: 2*base, attempt==3: 4*base, …
            wait_ms = min(
                policy.cap_delay_ms,
                policy.base_delay_ms * (2 ** (attempt - 1)),
            )
            sleep(wait_ms / 1000.0)
