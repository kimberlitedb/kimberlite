"""High-level Kimberlite client with Pythonic API."""

import ctypes
from typing import List, Optional, Sequence
from types import TracebackType

from .ffi import (
    _lib,
    _check_error,
    KmbClient,
    KmbClientConfig,
    KmbReadResult,
    KmbQueryParam,
    KmbQueryValue,
    KmbQueryResult,
)
from .types import DataClass, StreamId, Offset, TenantId
from .value import Value, ValueType
from .errors import KimberliteError


class Event:
    """A single event read from a stream.

    Attributes:
        offset: Position of event in stream
        data: Event payload bytes
    """

    def __init__(self, offset: Offset, data: bytes):
        self.offset = offset
        self.data = data

    def __repr__(self) -> str:
        return f"Event(offset={self.offset}, data={self.data!r})"


class QueryResult:
    """Result of a SQL query.

    Attributes:
        columns: List of column names
        rows: List of rows (each row is a list of Value objects)
    """

    def __init__(self, columns: List[str], rows: List[List[Value]]):
        """Initialize query result.

        Args:
            columns: Column names in result set
            rows: Rows of data (each row contains Value objects matching columns)
        """
        self.columns = columns
        self.rows = rows

    def __repr__(self) -> str:
        return f"QueryResult(columns={self.columns}, rows={len(self.rows)} rows)"

    def __len__(self) -> int:
        """Return number of rows in result."""
        return len(self.rows)


