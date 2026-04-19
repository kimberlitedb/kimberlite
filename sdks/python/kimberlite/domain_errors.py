"""Domain-level error mapping for Kimberlite.

AUDIT-2026-04 S2.4 — lifts ``mapKimberliteError`` out of
``notebar/packages/kimberlite-client/src/retry.ts`` into the SDK
so every Python app (FastAPI handlers, Django views, Flask
routes) gets a single canonical translation from wire-level
errors to app-visible domain-error shapes.

Typical usage in an HTTP handler::

    from kimberlite.domain_errors import as_result, DomainError

    result = as_result(lambda: client.query("..."))
    if not result.ok:
        return render_error_response(result.err)
    return jsonify(result.value)
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Generic, List, TypeVar, Union

from .errors import KimberliteError

T = TypeVar("T")


# ---------------------------------------------------------------------------
# DomainError variants — one dataclass per shape.
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class NotFound:
    """The referenced entity does not exist (stream, table, tenant,
    API key)."""


@dataclass(frozen=True)
class Forbidden:
    """Authenticated but not authorised, or the token is invalid."""


@dataclass(frozen=True)
class ConcurrentModification:
    """Optimistic-concurrency conflict (``OffsetMismatch``). Re-read
    the stream offset and retry."""


@dataclass(frozen=True)
class Conflict:
    """A uniqueness or precondition conflict."""

    reasons: List[str] = field(default_factory=list)


@dataclass(frozen=True)
class InvariantViolation:
    """Server-side invariant violated. Indicates a server bug."""

    name: str


@dataclass(frozen=True)
class Unavailable:
    """Server or cluster is unavailable for this request."""

    message: str


@dataclass(frozen=True)
class RateLimited:
    """Server-side rate limiting. Back off and retry."""


@dataclass(frozen=True)
class Timeout:
    """Client-side or network timeout."""


@dataclass(frozen=True)
class Validation:
    """Caller error — malformed request, invalid SQL, bad data class."""

    message: str


#: Union of every domain-error shape. App code pattern-matches with
#: ``isinstance(err, NotFound)`` or similar.
DomainError = Union[
    NotFound,
    Forbidden,
    ConcurrentModification,
    Conflict,
    InvariantViolation,
    Unavailable,
    RateLimited,
    Timeout,
    Validation,
]


# ---------------------------------------------------------------------------
# Code → DomainError lookup. Source of truth for the translation is
# shared with the TS/Rust SDKs — see their equivalent modules.
#
# FFI codes are used because the Python SDK receives FFI-code ints on
# `KimberliteError.code`. See `sdks/python/kimberlite/ffi.py` for the
# KMB_ERR_* constants.
# ---------------------------------------------------------------------------

_CODE_NOT_FOUND = {4, 10}              # Stream/Tenant
_CODE_AUTH_FAILED = {11}
_CODE_RATE_LIMITED_LIKE = {14}         # CLUSTER_UNAVAILABLE covers rate-limited at FFI
_CODE_VALIDATION = {6, 7, 8, 9}        # InvalidDataClass, OffsetOOR, QuerySyntax, QueryExec
_CODE_TIMEOUT = {12}
_CODE_INTERNAL = {13}


def map_kimberlite_error(err: object) -> DomainError:
    """Translate any thrown value into a :data:`DomainError`.

    Non-Kimberlite exceptions fall through to :class:`Unavailable`
    with the stringified message — never opaque, never reveals
    stack frames.
    """
    if isinstance(err, KimberliteError):
        code = err.code
        if code in _CODE_NOT_FOUND:
            return NotFound()
        if code in _CODE_AUTH_FAILED:
            return Forbidden()
        if code in _CODE_RATE_LIMITED_LIKE:
            # FFI folds NotLeader + RateLimited into CLUSTER_UNAVAILABLE.
            # From an app's perspective both mean "back off and retry".
            return RateLimited()
        if code in _CODE_TIMEOUT:
            return Timeout()
        if code == 7:  # OffsetOutOfRange covers concurrent writes.
            return ConcurrentModification()
        if code in {8, 9, 6}:  # Parse, Execution, InvalidDataClass.
            return Validation(message=err.message or "invalid request")
        if code in _CODE_INTERNAL:
            return Unavailable(message=err.message or "internal error")
        # Unknown / Null / UTF-8 / generic KimberliteError.
        return Unavailable(message=err.message or "unknown error")

    if isinstance(err, BaseException):
        return Unavailable(message=str(err))

    return Unavailable(message=repr(err))


# ---------------------------------------------------------------------------
# Result<T, DomainError> — lightweight, no external dep.
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Ok(Generic[T]):
    """Successful result. Access via :attr:`value`."""

    value: T
    ok: bool = field(default=True, init=False)


@dataclass(frozen=True)
class Err:
    """Failed result — carries a :data:`DomainError`."""

    err: DomainError
    ok: bool = field(default=False, init=False)


Result = Union[Ok[T], Err]


def as_result(op: Callable[[], T]) -> Result[T]:
    """Run ``op()`` and return a :data:`Result`.

    Wraps a throwing callable in a ``Result`` so handlers can
    pattern-match without a try/except around every call site::

        result = as_result(lambda: client.query("..."))
        if isinstance(result, Err):
            return render_error(result.err)
        return result.value
    """
    try:
        return Ok(value=op())
    except BaseException as e:  # noqa: BLE001 — intentional wide catch
        return Err(err=map_kimberlite_error(e))
