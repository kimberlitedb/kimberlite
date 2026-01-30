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
)
from .types import DataClass, StreamId, Offset, TenantId
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

    def append(self, stream_id: StreamId, events: Sequence[bytes]) -> Offset:
        """Append events to a stream.

        Args:
            stream_id: Target stream identifier
            events: List of event payloads (raw bytes)

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
            ... ])
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
