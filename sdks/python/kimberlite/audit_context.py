"""Audit context propagation.

AUDIT-2026-04 S2.4 — provides an ambient context carrier so apps
can set ``{actor, reason, request_id, correlation_id}`` once per
request and have every nested Kimberlite operation pick it up for
structured logging / distributed tracing.

Uses :class:`contextvars.ContextVar` so the context survives
``await`` boundaries in asyncio apps and is isolated per thread
in sync apps (asyncio.run / thread-pool workers).

Example — FastAPI handler::

    from fastapi import Request
    from kimberlite.audit_context import AuditContext, run_with_audit

    @app.middleware("http")
    async def audit_middleware(request: Request, call_next):
        ctx = AuditContext(
            actor=request.state.user_id,
            reason=request.headers.get("x-access-reason", "default"),
            correlation_id=request.headers.get("x-request-id"),
        )
        with run_with_audit(ctx):
            return await call_next(request)
"""

from __future__ import annotations

import contextvars
from contextlib import contextmanager
from dataclasses import dataclass
from typing import Iterator, Optional


@dataclass(frozen=True)
class AuditContext:
    """Structured audit context carried through a call chain.

    ``actor`` and ``reason`` are mandatory in regulated-industry
    apps (HIPAA minimum-necessary, GDPR purpose limitation,
    FedRAMP audit-trail completeness).
    """

    actor: str
    reason: str
    request_id: Optional[str] = None
    correlation_id: Optional[str] = None


# Use a module-private ContextVar so callers cannot directly
# mutate it — they must go through run_with_audit / current_audit.
_CTX: contextvars.ContextVar[Optional[AuditContext]] = contextvars.ContextVar(
    "kimberlite_audit_ctx",
    default=None,
)


@contextmanager
def run_with_audit(ctx: AuditContext) -> Iterator[AuditContext]:
    """Context-manager that installs ``ctx`` as the active audit
    context for the duration of the ``with`` block.

    Nested blocks see the innermost context; outer contexts are
    restored on exit (including via exception).

    Example::

        with run_with_audit(AuditContext("alice", "chart-review")):
            rows = client.query("SELECT * FROM patients WHERE id = $1", [42])
    """
    token = _CTX.set(ctx)
    try:
        yield ctx
    finally:
        _CTX.reset(token)


def current_audit() -> Optional[AuditContext]:
    """Return the currently-active :class:`AuditContext`, or
    ``None`` if no context is active.

    SDK call sites can enrich structured logs with the current
    context without requiring callers to pass it explicitly::

        ctx = current_audit()
        logger.info("query issued", extra={"actor": ctx.actor if ctx else "?"})
    """
    return _CTX.get()


def require_audit() -> AuditContext:
    """Return the active :class:`AuditContext`, raising
    :class:`RuntimeError` if none is set.

    Use at call sites that refuse to run without attribution
    (break-glass queries, PHI exports, compliance reports).
    """
    ctx = _CTX.get()
    if ctx is None:
        raise RuntimeError(
            "require_audit(): no audit context active — "
            "wrap the call in `with run_with_audit(AuditContext(...)):`"
        )
    return ctx


@contextmanager
def _ffi_audit_attached() -> Iterator[None]:
    """Mirror the current Python audit context onto the Rust FFI
    thread-local for the duration of the ``with`` block.

    Called automatically by the :class:`kimberlite.Client` methods so
    every wire Request carries the caller's attribution. No-op when
    there's no active context.

    Not part of the public API — apps use :func:`run_with_audit`.
    """
    ctx = _CTX.get()
    if ctx is None:
        yield
        return

    # Deferred import to avoid cycle: ffi.py imports audit_context? No,
    # but audit_context is a leaf module today so we keep the import
    # lazy to stay tolerant of future shuffles.
    from .ffi import _lib

    def _encode(s: Optional[str]) -> Optional[bytes]:
        if s is None or s == "":
            return None
        return s.encode("utf-8")

    _lib.kmb_audit_set(
        _encode(ctx.actor),
        _encode(ctx.reason),
        _encode(ctx.correlation_id),
        _encode(ctx.request_id),
    )
    try:
        yield
    finally:
        _lib.kmb_audit_clear()
