# Python SDK Quickstart

Get started with Kimberlite in Python in under 5 minutes.

## Installation

Install via pip (requires Python 3.8+):

```bash
pip install kimberlite
```

Or for development:

```bash
cd sdks/python
pip install -e .
```

## Basic Usage

### 1. Connect to Kimberlite

```python
from kimberlite import Client, DataClass

client = Client.connect(
    addresses=["localhost:5432"],
    tenant_id=1,
    auth_token="your-token"
)
```

**Best Practice**: Use context manager for automatic cleanup:

```python
with Client.connect(
    addresses=["localhost:5432"],
    tenant_id=1,
    auth_token="your-token"
) as client:
    # Your code here
    pass
# Connection automatically closed
```

### 2. Create a Stream

```python
stream_id = client.create_stream("events", DataClass.PHI)
print(f"Created stream: {stream_id}")
```

### 3. Append Events

```python
events = [
    b'{"type": "admission", "patient_id": "P123"}',
    b'{"type": "diagnosis", "patient_id": "P123", "code": "I10"}',
    b'{"type": "discharge", "patient_id": "P123"}',
]

offset = client.append(stream_id, events)
print(f"Appended {len(events)} events starting at offset {offset}")
```

### 4. Read Events

```python
from kimberlite import Offset

events = client.read(
    stream_id,
    from_offset=Offset(0),
    max_bytes=1024 * 1024  # 1 MB
)

for event in events:
    print(f"Offset {event.offset}: {event.data.decode('utf-8')}")
```

## Complete Example

```python
#!/usr/bin/env python3
"""Complete Kimberlite Python example."""

from kimberlite import Client, DataClass, ConnectionError
import json

def main():
    try:
        with Client.connect(
            addresses=["localhost:5432"],
            tenant_id=1,
            auth_token="development-token"
        ) as client:
            # Create stream for PHI data
            stream_id = client.create_stream("patient_events", DataClass.PHI)
            print(f"✓ Created stream: {stream_id}")

            # Prepare events
            events = [
                json.dumps({
                    "type": "admission",
                    "patient_id": "P123",
                    "timestamp": "2024-01-15T10:00:00Z"
                }).encode('utf-8'),
                json.dumps({
                    "type": "diagnosis",
                    "patient_id": "P123",
                    "code": "I10",
                    "description": "Essential hypertension"
                }).encode('utf-8'),
            ]

            # Append events
            first_offset = client.append(stream_id, events)
            print(f"✓ Appended {len(events)} events at offset {first_offset}")

            # Read back
            read_events = client.read(stream_id, from_offset=first_offset)
            print(f"✓ Read {len(read_events)} events")

            for event in read_events:
                data = json.loads(event.data.decode('utf-8'))
                print(f"  {event.offset}: {data['type']}")

    except ConnectionError as e:
        print(f"Failed to connect: {e}")
        return 1

    return 0

if __name__ == "__main__":
    exit(main())
```

## Common Patterns

### Error Handling

```python
from kimberlite import (
    StreamNotFoundError,
    PermissionDeniedError,
    AuthenticationError
)

try:
    stream_id = client.create_stream("events", DataClass.PHI)
except PermissionDeniedError:
    print("No permission for PHI data")
except AuthenticationError:
    print("Authentication failed")
```

### Working with JSON

```python
import json

# Serialize to JSON
event = json.dumps({"user_id": 123, "action": "login"}).encode('utf-8')
client.append(stream_id, [event])

# Deserialize from JSON
events = client.read(stream_id)
for event in events:
    data = json.loads(event.data.decode('utf-8'))
    print(data["action"])
```

### Batch Processing

```python
# Process events in batches
batch_size = 100
offset = 0

while True:
    events = client.read(stream_id, from_offset=offset, max_bytes=1024*1024)
    if not events:
        break

    # Process batch
    for event in events:
        process_event(event)

    offset = events[-1].offset + 1
```

### Type Hints

```python
from typing import List
from kimberlite import Client, StreamId, Offset, Event

def append_logs(client: Client, stream_id: StreamId, logs: List[bytes]) -> Offset:
    """Append log entries and return first offset."""
    return client.append(stream_id, logs)

def read_recent(client: Client, stream_id: StreamId) -> List[Event]:
    """Read recent events from stream."""
    return client.read(stream_id, from_offset=0, max_bytes=1024)
```

## Testing

Use pytest for testing:

```python
import pytest
from kimberlite import Client, DataClass

@pytest.fixture
def client():
    """Provide test client."""
    with Client.connect(
        addresses=["localhost:5432"],
        tenant_id=1,
        auth_token="test"
    ) as c:
        yield c

def test_create_stream(client):
    """Test stream creation."""
    stream_id = client.create_stream("test_stream", DataClass.NON_PHI)
    assert stream_id > 0
```

## Next Steps

- [SDK Architecture](../../reference/sdk/overview.md)
- [Protocol Specification](../../reference/protocol.md)
- Python examples (coming soon)
- Type stubs included
