"""Tests for :mod:`kimberlite.retry`.

AUDIT-2026-04 S2.4 — mirrors the TS SDK's ``retry.test.ts`` and
the Rust SDK's retry unit tests.
"""

import pytest

from kimberlite.errors import KimberliteError
from kimberlite.retry import DEFAULT_RETRY, RetryPolicy, with_retry


class _RetryableError(KimberliteError):
    """Test error whose `is_retryable` returns True regardless of code."""

    def __init__(self, msg: str = "rate-limited"):
        super().__init__(msg, code=14)  # CLUSTER_UNAVAILABLE → retryable


class _TerminalError(KimberliteError):
    """Test error whose `is_retryable` returns False (QuerySyntax)."""

    def __init__(self, msg: str = "bad SQL"):
        super().__init__(msg, code=8)  # QUERY_SYNTAX → not retryable


# Fast policy so the suite doesn't touch the wall clock.
FAST = RetryPolicy(max_attempts=4, base_delay_ms=1, cap_delay_ms=4)


def _no_sleep(_: float) -> None:
    """Injected sleep that does nothing — lets tests run instantly."""
    return None


def test_returns_op_result_on_first_success():
    calls = {"n": 0}

    def op():
        calls["n"] += 1
        return 42

    result = with_retry(op, FAST, sleep=_no_sleep)
    assert result == 42
    assert calls["n"] == 1


def test_retries_retryable_error_and_eventually_succeeds():
    calls = {"n": 0}

    def op():
        calls["n"] += 1
        if calls["n"] < 3:
            raise _RetryableError(f"attempt {calls['n']}")
        return "ok"

    result = with_retry(op, FAST, sleep=_no_sleep)
    assert result == "ok"
    assert calls["n"] == 3


def test_non_retryable_error_is_not_retried():
    calls = {"n": 0}

    def op():
        calls["n"] += 1
        raise _TerminalError()

    with pytest.raises(_TerminalError):
        with_retry(op, FAST, sleep=_no_sleep)
    assert calls["n"] == 1


def test_gives_up_after_max_attempts():
    calls = {"n": 0}

    def op():
        calls["n"] += 1
        raise _RetryableError(f"attempt {calls['n']}")

    with pytest.raises(_RetryableError) as exc_info:
        with_retry(op, FAST, sleep=_no_sleep)
    assert calls["n"] == FAST.max_attempts
    # The LAST error is the one surfaced — diagnostics should point
    # at the most recent failure, not the first.
    assert "attempt 4" in str(exc_info.value)


def test_max_attempts_1_disables_retry():
    """``max_attempts=1`` means the initial attempt is the only attempt."""
    calls = {"n": 0}
    policy = RetryPolicy(max_attempts=1, base_delay_ms=1, cap_delay_ms=1)

    def op():
        calls["n"] += 1
        raise _RetryableError()

    with pytest.raises(_RetryableError):
        with_retry(op, policy, sleep=_no_sleep)
    assert calls["n"] == 1


def test_default_retry_is_four_attempts():
    """Sanity check on public constants — TS/Rust/Python agree."""
    assert DEFAULT_RETRY.max_attempts == 4
    assert DEFAULT_RETRY.base_delay_ms == 50
    assert DEFAULT_RETRY.cap_delay_ms == 800


def test_non_kimberlite_error_propagates_without_retry():
    """A bare ``ValueError`` from app code is never retried — it's a
    programming error that won't go away on retry."""
    calls = {"n": 0}

    def op():
        calls["n"] += 1
        raise ValueError("app bug")

    with pytest.raises(ValueError):
        with_retry(op, FAST, sleep=_no_sleep)
    assert calls["n"] == 1


def test_backoff_sequence_is_exponential_and_capped():
    """The wait durations passed to the injected `sleep` are
    base, 2×base, min(cap, 4×base) for attempts 1→2, 2→3, 3→4."""
    waits: list[float] = []

    def capturing_sleep(seconds: float) -> None:
        waits.append(seconds)

    calls = {"n": 0}

    def op():
        calls["n"] += 1
        raise _RetryableError()

    policy = RetryPolicy(max_attempts=4, base_delay_ms=10, cap_delay_ms=30)
    with pytest.raises(_RetryableError):
        with_retry(op, policy, sleep=capturing_sleep)

    # Three waits between 4 attempts: 10 ms, 20 ms, min(30, 40)=30 ms.
    assert waits == [0.010, 0.020, 0.030]
