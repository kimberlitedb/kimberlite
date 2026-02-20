---
title: "Python API Reference"
section: "reference/sdk"
slug: "python-api"
order: 3
---

# Python API Reference

Python SDK for Kimberlite.

**Package:** `kimberlite`
**Python:** 3.8+

## Installation

```bash
pip install kimberlite
```

## Client

### connect()

```python
from kimberlite import Client

# Basic connection
client = Client("localhost:3000")

# With authentication
client = Client(
    "localhost:3000",
    tenant_id=TenantId(1),
    api_key="your-api-key"
)

# With options
client = Client(
    "localhost:3000",
    timeout=30,
    max_retries=3,
    compression=True
)
```

**Parameters:**
- `address` (str): Server address (host:port)
- `tenant_id` (TenantId, optional): Tenant for authentication
- `api_key` (str, optional): API key for authentication
- `timeout` (int, optional): Request timeout in seconds (default: 30)
- `max_retries` (int, optional): Maximum retry attempts (default: 3)
- `compression` (bool, optional): Enable compression (default: False)

### append()

```python
position = client.append(
    tenant_id=TenantId(1),
    stream_id=StreamId(1, 100),
    data=b"event data"
)
```

**Parameters:**
- `tenant_id` (TenantId): Tenant identifier
- `stream_id` (StreamId): Stream identifier
- `data` (bytes): Event data

**Returns:** `Position` - Log position where event was appended

### append_batch()

```python
events = [b"event1", b"event2", b"event3"]
positions = client.append_batch(TenantId(1), StreamId(1, 100), events)
```

**Returns:** `List[Position]` - Positions for each event

### read_stream()

```python
events = client.read_stream(TenantId(1), StreamId(1, 100))

for event in events:
    print(f"Position: {event.position}, Data: {event.data}")
```

**Returns:** `List[Event]`

### read_from_position()

```python
events = client.read_from_position(Position(1000), limit=100)
```

**Parameters:**
- `position` (Position): Starting position
- `limit` (int, optional): Maximum events to read

**Returns:** `List[Event]`

### subscribe()

```python
subscription = client.subscribe(TenantId(1), StreamId(1, 100))

for event in subscription:
    print(f"New event: {event.data}")
    if should_stop:
        subscription.close()
        break
```

**Returns:** `Subscription` iterator

### close()

```python
client.close()
```

## Types

### TenantId

```python
from kimberlite import TenantId

tenant = TenantId(1)
print(tenant.value)  # 1
```

### StreamId

```python
from kimberlite import StreamId

stream = StreamId(tenant_id=1, stream_number=100)
print(stream.tenant_id)      # 1
print(stream.stream_number)  # 100
```

### Position

```python
from kimberlite import Position

pos = Position(1000)
print(pos.value)  # 1000
```

### Event

```python
# Event fields
event.position    # Position
event.tenant_id   # TenantId
event.stream_id   # StreamId
event.timestamp   # datetime
event.data        # bytes
```

## Async API

For async/await support:

```python
import asyncio
from kimberlite import AsyncClient

async def main():
    client = await AsyncClient.connect("localhost:3000")

    position = await client.append(
        TenantId(1),
        StreamId(1, 100),
        b"event data"
    )

    events = await client.read_stream(TenantId(1), StreamId(1, 100))

    await client.close()

asyncio.run(main())
```

## Context Manager

```python
with Client("localhost:3000") as client:
    client.append(TenantId(1), StreamId(1, 100), b"data")
# Connection automatically closed
```

## Error Handling

```python
from kimberlite import (
    KimberliteError,
    UnauthorizedError,
    NetworkError,
    TimeoutError
)

try:
    position = client.append(tenant, stream, data)
except UnauthorizedError:
    print("Authentication failed")
except NetworkError as e:
    print(f"Network error: {e}")
except TimeoutError:
    print("Request timed out")
except KimberliteError as e:
    print(f"Error: {e}")
```

## Testing

```python
from kimberlite.testing import MockClient

def test_append():
    client = MockClient()
    client.expect_append(
        TenantId(1),
        StreamId(1, 100),
        b"data"
    ).returns(Position(1))

    position = client.append(TenantId(1), StreamId(1, 100), b"data")
    assert position == Position(1)
```

## Examples

See [Python Quickstart](/docs/coding/python) for complete examples.
