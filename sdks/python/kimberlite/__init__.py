"""Kimberlite Python SDK.

Pythonic client library for Kimberlite database with type hints and context managers.
"""

from .client import Client, QueryResult
from .types import DataClass, StreamId, Offset
from .value import Value, ValueType
from .errors import (
    KimberliteError,
    ConnectionError,
    StreamNotFoundError,
    PermissionDeniedError,
    AuthenticationError,
    TimeoutError,
)

__version__ = "0.1.0"
__all__ = [
    "Client",
    "QueryResult",
    "DataClass",
    "StreamId",
    "Offset",
    "Value",
    "ValueType",
    "KimberliteError",
    "ConnectionError",
    "StreamNotFoundError",
    "PermissionDeniedError",
    "AuthenticationError",
    "TimeoutError",
]
