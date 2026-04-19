"""Error types for Kimberlite Python SDK.

All exceptions inherit from :class:`KimberliteError` and carry a numeric
``code`` from the FFI layer. Classification predicates (``is_retryable``,
``is_not_found``, etc.) mirror the Rust client at
``crates/kimberlite-client/src/error.rs`` so cross-language applications
can branch on error shape identically.
"""

from typing import Callable, Dict, Optional


# FFI error codes, mirrored from sdks/python/kimberlite/ffi.py and
# crates/kimberlite-ffi/src/lib.rs. Declared here so predicate methods
# can classify without importing ffi (which would create a cycle at
# module load time).
_ERR_CONNECTION_FAILED = 3
_ERR_STREAM_NOT_FOUND = 4
_ERR_PERMISSION_DENIED = 5
_ERR_INVALID_DATA_CLASS = 6
_ERR_OFFSET_OUT_OF_RANGE = 7
_ERR_QUERY_SYNTAX = 8
_ERR_QUERY_EXECUTION = 9
_ERR_TENANT_NOT_FOUND = 10
_ERR_AUTH_FAILED = 11
_ERR_TIMEOUT = 12
_ERR_INTERNAL = 13
_ERR_CLUSTER_UNAVAILABLE = 14
_ERR_UNKNOWN = 15


class KimberliteError(Exception):
    """Base exception for all Kimberlite errors.

    Attributes:
        message: Human-readable description of the failure.
        code: Numeric FFI error code (see ``ffi.py`` for canonical
            values). ``None`` for errors synthesised client-side.
        request_id: Wire-protocol request ID associated with this
            error, if available. The FFI layer does not yet surface
            this on every error path; see the ``last_request_id``
            getter on :class:`~kimberlite.client.Client` for the
            last-issued request. Populated by the FFI shim when
            request correlation is available.
    """

    def __init__(
        self,
        message: str,
        code: Optional[int] = None,
        request_id: Optional[int] = None,
    ):
        super().__init__(message)
        self.message = message
        self.code = code
        self.request_id = request_id

    # -------------------------------------------------------------------
    # Classification predicates.
    #
    # Mirrors the Rust client's classification at
    # crates/kimberlite-client/src/error.rs::ClientError. A caller that
    # writes idiomatic Rust: ``if err.is_retryable() { retry() }`` should
    # get the same shape in Python: ``if err.is_retryable(): retry()``.
    # -------------------------------------------------------------------

    def is_retryable(self) -> bool:
        """True if the error is likely to succeed on retry.

        Returns ``True`` for transient failures: connection loss,
        timeout, and cluster-unavailable (which covers NotLeader and
        rate-limited server states at the FFI layer). Matches the
        Rust client's ``ClientError::is_retryable``.
        """
        if self.code is None:
            return False
        return self.code in (
            _ERR_CONNECTION_FAILED,
            _ERR_TIMEOUT,
            _ERR_CLUSTER_UNAVAILABLE,
        )

    def is_not_found(self) -> bool:
        """True if a named resource (stream, table, tenant) was not found."""
        if self.code is None:
            return False
        return self.code in (_ERR_STREAM_NOT_FOUND, _ERR_TENANT_NOT_FOUND)

    def is_auth_failed(self) -> bool:
        """True if authentication failed (bad token, expired, revoked)."""
        return self.code == _ERR_AUTH_FAILED

    def is_not_leader(self) -> bool:
        """True if the request reached a non-leader replica.

        Clients typically reconnect to the leader hint in the message
        and retry. At the FFI boundary this is folded into
        ``ClusterUnavailable`` — ``is_retryable()`` covers both shapes.
        """
        return self.code == _ERR_CLUSTER_UNAVAILABLE

    def is_rate_limited(self) -> bool:
        """True if the server rejected the request due to rate limiting.

        At the FFI boundary this is folded into ``ClusterUnavailable``;
        use :meth:`is_retryable` for the broader "back off and retry"
        check.
        """
        return self.code == _ERR_CLUSTER_UNAVAILABLE

    def is_offset_mismatch(self) -> bool:
        """True if this is an optimistic-concurrency conflict on append.

        The caller should re-read the stream's current offset and
        retry the append with the fresh value. At the FFI boundary
        this is reported as ``OffsetOutOfRange``.
        """
        return self.code == _ERR_OFFSET_OUT_OF_RANGE

    def is_permission_denied(self) -> bool:
        """True if the tenant lacks permission for this operation."""
        return self.code == _ERR_PERMISSION_DENIED


