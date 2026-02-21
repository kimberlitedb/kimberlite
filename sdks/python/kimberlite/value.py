"""SQL value types for Kimberlite queries.

This module provides type-safe representations of SQL values that can be used
as query parameters and returned from query results.
"""

from datetime import datetime
from enum import IntEnum
from typing import Optional, Union


class ValueType(IntEnum):
    """Type tag for SQL values.

    Attributes:
        NULL: SQL NULL value
        BIGINT: 64-bit signed integer
        TEXT: UTF-8 string
        BOOLEAN: Boolean (true/false)
        TIMESTAMP: Timestamp (nanoseconds since Unix epoch)
    """

    NULL = 0
    BIGINT = 1
    TEXT = 2
    BOOLEAN = 3
    TIMESTAMP = 4


class Value:
    """SQL value wrapper with type information.

    A Value represents a typed SQL value that can be used as a query parameter
    or returned from a query result. Values are immutable.

    Examples:
        >>> Value.null()
        Value.null()
        >>> Value.bigint(42)
        Value(BIGINT, 42)
        >>> Value.text("hello")
        Value(TEXT, 'hello')
        >>> Value.boolean(True)
        Value(BOOLEAN, True)
        >>> Value.timestamp(1234567890)
        Value(TIMESTAMP, 1234567890)

    """

    __slots__ = ("type", "data")

    def __init__(self, value_type: ValueType, data: Union[None, int, str, bool]):
        """Create a Value with a specific type and data.

        Args:
            value_type: The type tag for this value
            data: The actual data (type must match value_type)

        Note:
            Use the static factory methods (null, bigint, text, boolean, timestamp)
            instead of calling this constructor directly.
        """
        self.type = value_type
        self.data = data

    @staticmethod
    def null() -> "Value":
        """Create a NULL value.

        Returns:
            A Value representing SQL NULL
        """
        return Value(ValueType.NULL, None)

    @staticmethod
    def bigint(val: int) -> "Value":
        """Create a BIGINT value from a Python int.

        Args:
            val: Integer value (must fit in 64-bit signed integer)

        Returns:
            A Value containing the integer

        Raises:
            ValueError: If val is out of range for a 64-bit signed integer
        """
        if not isinstance(val, int):
            raise TypeError(f"expected int, got {type(val).__name__}")
        if val < -(2**63) or val >= 2**63:
            raise ValueError(f"value {val} out of range for BIGINT")
        return Value(ValueType.BIGINT, val)

    @staticmethod
    def text(val: str) -> "Value":
        """Create a TEXT value from a Python string.

        Args:
            val: UTF-8 string value

        Returns:
            A Value containing the string

        Raises:
            TypeError: If val is not a string
        """
        if not isinstance(val, str):
            raise TypeError(f"expected str, got {type(val).__name__}")
        return Value(ValueType.TEXT, val)

    @staticmethod
    def boolean(val: bool) -> "Value":
        """Create a BOOLEAN value from a Python bool.

        Args:
            val: Boolean value

        Returns:
            A Value containing the boolean

        Raises:
            TypeError: If val is not a bool
        """
        if not isinstance(val, bool):
            raise TypeError(f"expected bool, got {type(val).__name__}")
        return Value(ValueType.BOOLEAN, val)

    @staticmethod
    def timestamp(nanos: int) -> "Value":
        """Create a TIMESTAMP value from nanoseconds since Unix epoch.

        Args:
            nanos: Nanoseconds since Unix epoch (1970-01-01 00:00:00 UTC)

        Returns:
            A Value containing the timestamp

        Raises:
            TypeError: If nanos is not an integer
        """
        if not isinstance(nanos, int):
            raise TypeError(f"expected int, got {type(nanos).__name__}")
        return Value(ValueType.TIMESTAMP, nanos)

    @staticmethod
    def from_datetime(dt: datetime) -> "Value":
        """Create a TIMESTAMP value from a Python datetime.

        Args:
            dt: Python datetime object (will be converted to UTC if naive)

        Returns:
            A Value containing the timestamp

        Raises:
            TypeError: If dt is not a datetime

        Examples:
            >>> from datetime import datetime
            >>> Value.from_datetime(datetime(2024, 1, 1, 12, 0, 0))
            Value(TIMESTAMP, ...)
        """
        if not isinstance(dt, datetime):
            raise TypeError(f"expected datetime, got {type(dt).__name__}")

        # Convert to timestamp (seconds since epoch) then to nanoseconds
        timestamp_seconds = dt.timestamp()
        nanos = int(timestamp_seconds * 1_000_000_000)
        return Value(ValueType.TIMESTAMP, nanos)

    def to_datetime(self) -> Optional[datetime]:
        """Convert a TIMESTAMP value to a Python datetime.

        Returns:
            A datetime object in UTC, or None if this value is not a TIMESTAMP

        Examples:
            >>> val = Value.timestamp(1609459200_000_000_000)  # 2021-01-01 00:00:00 UTC
            >>> val.to_datetime()
            datetime.datetime(2021, 1, 1, 0, 0, tzinfo=datetime.timezone.utc)
        """
        if self.type == ValueType.TIMESTAMP:
            assert isinstance(self.data, int)
            timestamp_seconds = self.data / 1_000_000_000
            return datetime.fromtimestamp(timestamp_seconds)
        return None

    def is_null(self) -> bool:
        """Check if this value is NULL.

        Returns:
            True if this is a NULL value, False otherwise
        """
        return self.type == ValueType.NULL

    def __eq__(self, other: object) -> bool:
        """Check equality with another Value.

        Args:
            other: Another Value to compare with

        Returns:
            True if both values have the same type and data
        """
        if not isinstance(other, Value):
            return NotImplemented
        return self.type == other.type and self.data == other.data

    def __hash__(self) -> int:
        """Compute hash of this Value.

        Returns:
            Hash value based on type and data
        """
        return hash((self.type, self.data))

    def __repr__(self) -> str:
        """Return a developer-friendly representation.

        Returns:
            String representation suitable for debugging
        """
        if self.is_null():
            return "Value.null()"
        return f"Value({self.type.name}, {self.data!r})"

    def __str__(self) -> str:
        """Return a user-friendly string representation.

        Returns:
            String representation of the value's data
        """
        if self.is_null():
            return "NULL"
        return str(self.data)
