# Kimberlite Python SDK

**Status**: 🚧 In Progress (Phase 11.2)

Pythonic client library for Kimberlite database.

## Installation

```bash
pip install kimberlite
```

## Quick Start

```python
from kimberlite import Client, DataClass

# Connect with context manager
with Client.connect(
    addresses=["localhost:5432"],
    tenant_id=1,
    auth_token="secret"
) as client:
    # Create stream
    stream_id = client.create_stream("events", DataClass.PHI)

    # Append events
    events = [b"event1", b"event2", b"event3"]
    offset = client.append(stream_id, events)

    # Read events
    results = client.read(stream_id, from_offset=0, max_events=100)
    for event in results:
        print(f"Offset {event.offset}: {event.data}")

    # Query
    rows = client.query(
        "SELECT * FROM events WHERE timestamp > ?",
        params=[1704067200]
    )
    for row in rows:
        print(row["id"], row["data"])
```

## Features

- Type hints for IDE autocomplete
- Context managers for resource cleanup
- Exceptions for error handling
- Generator-based iteration for large result sets

## Documentation

- API Reference (coming soon)
- [Protocol Specification](../../docs/PROTOCOL.md)
- [SDK Architecture](../../docs/SDK.md)

## Development Status

Phase 11.2 deliverables:
- [ ] ctypes-based FFI wrapper
- [ ] Type stubs (`.pyi` files)
- [ ] Wheel distribution with bundled binaries
- [ ] Unit and integration tests
- [ ] PyPI publishing
