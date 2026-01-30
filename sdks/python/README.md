# Kimberlite Python SDK

**Status**: 🚧 In Progress (Phase 11.2)

Pythonic client library for Kimberlite database.

## Installation

```bash
pip install kimberlite
```

## Quick Start

### Stream Operations

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
    results = client.read(stream_id, from_offset=0, max_bytes=1024)
    for event in results:
        print(f"Offset {event.offset}: {event.data}")
```

### SQL Queries

```python
from kimberlite import Client, Value

with Client.connect(addresses=["localhost:5432"], tenant_id=1) as client:
    # Create table
    client.execute("""
        CREATE TABLE users (
            id BIGINT PRIMARY KEY,
            name TEXT,
            email TEXT,
            active BOOLEAN,
            created_at TIMESTAMP
        )
    """)

    # Insert data with parameterized queries
    client.execute(
        "INSERT INTO users (id, name, email, active, created_at) VALUES ($1, $2, $3, $4, $5)",
        [
            Value.bigint(1),
            Value.text("Alice"),
            Value.text("alice@example.com"),
            Value.boolean(True),
            Value.timestamp(1609459200_000_000_000)  # 2021-01-01 UTC
        ]
    )

    # Query data
    result = client.query(
        "SELECT * FROM users WHERE active = $1",
        [Value.boolean(True)]
    )

    for row in result.rows:
        id_val = row[result.columns.index('id')]
        name_val = row[result.columns.index('name')]
        print(f"User {id_val.data}: {name_val.data}")

    # Point-in-time query (compliance audit)
    from kimberlite.types import Offset
    historical_offset = Offset(1000)
    historical_result = client.query_at(
        "SELECT COUNT(*) FROM users",
        [],
        historical_offset
    )
```

## Features

### Stream Operations
- Create and manage event streams
- Append events with automatic batching
- Read events with offset-based pagination
- Type hints for IDE autocomplete

### SQL Query Engine
- Full SQL support: SELECT, INSERT, UPDATE, DELETE, DDL
- Parameterized queries with type-safe Value objects
- Point-in-time queries for compliance audits
- All SQL types: NULL, BIGINT, TEXT, BOOLEAN, TIMESTAMP

### Python Integration
- Context managers for automatic resource cleanup
- Type hints and mypy strict mode support
- Rich exception hierarchy for error handling
- Pythonic API design

### Compliance Features
- Query historical state at any log position
- Immutable audit trail
- Data classification (PHI, Non-PHI, De-identified)

## Usage Examples

### Working with Value Types

```python
from kimberlite import Value
from datetime import datetime

# Create values
null_val = Value.null()
int_val = Value.bigint(42)
text_val = Value.text("Hello, 世界!")
bool_val = Value.boolean(True)
ts_val = Value.timestamp(1609459200_000_000_000)

# From Python datetime
dt = datetime(2024, 1, 1, 12, 0, 0)
ts_from_dt = Value.from_datetime(dt)

# Convert timestamp back to datetime
dt_back = ts_val.to_datetime()
print(dt_back.isoformat())  # "2021-01-01T00:00:00"
```

### CRUD Operations

```python
# CREATE
client.execute("""
    CREATE TABLE products (
        id BIGINT PRIMARY KEY,
        name TEXT,
        price BIGINT,
        in_stock BOOLEAN
    )
""")

# INSERT
client.execute(
    "INSERT INTO products (id, name, price, in_stock) VALUES ($1, $2, $3, $4)",
    [Value.bigint(1), Value.text("Widget"), Value.bigint(1999), Value.boolean(True)]
)

# UPDATE
client.execute(
    "UPDATE products SET price = $1 WHERE id = $2",
    [Value.bigint(2499), Value.bigint(1)]
)

# DELETE
client.execute(
    "DELETE FROM products WHERE id = $1",
    [Value.bigint(1)]
)

# SELECT
result = client.query("SELECT * FROM products WHERE in_stock = $1", [Value.boolean(True)])
for row in result.rows:
    print(row)
```

### Compliance Audit Example

```python
from kimberlite.types import Offset

# Record initial state
checkpoint_offset = Offset(client.log_position())  # Hypothetical API

# Make changes
client.execute("UPDATE users SET email = $1 WHERE id = $2", [
    Value.text("newemail@example.com"),
    Value.bigint(1)
])

# Later: Audit what the state was at checkpoint
historical_result = client.query_at(
    "SELECT email FROM users WHERE id = $1",
    [Value.bigint(1)],
    checkpoint_offset
)
# Returns the old email, proving what the state was at that point in time
```

## Documentation

- API Reference (coming soon)
- [Protocol Specification](../../docs/PROTOCOL.md)
- [SDK Architecture](../../docs/SDK.md)
- [Query Examples](examples/query_example.py)

## Installation (Development)

```bash
# Build FFI library
cd ../../
cargo build -p kimberlite-ffi

# Install Python SDK in development mode
cd sdks/python
pip install -e .
```

## Development Status

SDK Implementation:
- [x] ctypes-based FFI wrapper
- [x] Stream operations (create, append, read)
- [x] SQL query engine (SELECT, INSERT, UPDATE, DELETE, DDL)
- [x] Parameterized queries with Value types
- [x] Point-in-time queries (query_at)
- [x] Type hints and mypy strict mode
- [x] Comprehensive unit tests (48+ tests for values, 5+ for queries)
- [x] Integration tests
- [ ] Wheel distribution with bundled binaries
- [ ] PyPI publishing

Value Type System:
- [x] NULL, BIGINT, TEXT, BOOLEAN, TIMESTAMP
- [x] DateTime conversion helpers
- [x] Equality and hashing support
- [x] Type-safe constructors
