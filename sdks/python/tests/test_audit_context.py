"""Tests for :mod:`kimberlite.audit_context`.

AUDIT-2026-04 S2.4 — pins the ContextVar behaviour so nested /
async / threaded call chains see the right context.
"""

import asyncio
import threading
from typing import Optional

import pytest

from kimberlite.audit_context import (
    AuditContext,
    current_audit,
    require_audit,
    run_with_audit,
)


ALICE = AuditContext(
    actor="alice@example.com",
    reason="chart-review",
    correlation_id="req-123",
)
BOB = AuditContext(actor="bob@example.com", reason="break-glass")


def test_no_context_returns_none():
    assert current_audit() is None


def test_with_block_exposes_context():
    with run_with_audit(ALICE):
        assert current_audit() == ALICE


def test_context_cleared_after_with_block():
    with run_with_audit(ALICE):
        pass
    assert current_audit() is None


def test_context_cleared_after_exception():
    class _Err(Exception):
        pass

    with pytest.raises(_Err):
        with run_with_audit(ALICE):
            raise _Err()
    # Context must be reset even when the block raised.
    assert current_audit() is None


def test_nested_scopes_restore_outer_context():
    with run_with_audit(ALICE):
        assert current_audit().actor == "alice@example.com"
        with run_with_audit(BOB):
            assert current_audit().actor == "bob@example.com"
            assert current_audit().reason == "break-glass"
        # Outer restored.
        assert current_audit().actor == "alice@example.com"


def test_require_audit_raises_without_context():
    with pytest.raises(RuntimeError, match="no audit context active"):
        require_audit()


def test_require_audit_returns_context_when_active():
    with run_with_audit(ALICE):
        assert require_audit() == ALICE


def test_context_survives_asyncio_await_boundaries():
    async def inner() -> Optional[AuditContext]:
        # Yield to the event loop, then read the context.
        await asyncio.sleep(0)
        return current_audit()

    async def main() -> None:
        with run_with_audit(ALICE):
            observed = await inner()
            assert observed is not None
            assert observed.actor == "alice@example.com"

    asyncio.run(main())


def test_asyncio_parallel_tasks_do_not_cross_contaminate():
    async def worker(ctx: AuditContext, delay: float) -> str:
        with run_with_audit(ctx):
            await asyncio.sleep(delay)
            return current_audit().actor

    async def main() -> tuple[str, str]:
        # Important: `asyncio.gather` creates separate tasks, each
        # with its own context copy. If the ContextVar were truly
        # shared mutable state, these would race.
        return await asyncio.gather(
            worker(ALICE, 0.01),
            worker(BOB, 0.005),
        )

    a, b = asyncio.run(main())
    assert a == "alice@example.com"
    assert b == "bob@example.com"


def test_threads_are_isolated():
    # Different OS threads must see independent contexts — the
    # ContextVar defaults to per-thread isolation in Python.
    results: dict[str, Optional[str]] = {}

    def run(name: str, ctx: AuditContext) -> None:
        with run_with_audit(ctx):
            c = current_audit()
            results[name] = c.actor if c else None

    t1 = threading.Thread(target=run, args=("t1", ALICE))
    t2 = threading.Thread(target=run, args=("t2", BOB))
    t1.start()
    t2.start()
    t1.join()
    t2.join()

    assert results["t1"] == "alice@example.com"
    assert results["t2"] == "bob@example.com"
    # Main thread saw nothing.
    assert current_audit() is None


def test_audit_context_is_frozen_and_immutable():
    with pytest.raises(Exception):
        ALICE.actor = "mallory"  # type: ignore[misc]