class Client:
    """Kimberlite database client.

    Provides a Pythonic interface to Kimberlite with context manager support.

    Example:
        >>> with Client.connect(
        ...     addresses=["localhost:5432"],
        ...     tenant_id=1,
        ...     auth_token="secret"
        ... ) as client:
        ...     stream_id = client.create_stream("events", DataClass.PHI)
        ...     offset = client.append(stream_id, [b"event1", b"event2"])
        ...     events = client.read(stream_id, from_offset=0, max_bytes=1024)
    """

    def __init__(self, handle: KmbClient):
        """Initialize client with FFI handle.

        Args:
            handle: Opaque FFI client handle

        Note:
            Use Client.connect() instead of calling this directly.
        """
        self._handle = handle
        self._closed = False

    @classmethod
    def connect(
        cls,
        addresses: List[str],
        tenant_id: int,
        auth_token: Optional[str] = None,
        client_name: str = "kimberlite-python",
        client_version: str = "0.1.0",
    ) -> "Client":
        """Connect to Kimberlite cluster.

        Args:
            addresses: List of "host:port" server addresses
            tenant_id: Tenant identifier
            auth_token: Optional authentication token
            client_name: Client identifier (for server logs)
            client_version: Client version string

        Returns:
            Connected client instance

        Raises:
            ConnectionError: If connection fails
            AuthenticationError: If authentication fails

        Example:
            >>> client = Client.connect(
            ...     addresses=["localhost:5432", "localhost:5433"],
            ...     tenant_id=1,
            ...     auth_token="secret"
            ... )
        """
        # Convert addresses to C array
        addr_ptrs = (ctypes.c_char_p * len(addresses))()
        for i, addr in enumerate(addresses):
            addr_ptrs[i] = addr.encode('utf-8')

        # Prepare config
        config = KmbClientConfig(
            addresses=ctypes.cast(addr_ptrs, ctypes.POINTER(ctypes.c_char_p)),
            address_count=len(addresses),
            tenant_id=tenant_id,
            auth_token=auth_token.encode('utf-8') if auth_token else None,
            client_name=client_name.encode('utf-8'),
            client_version=client_version.encode('utf-8'),
        )

        # Connect
        handle = KmbClient()
        err = _lib.kmb_client_connect(ctypes.byref(config), ctypes.byref(handle))
        _check_error(err)

        return cls(handle)

    def disconnect(self) -> None:
        """Disconnect from cluster and free resources.

        This is called automatically when using context manager.
        Safe to call multiple times.
        """
        if not self._closed and self._handle:
            _lib.kmb_client_disconnect(self._handle)
            self._closed = True
            self._handle = None

    def __enter__(self) -> "Client":
        """Enter context manager."""
        return self

    def __exit__(
        self,
        exc_type: Optional[type],
        exc_val: Optional[BaseException],
        exc_tb: Optional[TracebackType],
    ) -> None:
        """Exit context manager and disconnect."""
        self.disconnect()

    def __del__(self) -> None:
        """Ensure cleanup on garbage collection."""
        self.disconnect()

    def _check_connected(self) -> None:
        """Verify client is still connected.

        Raises:
            KimberliteError: If client is closed
        """
        if self._closed or not self._handle:
            raise KimberliteError("Client is closed")

    def create_stream(self, name: str, data_class: DataClass) -> StreamId:
        """Create a new stream.

        Args:
            name: Stream name (alphanumeric + underscore, max 256 chars)
            data_class: Data classification (PHI, NON_PHI, or DEIDENTIFIED)

        Returns:
            Stream identifier

        Raises:
            StreamAlreadyExistsError: If stream name already exists
            PermissionDeniedError: If tenant lacks permission for data class

        Example:
            >>> stream_id = client.create_stream("events", DataClass.PHI)
        """
        self._check_connected()

        stream_id = ctypes.c_uint64()
        err = _lib.kmb_client_create_stream(
            self._handle,
            name.encode('utf-8'),
            data_class.value,
            ctypes.byref(stream_id),
        )
        _check_error(err)

        return StreamId(stream_id.value)

    def append(
        self,
        stream_id: StreamId,
        events: Sequence[bytes],
        expected_offset: Offset = Offset(0),
    ) -> Offset:
        """Append events to a stream with optimistic concurrency control.

        Args:
            stream_id: Target stream identifier
            events: List of event payloads (raw bytes)
            expected_offset: Expected current stream offset for concurrency control

        Returns:
            Offset of first appended event

        Raises:
            StreamNotFoundError: If stream does not exist
            PermissionDeniedError: If write not permitted

        Example:
            >>> offset = client.append(stream_id, [
            ...     b"event1",
            ...     b"event2",
            ...     b"event3"
            ... ], expected_offset=Offset(0))
        """
        self._check_connected()

        if not events:
            raise ValueError("Cannot append empty event list")

        # Convert to C arrays
        event_count = len(events)
        event_ptrs = (ctypes.POINTER(ctypes.c_uint8) * event_count)()
        event_lengths = (ctypes.c_size_t * event_count)()

        for i, event in enumerate(events):
            event_bytes = ctypes.create_string_buffer(event)
            event_ptrs[i] = ctypes.cast(event_bytes, ctypes.POINTER(ctypes.c_uint8))
            event_lengths[i] = len(event)

        first_offset = ctypes.c_uint64()
        err = _lib.kmb_client_append(
            self._handle,
            int(stream_id),
            int(expected_offset),
            event_ptrs,
            event_lengths,
            event_count,
            ctypes.byref(first_offset),
        )
        _check_error(err)

        return Offset(first_offset.value)

    def read(
        self,
        stream_id: StreamId,
        from_offset: Offset = Offset(0),
        max_bytes: int = 1024 * 1024,  # 1 MB default
    ) -> List[Event]:
        """Read events from a stream.

        Args:
            stream_id: Source stream identifier
            from_offset: Starting offset (default: 0)
            max_bytes: Maximum bytes to read (default: 1 MB)

        Returns:
            List of events with offsets and data

        Raises:
            StreamNotFoundError: If stream does not exist
            PermissionDeniedError: If read not permitted

        Example:
            >>> events = client.read(stream_id, from_offset=0, max_bytes=1024)
            >>> for event in events:
            ...     print(f"Offset {event.offset}: {event.data}")
        """
        self._check_connected()

        result_ptr = ctypes.POINTER(KmbReadResult)()
        err = _lib.kmb_client_read_events(
            self._handle,
            int(stream_id),
            int(from_offset),
            max_bytes,
            ctypes.byref(result_ptr),
        )
        _check_error(err)

        try:
            result = result_ptr.contents
            events = []

            for i in range(result.event_count):
                # Get event data pointer and length
                event_ptr = result.events[i]
                event_len = result.event_lengths[i]

                # Copy bytes from C memory
                data = bytes(ctypes.cast(event_ptr, ctypes.POINTER(ctypes.c_uint8 * event_len)).contents)

                # Calculate offset (sequential from from_offset)
                offset = Offset(int(from_offset) + i)
                events.append(Event(offset, data))

            return events

        finally:
            # Free result
            if result_ptr:
                _lib.kmb_read_result_free(result_ptr)

    def query(self, sql: str, params: Optional[List[Value]] = None) -> QueryResult:
        """Execute a SELECT query against current state.

        Args:
            sql: SQL query string (use $1, $2, $3 for parameters)
            params: Query parameters (optional)

        Returns:
            QueryResult with columns and rows

        Raises:
            QuerySyntaxError: If SQL is invalid
            QueryExecutionError: If execution fails
            StreamNotFoundError: If queried stream does not exist

        Example:
            >>> result = client.query(
            ...     "SELECT * FROM users WHERE id = $1",
            ...     [Value.bigint(42)]
            ... )
            >>> for row in result.rows:
            ...     print(f"ID: {row[0].data}, Name: {row[1].data}")
        """
        self._check_connected()
        params = params or []

        # Convert params to FFI format
        param_count = len(params)
        if param_count > 0:
            ffi_params = (KmbQueryParam * param_count)()
            for i, param in enumerate(params):
                ffi_params[i] = self._value_to_param(param)
            params_ptr = ffi_params
        else:
            params_ptr = None

        # Call FFI
        result_ptr = ctypes.POINTER(KmbQueryResult)()
        err = _lib.kmb_client_query(
            self._handle,
            sql.encode('utf-8'),
            params_ptr,
            param_count,
            ctypes.byref(result_ptr),
        )
        _check_error(err)

        try:
            return self._parse_query_result(result_ptr.contents)
        finally:
            _lib.kmb_query_result_free(result_ptr)

    def query_at(
        self,
        sql: str,
        params: Optional[List[Value]],
        position: Offset,
    ) -> QueryResult:
        """Execute a SELECT query at a specific log position (point-in-time).

        Critical for compliance: Query historical state for audits.

        Args:
            sql: SQL query string (use $1, $2, $3 for parameters)
            params: Query parameters (optional)
            position: Log position (offset) to query at

        Returns:
            QueryResult as of that point in time

        Raises:
            QuerySyntaxError: If SQL is invalid
            QueryExecutionError: If execution fails
            PositionAheadError: If position is in the future

        Example:
            >>> # Capture current position
            >>> offset = Offset(1000)
            >>> # Query state as of that position
            >>> result = client.query_at(
            ...     "SELECT COUNT(*) FROM users",
            ...     [],
            ...     offset
            ... )
        """
        self._check_connected()
        params = params or []

        # Convert params to FFI format
        param_count = len(params)
        if param_count > 0:
            ffi_params = (KmbQueryParam * param_count)()
            for i, param in enumerate(params):
                ffi_params[i] = self._value_to_param(param)
            params_ptr = ffi_params
        else:
            params_ptr = None

        # Call FFI
        result_ptr = ctypes.POINTER(KmbQueryResult)()
        err = _lib.kmb_client_query_at(
            self._handle,
            sql.encode('utf-8'),
            params_ptr,
            param_count,
            int(position),
            ctypes.byref(result_ptr),
        )
        _check_error(err)

        try:
            return self._parse_query_result(result_ptr.contents)
        finally:
            _lib.kmb_query_result_free(result_ptr)

    def execute(self, sql: str, params: Optional[List[Value]] = None) -> int:
        """Execute DDL/DML statement (CREATE TABLE, INSERT, UPDATE, DELETE).

        Args:
            sql: SQL statement (use $1, $2, $3 for parameters)
            params: Query parameters (optional)

        Returns:
            Number of rows affected (0 for DDL)

        Raises:
            QuerySyntaxError: If SQL is invalid
            QueryExecutionError: If execution fails

        Example:
            >>> # DDL
            >>> client.execute("CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT)")
            0
            >>> # DML with parameters
            >>> client.execute(
            ...     "INSERT INTO users (id, name) VALUES ($1, $2)",
            ...     [Value.bigint(1), Value.text("Alice")]
            ... )
            1
            >>> # UPDATE with RETURNING
            >>> result = client.query(
            ...     "UPDATE users SET name = $2 WHERE id = $1 RETURNING *",
            ...     [Value.bigint(1), Value.text("Bob")]
            ... )
        """
        result = self.query(sql, params)
        # For non-SELECT queries, the row count indicates rows affected
        return len(result.rows)

    def _value_to_param(self, val: Value) -> KmbQueryParam:
        """Convert a Python Value to FFI KmbQueryParam.

        Args:
            val: Value to convert

        Returns:
            FFI query parameter structure
        """
        param = KmbQueryParam()

        if val.type == ValueType.NULL:
            param.param_type = 0  # KmbParamNull
        elif val.type == ValueType.BIGINT:
            param.param_type = 1  # KmbParamBigInt
            param.bigint_val = val.data
        elif val.type == ValueType.TEXT:
            param.param_type = 2  # KmbParamText
            param.text_val = val.data.encode('utf-8')
        elif val.type == ValueType.BOOLEAN:
            param.param_type = 3  # KmbParamBoolean
            param.bool_val = 1 if val.data else 0
        elif val.type == ValueType.TIMESTAMP:
            param.param_type = 4  # KmbParamTimestamp
            param.timestamp_val = val.data
        else:
            raise ValueError(f"Unknown value type: {val.type}")

        return param

    def _parse_query_result(self, result: KmbQueryResult) -> QueryResult:
        """Parse FFI KmbQueryResult to Python QueryResult.

        Args:
            result: FFI query result structure

        Returns:
            Python QueryResult object
        """
        # Extract columns
        columns = []
        for i in range(result.column_count):
            col_name = result.columns[i].decode('utf-8')
            columns.append(col_name)

        # Extract rows
        rows = []
        for i in range(result.row_count):
            row = []
            row_len = result.row_lengths[i]
            for j in range(row_len):
                ffi_val = result.rows[i][j]
                val = self._parse_query_value(ffi_val)
                row.append(val)
            rows.append(row)

        return QueryResult(columns, rows)

    def _parse_query_value(self, ffi_val: KmbQueryValue) -> Value:
        """Parse FFI KmbQueryValue to Python Value.

        Args:
            ffi_val: FFI query value structure

        Returns:
            Python Value object
        """
        value_type = ffi_val.value_type

        if value_type == 0:  # KmbValueNull
            return Value.null()
        elif value_type == 1:  # KmbValueBigInt
            return Value.bigint(ffi_val.bigint_val)
        elif value_type == 2:  # KmbValueText
            if ffi_val.text_val:
                text = ffi_val.text_val.decode('utf-8')
                return Value.text(text)
            return Value.null()
        elif value_type == 3:  # KmbValueBoolean
            return Value.boolean(ffi_val.bool_val != 0)
        elif value_type == 4:  # KmbValueTimestamp
            return Value.timestamp(ffi_val.timestamp_val)
        else:
            raise ValueError(f"Unknown query value type: {value_type}")
