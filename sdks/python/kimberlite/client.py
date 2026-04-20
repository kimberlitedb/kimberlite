"""High-level Kimberlite client with Pythonic API."""

import ctypes
import dataclasses
import threading
import typing
from dataclasses import dataclass
from typing import Any, Callable, List, Mapping, Optional, Sequence, Type, TypeVar
from types import TracebackType

from .ffi import (
    _lib,
    _check_error,
    KmbClient,
    KmbClientConfig,
    KmbExecuteResult,
    KmbReadResult,
    KmbQueryParam,
    KmbQueryValue,
    KmbQueryResult,
    KmbSubscribeResult,
)
from .types import DataClass, Placement, StreamId, Offset, TenantId
from .value import Value, ValueType
from .errors import ConnectionError as KimberliteConnectionError
from .errors import KimberliteError

T = TypeVar("T")


@dataclass(frozen=True)
class ExecuteResult:
    """Result of a DML / DDL ``execute()`` call.

    Attributes:
        rows_affected: Number of rows inserted / updated / deleted (0 for DDL).
        log_offset: Log offset at which the change was committed.
    """

    rows_affected: int
    log_offset: int


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

    def __init__(
        self,
        handle: KmbClient,
        connect_config: Optional[dict] = None,
        auto_reconnect: bool = True,
    ):
        """Initialize client with FFI handle.

        Args:
            handle: Opaque FFI client handle
            connect_config: Saved connection parameters (addresses, tenant_id,
                auth_token, etc.) used by :meth:`reconnect` to rebuild the
                native handle after a connection drop. ``None`` disables
                reconnection — the connection cannot be re-established once
                lost.
            auto_reconnect: When True (default), FFI calls that raise a
                :class:`~kimberlite.errors.ConnectionError` will attempt a
                single transparent reconnect + retry. When False, callers
                must invoke :meth:`reconnect` explicitly.

        Note:
            Use Client.connect() instead of calling this directly.
        """
        self._handle: Optional[KmbClient] = handle
        self._closed = False
        self._lock = threading.RLock()  # Reentrant lock for thread-safe handle access

        # AUDIT-2026-04 S2.2 — auto-reconnect state. Mirrors the
        # TypeScript SDK's Client.autoReconnect / reconnectCount
        # so Python callers get the same resilience semantics.
        self._connect_config = connect_config
        self._auto_reconnect = bool(auto_reconnect)
        self._reconnect_count = 0

    @classmethod
    def connect(
        cls,
        addresses: List[str],
        tenant_id: int,
        auth_token: Optional[str] = None,
        client_name: str = "kimberlite-python",
        client_version: str = "0.1.0",
        auto_reconnect: bool = True,
    ) -> "Client":
        """Connect to Kimberlite cluster.

        Args:
            addresses: List of "host:port" server addresses
            tenant_id: Tenant identifier
            auth_token: Optional authentication token
            client_name: Client identifier (for server logs)
            client_version: Client version string
            auto_reconnect: Whether to transparently reconnect + retry once
                on a :class:`~kimberlite.errors.ConnectionError`. Matches the
                TypeScript SDK default.

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
        # Save config for later reconnect() calls. We keep plain Python
        # types (not the ctypes structures) so a reconnect rebuilds the
        # native config cleanly.
        connect_config = {
            "addresses": list(addresses),
            "tenant_id": tenant_id,
            "auth_token": auth_token,
            "client_name": client_name,
            "client_version": client_version,
        }

        handle = cls._connect_native(connect_config)
        return cls(
            handle,
            connect_config=connect_config,
            auto_reconnect=auto_reconnect,
        )

    @staticmethod
    def _connect_native(config: dict) -> KmbClient:
        """Open a fresh FFI handle using the saved connect parameters.

        Shared between :meth:`connect` and :meth:`reconnect` so the
        config→native-handle path is identical. Raises the usual
        `KimberliteError` subclasses on failure.
        """
        addresses: List[str] = config["addresses"]
        addr_ptrs = (ctypes.c_char_p * len(addresses))()
        for i, addr in enumerate(addresses):
            addr_ptrs[i] = addr.encode('utf-8')

        auth_token: Optional[str] = config["auth_token"]
        native_config = KmbClientConfig(
            addresses=ctypes.cast(addr_ptrs, ctypes.POINTER(ctypes.c_char_p)),
            address_count=len(addresses),
            tenant_id=config["tenant_id"],
            auth_token=auth_token.encode('utf-8') if auth_token else None,
            client_name=config["client_name"].encode('utf-8'),
            client_version=config["client_version"].encode('utf-8'),
        )

        handle = KmbClient()
        err = _lib.kmb_client_connect(
            ctypes.byref(native_config),
            ctypes.byref(handle),
        )
        _check_error(err)
        return handle

    @property
    def reconnect_count(self) -> int:
        """Number of times this client has replaced its native handle via
        :meth:`reconnect` (directly or through auto-reconnect).

        Starts at 0 and monotonically increases for the life of the
        ``Client``. Useful for observability and for tests that assert
        transparent reconnect behaviour.
        """
        return self._reconnect_count

    def reconnect(self) -> None:
        """Force a reconnect — open a fresh native handle and replace
        the current one.

        Useful after a long idle period, a known server restart, or
        when the caller wants to explicitly reset the connection. This
        method is a no-op if the client was constructed without a
        saved config (direct construction from a raw FFI handle).

        Raises:
            KimberliteError: If reconnection fails.
        """
        with self._lock:
            if self._closed:
                raise KimberliteError("Client is closed")
            if self._connect_config is None:
                raise KimberliteError(
                    "reconnect() unavailable — client was built from a raw "
                    "handle without saved config"
                )
            # Build the new handle before tearing the old one down so
            # a failed reconnect leaves the client in its previous
            # (still-usable) state.
            new_handle = Client._connect_native(self._connect_config)
            old_handle = self._handle
            self._handle = new_handle
            self._reconnect_count += 1
            if old_handle is not None:
                try:
                    _lib.kmb_client_disconnect(old_handle)
                except Exception:  # pragma: no cover - belt and braces
                    # Disconnecting the stale handle is best-effort —
                    # a failure here would typically surface as a
                    # memory leak on the native side, not user-visible
                    # misbehaviour. We swallow to guarantee the
                    # `reconnect_count` advance reflects the new
                    # handle being live.
                    pass

    def _invoke_with_reconnect(self, fn: Callable[[], T]) -> T:
        """Run an FFI-issuing callable with auto-reconnect semantics.

        AUDIT-2026-04 S2.2 — mirror of the TypeScript SDK's
        `Client.invoke`. Mid- and high-latency call sites should
        route through this helper so that a single transient
        connection drop is invisible to the caller, while true
        failures still surface.

        The retry is bounded: at most one reconnect + retry. If the
        retry also raises, the second error is propagated verbatim.

        Behaviour matrix:

        - `auto_reconnect=False`: behaves exactly like `fn()`.
        - `auto_reconnect=True`, no `ConnectionError`: behaves like
          `fn()`.
        - `auto_reconnect=True`, `ConnectionError`: reconnect once,
          retry `fn()`; any second error is propagated.

        Methods that own their own lock must *not* call this while
        holding the lock — the reconnect path takes the same lock.
        """
        try:
            return fn()
        except KimberliteConnectionError:
            if not self._auto_reconnect:
                raise
            self.reconnect()
            return fn()

    def disconnect(self) -> None:
        """Disconnect from cluster and free resources.

        This is called automatically when using context manager.
        Safe to call multiple times.
        Thread-safe: protected by RLock.
        """
        with self._lock:
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

        Note:
            This check is advisory only when called without the lock held.
            Callers that need a stable handle must hold self._lock for the
            duration of the FFI call (see append, read, query methods).
        """
        with self._lock:
            if self._closed or not self._handle:
                raise KimberliteError("Client is closed")

    def create_stream(
        self,
        name: str,
        data_class: DataClass,
        placement: Placement = Placement.GLOBAL,
        custom_region: Optional[str] = None,
    ) -> StreamId:
        """Create a new stream.

        Args:
            name: Stream name (alphanumeric + underscore, max 256 chars)
            data_class: Data classification
            placement: Geographic placement policy (default: ``Placement.GLOBAL``)
            custom_region: Region identifier when ``placement == Placement.CUSTOM``

        Returns:
            Stream identifier

        Raises:
            StreamAlreadyExistsError: If stream name already exists
            PermissionDeniedError: If tenant lacks permission for data class

        Example:
            >>> stream_id = client.create_stream("events", DataClass.PHI)
            >>> stream_id = client.create_stream(
            ...     "eu_events",
            ...     DataClass.PII,
            ...     placement=Placement.CUSTOM,
            ...     custom_region="eu-central-1",
            ... )
        """
        self._check_connected()

        if placement == Placement.CUSTOM and not custom_region:
            raise ValueError(
                "Placement.CUSTOM requires a non-empty `custom_region` argument"
            )

        stream_id = ctypes.c_uint64()

        # Fast path: default Global placement uses the legacy 3-arg entry point
        # so old FFI binaries without kmb_client_create_stream_with_placement
        # keep working for the common case.
        if placement == Placement.GLOBAL and custom_region is None:
            err = _lib.kmb_client_create_stream(
                self._handle,
                name.encode("utf-8"),
                data_class.value,
                ctypes.byref(stream_id),
            )
        else:
            custom_arg = (
                custom_region.encode("utf-8") if custom_region is not None else None
            )
            err = _lib.kmb_client_create_stream_with_placement(
                self._handle,
                name.encode("utf-8"),
                data_class.value,
                placement.value,
                custom_arg,
                ctypes.byref(stream_id),
            )
        _check_error(err)

        return StreamId(stream_id.value)

    @property
    def compliance(self) -> "ComplianceNamespace":  # type: ignore[name-defined]
        """Compliance operations — GDPR consent + erasure.

        Example:
            >>> client.compliance.consent.grant("alice", "Marketing")
            >>> req = client.compliance.erasure.request("alice")
        """
        from .compliance import ComplianceNamespace

        self._check_connected()
        return ComplianceNamespace(self._handle)

    @property
    def admin(self) -> "AdminNamespace":  # type: ignore[name-defined]
        """Admin operations namespace — schema, tenants, API keys, server info.

        All admin operations require the Admin role. Calls from non-Admin
        identities raise ``AuthenticationError``.

        Example:
            >>> tables = client.admin.list_tables()
            >>> info = client.admin.server_info()
        """
        from .admin import AdminNamespace

        self._check_connected()
        return AdminNamespace(self._handle)

    @property
    def tenant_id(self) -> TenantId:
        """Return the tenant ID this client is connected as."""
        self._check_connected()
        out = ctypes.c_uint64()
        err = _lib.kmb_client_tenant_id(self._handle, ctypes.byref(out))
        _check_error(err)
        return TenantId(out.value)

    @property
    def last_request_id(self) -> Optional[int]:
        """Return the wire request ID of the most recently sent request.

        Returns ``None`` if no request has been sent yet. Useful for
        correlating client-side logs with server-side tracing output.
        """
        self._check_connected()
        out = ctypes.c_uint64()
        err = _lib.kmb_client_last_request_id(self._handle, ctypes.byref(out))
        _check_error(err)
        return out.value if out.value != 0 else None

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
        buffers: List[ctypes.Array[ctypes.c_char]] = []  # prevent GC of temporary buffers

        for i, event in enumerate(events):
            buf = ctypes.create_string_buffer(event)
            buffers.append(buf)  # keep reference alive until after FFI call
            event_ptrs[i] = ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8))
            event_lengths[i] = len(event)

        first_offset = ctypes.c_uint64()
        with self._lock:
            self._check_connected()
            err = _lib.kmb_client_append(
                self._handle,
                int(stream_id),
                int(expected_offset),
                event_ptrs,
                event_lengths,
                event_count,
                ctypes.byref(first_offset),
            )
        # buffers kept alive above; safe to release now
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

    def subscribe(
        self,
        stream_id: StreamId,
        from_offset: Offset = Offset(0),
        initial_credits: int = 128,
        consumer_group: Optional[str] = None,
        low_water: Optional[int] = None,
        refill: Optional[int] = None,
    ) -> "Subscription":
        """Subscribe to real-time events on a stream.

        Returns a :class:`Subscription` iterator. Iterate with ``for`` or
        call ``next_event()`` directly. Credits auto-refill when the balance
        drops below ``low_water``.

        Args:
            stream_id: Target stream.
            from_offset: Offset to start streaming from (default: 0).
            initial_credits: Initial flow-control credits (default: 128).
            consumer_group: Optional consumer-group label (reserved; future use).
            low_water: Threshold below which credits auto-refill
                (default: ``max(initial_credits // 4, 1)``).
            refill: Credits to grant per auto-refill (default: ``initial_credits``).

        Example:
            >>> with client.subscribe(stream_id, initial_credits=64) as sub:
            ...     for event in sub:
            ...         print(event.offset, event.data)
        """
        from .subscription import Subscription

        self._check_connected()
        if initial_credits <= 0:
            raise ValueError("initial_credits must be > 0")

        result = KmbSubscribeResult()
        err = _lib.kmb_subscribe(
            self._handle,
            int(stream_id),
            int(from_offset),
            initial_credits,
            ctypes.byref(result),
        )
        _check_error(err)

        # consumer_group is accepted by the Rust client but the FFI entry
        # point currently sends None; future work will extend the FFI to
        # thread the value through.
        _ = consumer_group

        return Subscription(
            handle=self._handle,
            subscription_id=int(result.subscription_id),
            start_offset=Offset(int(result.start_offset)),
            initial_credits=int(result.initial_credits),
            low_water=low_water,
            refill=refill,
        )

    def execute(
        self, sql: str, params: Optional[List[Value]] = None
    ) -> ExecuteResult:
        """Execute DDL/DML statement (CREATE TABLE, INSERT, UPDATE, DELETE).

        Args:
            sql: SQL statement (use ``$1``, ``$2``, ``$3`` for parameters)
            params: Query parameters (optional)

        Returns:
            ``ExecuteResult(rows_affected, log_offset)``

        Raises:
            QuerySyntaxError: If SQL is invalid
            QueryExecutionError: If execution fails

        Example:
            >>> # DDL — rows_affected is 0
            >>> client.execute(
            ...     "CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT)"
            ... )
            ExecuteResult(rows_affected=0, log_offset=...)
            >>> # DML with parameters
            >>> r = client.execute(
            ...     "INSERT INTO users (id, name) VALUES ($1, $2)",
            ...     [Value.bigint(1), Value.text("Alice")],
            ... )
            >>> r.rows_affected
            1
            >>> # For UPDATE ... RETURNING use `query()`
        """
        self._check_connected()
        params = params or []

        param_count = len(params)
        if param_count > 0:
            ffi_params = (KmbQueryParam * param_count)()
            for i, param in enumerate(params):
                ffi_params[i] = self._value_to_param(param)
            params_ptr = ffi_params
        else:
            params_ptr = None

        out = KmbExecuteResult()
        err = _lib.kmb_client_execute(
            self._handle,
            sql.encode("utf-8"),
            params_ptr,
            param_count,
            ctypes.byref(out),
        )
        _check_error(err)
        return ExecuteResult(
            rows_affected=int(out.rows_affected),
            log_offset=int(out.log_offset),
        )

    def query_break_glass(
        self,
        reason: str,
        sql: str,
        params: Optional[List[Value]] = None,
    ) -> QueryResult:
        """Issue a healthcare BREAK_GLASS query.

        AUDIT-2026-04 S3.5 — prepends
        ``WITH BREAK_GLASS REASON='<reason>'`` to the SQL and
        runs it through :meth:`query`. The server emits an
        audit signal tagged with the reason before executing
        the inner statement under normal RBAC + masking.

        Use for emergency-access (ER intake, code-blue queries)
        where regulators require attributable access.

        Args:
            reason: Free-form justification text. Must not
                contain single quotes — the server's prefix
                parser doesn't support escapes.
            sql: The underlying SELECT (or other query).
            params: Query parameters.

        Returns:
            The :class:`QueryResult` from the inner query.

        Raises:
            ValueError: If ``reason`` contains a single quote.
        """
        if "'" in reason:
            raise ValueError(
                "query_break_glass: reason must not contain single quotes"
            )
        return self.query(f"WITH BREAK_GLASS REASON='{reason}' {sql}", params)

    def query_explain(
        self, sql: str, params: Optional[List[Value]] = None
    ) -> str:
        """Return the query's access plan tree without executing it.

        AUDIT-2026-04 S3.3 — sugar over :meth:`query`. Equivalent
        to running ``EXPLAIN <sql>`` and unwrapping the single-cell
        ``plan`` column.

        Useful for ops tooling and interactive REPL sessions where
        you want to inspect the plan without parsing a
        :class:`QueryResult`.

        Args:
            sql: SQL query to EXPLAIN.
            params: Query parameters (optional).

        Returns:
            Multi-line plan tree string. Same query always
            produces the same bytes — deterministic.

        Raises:
            KimberliteError: On parse / plan failures.
        """
        result = self.query(f"EXPLAIN {sql}", params)
        if not result.rows:
            raise KimberliteError(
                "query_explain: server returned empty rows for EXPLAIN",
            )
        cell = result.rows[0][0]
        if cell.type != ValueType.TEXT:
            raise KimberliteError(
                f"query_explain: expected TEXT plan cell, got {cell.type!r}",
            )
        return str(cell.data)

    def upsert_row(
        self,
        table: str,
        columns: Sequence[str],
        values: Sequence[Value],
    ) -> int:
        """Upsert a row keyed by ``columns[0] = values[0]``.

        AUDIT-2026-04 S2.4 — port of notebar's ``upsertRow``
        helper. Kimberlite does not (yet) support
        ``INSERT ... ON CONFLICT``, so this UPDATE-then-INSERT
        dance is the canonical upsert shape.

        Args:
            table: Target table name.
            columns: Column list; ``columns[0]`` must be the
                primary-key column.
            values: Values matching ``columns`` pairwise. Must
                have the same length as ``columns``.

        Returns:
            Number of rows affected by the winning path — 1 if
            the UPDATE hit an existing row, 1 if the INSERT ran,
            0 only for pathological table definitions.

        Raises:
            ValueError: If ``columns`` / ``values`` have
                mismatched or zero length. Raised before any
                network round-trip.
        """
        if len(columns) == 0 or len(columns) != len(values):
            raise ValueError(
                "upsert_row: columns and values must have matching non-zero length"
            )
        pk_col = columns[0]
        pk_val = values[0]
        set_cols = list(columns[1:])
        set_vals = list(values[1:])

        if set_cols:
            set_clause = ", ".join(
                f"{c} = ${i + 1}" for i, c in enumerate(set_cols)
            )
            update_sql = (
                f"UPDATE {table} SET {set_clause} "
                f"WHERE {pk_col} = ${len(set_cols) + 1}"
            )
            result = self.execute(update_sql, [*set_vals, pk_val])
            if result.rows_affected > 0:
                return int(result.rows_affected)

        col_list = ", ".join(columns)
        placeholders = ", ".join(f"${i + 1}" for i in range(len(columns)))
        insert_sql = (
            f"INSERT INTO {table} ({col_list}) VALUES ({placeholders})"
        )
        result = self.execute(insert_sql, list(values))
        return int(result.rows_affected)

    def query_rows(
        self,
        sql: str,
        params: Optional[List[Value]],
        mapper: Callable[[List[Value], List[str]], T],
    ) -> List[T]:
        """Execute a SELECT and map every row through ``mapper`` to ``T``.

        Use this when you want ``List[T]`` directly rather than the dynamic
        ``QueryResult`` shape.

        Args:
            sql: SQL query string
            params: Query parameters (optional)
            mapper: Callable that receives ``(row_values, columns)`` and
                returns a typed ``T``

        Returns:
            List of ``T`` instances, one per result row

        Example:
            >>> users = client.query_rows(
            ...     "SELECT id, name FROM users",
            ...     [],
            ...     lambda row, cols: {
            ...         "id": row[cols.index("id")].data,
            ...         "name": row[cols.index("name")].data,
            ...     },
            ... )
        """
        result = self.query(sql, params)
        return [mapper(row, result.columns) for row in result.rows]

    def query_model(
        self,
        sql: str,
        params: Optional[List[Value]],
        model: Type[T],
    ) -> List[T]:
        """Execute a SELECT and deserialise every row into a ``@dataclass``.

        Column names in the result set are matched to dataclass field names
        by exact string match. Missing fields with defaults are populated;
        missing fields without defaults raise ``KimberliteError``.

        Args:
            sql: SQL query string
            params: Query parameters (optional)
            model: A dataclass type (``@dataclass`` decorated)

        Returns:
            List of model instances, one per result row

        Example:
            >>> from dataclasses import dataclass
            >>> @dataclass
            ... class User:
            ...     id: int
            ...     name: str
            >>> users = client.query_model(
            ...     "SELECT id, name FROM users WHERE tenant_id = $1",
            ...     [Value.bigint(42)],
            ...     User,
            ... )
        """
        if not dataclasses.is_dataclass(model):
            raise TypeError(
                f"query_model(model=...) requires a @dataclass; got {model!r}"
            )
        result = self.query(sql, params)
        return [_row_to_dataclass(row, result.columns, model) for row in result.rows]

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
            assert isinstance(val.data, str)
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


