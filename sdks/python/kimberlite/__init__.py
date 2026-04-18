"""Kimberlite Python SDK.

Pythonic client library for Kimberlite database with type hints and context managers.
"""

from .client import Client, ExecuteResult, QueryResult
from .pool import Pool, PooledClient, PoolStats
from .subscription import Subscription, SubscriptionEvent, SubscriptionClosedError
from .types import DataClass, Placement, StreamId, Offset, TenantId
from .value import Value, ValueType
from .errors import (
    KimberliteError,
    ConnectionError,
    StreamNotFoundError,
    PermissionDeniedError,
    AuthenticationError,
    TimeoutError,
    InvalidDataClassError,
    OffsetOutOfRangeError,
    QuerySyntaxError,
    QueryExecutionError,
    InternalError,
    ClusterUnavailableError,
)

__version__ = "0.4.1"
__all__ = [
    "Client",
    "ExecuteResult",
    "Pool",
    "PooledClient",
    "PoolStats",
    "QueryResult",
    "Subscription",
    "SubscriptionEvent",
    "SubscriptionClosedError",
    "DataClass",
    "Placement",
    "StreamId",
    "Offset",
    "TenantId",
    "Value",
    "ValueType",
    "KimberliteError",
    "ConnectionError",
    "StreamNotFoundError",
    "PermissionDeniedError",
    "AuthenticationError",
    "TimeoutError",
    "InvalidDataClassError",
    "OffsetOutOfRangeError",
    "QuerySyntaxError",
    "QueryExecutionError",
    "InternalError",
    "ClusterUnavailableError",
]
