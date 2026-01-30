"""Error types for Kimberlite Python SDK."""

from typing import Optional


class KimberliteError(Exception):
    """Base exception for all Kimberlite errors."""

    def __init__(self, message: str, code: Optional[int] = None):
        super().__init__(message)
        self.message = message
        self.code = code


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
    """Offset is beyond stream end."""
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
    """No cluster replicas available."""
    pass


# Error code to exception mapping
ERROR_MAP = {
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


def raise_for_error_code(code: int) -> None:
    """Raise appropriate exception for FFI error code.

    Args:
        code: FFI error code (0 = success)

    Raises:
        KimberliteError: Appropriate exception for error code
    """
    if code == 0:
        return

    # Import here to avoid circular dependency
    from .ffi import _lib, _check_error

    msg = _lib.kmb_error_message(code).decode('utf-8')
    exception_factory = ERROR_MAP.get(code, lambda m: KimberliteError(m, code))
    raise exception_factory(msg)
