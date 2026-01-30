"""Kimberlite Python SDK.

Pythonic client library for Kimberlite database with type hints and context managers.
"""

from .client import Client
from .types import DataClass, StreamId, Offset
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
    "DataClass",
    "StreamId",
    "Offset",
    "KimberliteError",
    "ConnectionError",
    "StreamNotFoundError",
    "PermissionDeniedError",
    "AuthenticationError",
    "TimeoutError",
]