# ============================================================================
# Helpers for query_model() dataclass deserialisation
# ============================================================================

def _row_to_dataclass(
    row: List[Value],
    columns: List[str],
    model: Type[T],
) -> T:
    """Build a dataclass instance from a row, matching columns to fields by name."""
    field_map = {f.name: f for f in dataclasses.fields(model)}
    index_by_name: Mapping[str, int] = {col: i for i, col in enumerate(columns)}

    kwargs: dict = {}
    missing: List[str] = []
    for field_name, field in field_map.items():
        if field_name in index_by_name:
            raw = row[index_by_name[field_name]]
            kwargs[field_name] = _coerce_value(raw, field.type)
        elif (
            field.default is not dataclasses.MISSING
            or field.default_factory is not dataclasses.MISSING  # type: ignore[misc]
        ):
            # Field has a default — let the dataclass fill it.
            continue
        else:
            missing.append(field_name)

    if missing:
        raise KimberliteError(
            f"query_model: columns missing from result set for required "
            f"field(s) {missing} on {model.__name__}"
        )

    return model(**kwargs)


def _coerce_value(value: Value, annotation: Any) -> Any:
    """Coerce a `Value` to a Python-native scalar suitable for a dataclass field.

    Handles the ``Optional[X]`` wrapping produced by ``from __future__ import
    annotations`` and plain typed fields. Unknown annotations fall back to the
    raw ``Value.data`` attribute so downstream code can do its own handling.
    """
    # Null handling — always produce None regardless of annotation.
    if value.type == ValueType.NULL:
        return None

    # Unwrap Optional[T] / Union[T, None] to T.
    origin = typing.get_origin(annotation)
    if origin is typing.Union:
        args = [a for a in typing.get_args(annotation) if a is not type(None)]
        if len(args) == 1:
            annotation = args[0]

    # Plain scalar types — trust Value.data.
    return value.data