class ConnectionError(KimberliteError):
    """Failed to connect to Kimberlite server."""
    pass


class StreamNotFoundError(KimberliteError):
    """Stream ID does not exist."""
    pass


class PermissionDeniedError(KimberliteError):
    """Operation not permitted for this tenant."""
    pass


class AuthenticationError(KimberliteError):
    """Authentication failed."""
    pass


class TimeoutError(KimberliteError):
    """Operation timed out."""
    pass


class InvalidDataClassError(KimberliteError):
    """Invalid data class value."""
    pass


class OffsetOutOfRangeError(KimberliteError):
    """Offset is beyond stream end.

    Also surfaces optimistic-concurrency conflicts on append —
    callers should check :meth:`KimberliteError.is_offset_mismatch`
    and retry with a fresh stream offset.
    """
    pass


class QuerySyntaxError(KimberliteError):
    """SQL syntax error."""
    pass


class QueryExecutionError(KimberliteError):
    """Query execution error."""
    pass


class InternalError(KimberliteError):
    """Internal server error."""
    pass


class ClusterUnavailableError(KimberliteError):
    """No cluster replicas available.

    Also surfaces NotLeader and RateLimited at the wire boundary;
    callers should retry (see :meth:`KimberliteError.is_retryable`)
    or reconnect to a leader hint if the message carries one.
    """
    pass


# Error code to exception mapping
ERROR_MAP: Dict[int, Callable[[str], KimberliteError]] = {
    1: lambda msg: KimberliteError(msg, 1),  # NULL pointer
    2: lambda msg: KimberliteError(msg, 2),  # Invalid UTF-8
    3: lambda msg: ConnectionError(msg, 3),
    4: lambda msg: StreamNotFoundError(msg, 4),
    5: lambda msg: PermissionDeniedError(msg, 5),
    6: lambda msg: InvalidDataClassError(msg, 6),
    7: lambda msg: OffsetOutOfRangeError(msg, 7),
    8: lambda msg: QuerySyntaxError(msg, 8),
    9: lambda msg: QueryExecutionError(msg, 9),
    10: lambda msg: KimberliteError(msg, 10),  # Tenant not found
    11: lambda msg: AuthenticationError(msg, 11),
    12: lambda msg: TimeoutError(msg, 12),
    13: lambda msg: InternalError(msg, 13),
    14: lambda msg: ClusterUnavailableError(msg, 14),
    15: lambda msg: KimberliteError(msg, 15),  # Unknown
}


def raise_for_error_code(code: int, request_id: Optional[int] = None) -> None:
    """Raise appropriate exception for FFI error code.

    Args:
        code: FFI error code (0 = success)
        request_id: Wire request ID associated with the failing call,
            if the caller has it available. Attached to the raised
            exception for correlation in structured logs.

    Raises:
        KimberliteError: Appropriate exception for error code
    """
    if code == 0:
        return

    # Import here to avoid circular dependency
    from .ffi import _lib

    msg = _lib.kmb_error_message(code).decode('utf-8')
    exception_factory: Callable[[str], KimberliteError] = ERROR_MAP.get(
        code, lambda m: KimberliteError(m, code)
    )
    err = exception_factory(msg)
    if request_id is not None:
        err.request_id = request_id
    raise err
