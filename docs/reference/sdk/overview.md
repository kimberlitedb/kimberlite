---
title: "SDK Overview"
section: "reference/sdk"
slug: "overview"
order: 1
---

# SDK Overview

Client libraries for connecting to Kimberlite from various languages.

## Supported Languages

| Language | Package | Documentation |
|----------|---------|---------------|
| **Rust** | `kimberlite` | [API Docs](/docs/reference/sdk/rust-api) |
| **Python** | `kimberlite` | [API Docs](/docs/reference/sdk/python-api) |
| **TypeScript** | `@kimberlite/client` | [API Docs](/docs/reference/sdk/typescript-api) |
| **Go** | `github.com/kimberlitedb/kimberlite-go` | [API Docs](/docs/reference/sdk/go-api) |

## Installation

### Rust

```toml
[dependencies]
kimberlite = "1.0"
```

### Python

```bash
pip install kimberlite
```

### TypeScript

```bash
npm install @kimberlite/client
# or
yarn add @kimberlite/client
```

### Go

```bash
go get github.com/kimberlitedb/kimberlite-go
```

## Quick Start

### Rust

```rust
use kimberlite::{Client, TenantId, StreamId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect
    let client = Client::connect("localhost:3000").await?;

    // Append event
    let position = client.append(
        TenantId::new(1),
        StreamId::new(1, 100),
        b"event data"
    ).await?;

    // Read events
    let events = client.read_stream(
        TenantId::new(1),
        StreamId::new(1, 100)
    ).await?;

    Ok(())
}
```

### Python

```python
from kimberlite import Client, TenantId, StreamId

# Connect
client = Client("localhost:3000")

# Append event
position = client.append(
    TenantId(1),
    StreamId(1, 100),
    b"event data"
)

# Read events
events = client.read_stream(TenantId(1), StreamId(1, 100))
```

### TypeScript

```typescript
import { Client, TenantId, StreamId } from '@kimberlite/client';

// Connect
const client = await Client.connect('localhost:3000');

// Append event
const position = await client.append(
  new TenantId(1),
  new StreamId(1, 100),
  Buffer.from('event data')
);

// Read events
const events = await client.readStream(
  new TenantId(1),
  new StreamId(1, 100)
);
```

## Core Concepts

### Client

The main entry point for all SDK operations:

```rust
// Create client
let client = Client::connect("localhost:3000").await?;

// With authentication
let client = Client::connect_with_auth(
    "localhost:3000",
    TenantId::new(1),
    "api_key"
).await?;

// With TLS
let client = Client::connect_with_tls(
    "localhost:3000",
    tls_config
).await?;
```

### TenantId

Identifies a tenant in multi-tenant deployments:

```rust
let tenant = TenantId::new(1);
```

### StreamId

Identifies a stream within a tenant:

```rust
let stream = StreamId::new(tenant_id, stream_number);
```

### Position

Log position for reading/seeking:

```rust
let position = Position::new(1000);
let events = client.read_from_position(position).await?;
```

## Common Operations

### Append Events

```rust
// Single event
let position = client.append(tenant, stream, event_data).await?;

// Batch append (more efficient)
let positions = client.append_batch(tenant, stream, events).await?;
```

### Read Events

```rust
// Read entire stream
let events = client.read_stream(tenant, stream).await?;

// Read from position
let events = client.read_from_position(position).await?;

// Read with limit
let events = client.read_stream_with_limit(tenant, stream, 100).await?;
```

### Subscribe to Events

```rust
// Subscribe to new events
let mut subscription = client.subscribe(tenant, stream).await?;

while let Some(event) = subscription.next().await {
    println!("New event: {:?}", event);
}
```

## Connection Management

### Connection Pooling

```rust
// Create client pool
let pool = ClientPool::builder()
    .max_connections(10)
    .connect("localhost:3000")
    .await?;

// Get client from pool
let client = pool.get().await?;
client.append(tenant, stream, data).await?;
```

See [Connection Pooling Guide](/docs/coding/guides/connection-pooling).

### Reconnection

All SDKs automatically reconnect on connection loss:

```rust
let client = Client::builder()
    .reconnect_policy(ReconnectPolicy::Exponential {
        min_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(60),
        max_attempts: 10,
    })
    .connect("localhost:3000")
    .await?;
```

### Timeouts

```rust
let client = Client::builder()
    .timeout(Duration::from_secs(30))
    .connect("localhost:3000")
    .await?;
```

## Error Handling

### Rust

```rust
use kimberlite::{Client, Error};

match client.append(tenant, stream, data).await {
    Ok(position) => println!("Appended at {}", position),
    Err(Error::Unauthorized) => println!("Authentication failed"),
    Err(Error::NetworkError(e)) => println!("Network error: {}", e),
    Err(e) => println!("Other error: {}", e),
}
```

### Python

```python
from kimberlite import Client, KimberliteError, UnauthorizedError

try:
    position = client.append(tenant, stream, data)
except UnauthorizedError:
    print("Authentication failed")
except KimberliteError as e:
    print(f"Error: {e}")
```

### TypeScript

```typescript
import { Client, KimberliteError, UnauthorizedError } from '@kimberlite/client';

try {
  const position = await client.append(tenant, stream, data);
} catch (e) {
  if (e instanceof UnauthorizedError) {
    console.log('Authentication failed');
  } else if (e instanceof KimberliteError) {
    console.log(`Error: ${e.message}`);
  }
}
```

## Authentication

### API Key

```rust
let client = Client::connect_with_auth(
    "localhost:3000",
    TenantId::new(1),
    "your-api-key"
).await?;
```

### TLS Client Certificates

```rust
use kimberlite::tls::{TlsConfig, ClientCert};

let tls_config = TlsConfig::builder()
    .client_cert(ClientCert::from_pem(cert_pem, key_pem)?)
    .ca_cert(ca_pem)
    .build()?;

let client = Client::connect_with_tls("localhost:3000", tls_config).await?;
```

See [Security Guide](/docs/operating/security) for authentication setup.

## Configuration

### Rust

```rust
let client = Client::builder()
    .timeout(Duration::from_secs(30))
    .max_retries(3)
    .compression(true)
    .keepalive(Duration::from_secs(60))
    .connect("localhost:3000")
    .await?;
```

### Python

```python
client = Client(
    "localhost:3000",
    timeout=30,
    max_retries=3,
    compression=True,
    keepalive=60
)
```

### TypeScript

```typescript
const client = await Client.connect('localhost:3000', {
  timeout: 30000,
  maxRetries: 3,
  compression: true,
  keepalive: 60000
});
```

## Performance

### Throughput

| Operation | Throughput | Notes |
|-----------|------------|-------|
| Append (single) | 50k/sec | Per connection |
| Append (batch) | 500k/sec | Batches of 1000 |
| Read | 100k events/sec | Sequential read |
| Subscribe | 100k events/sec | Real-time stream |

### Batching

Batch operations for higher throughput:

```rust
// ❌ Slow: Individual appends
for event in events {
    client.append(tenant, stream, event).await?;
}

// ✅ Fast: Batch append
client.append_batch(tenant, stream, events).await?;
```

### Compression

Enable compression for large payloads:

```rust
let client = Client::builder()
    .compression(true)
    .connect("localhost:3000")
    .await?;
```

**Compression savings:**
- JSON data: ~70% reduction
- Binary data: ~50% reduction
- Already compressed: <5% reduction

## Language-Specific Features

### Rust

- **Zero-copy**: Direct access to event data without copying
- **Async/await**: Full async support with Tokio
- **Type safety**: Strong typing for tenant/stream IDs

### Python

- **Async support**: `asyncio` integration
- **Type hints**: Full type annotation
- **Context managers**: Automatic connection cleanup

### TypeScript

- **Promise-based**: Modern async/await API
- **TypeScript**: Full type definitions
- **Streams**: Node.js stream integration

## Testing

All SDKs provide test helpers:

### Rust

```rust
use kimberlite::testing::MockClient;

#[tokio::test]
async fn test_append() {
    let client = MockClient::new();
    client.expect_append()
        .with(tenant, stream, data)
        .returning(|_, _, _| Ok(Position::new(1)));

    let position = client.append(tenant, stream, data).await?;
    assert_eq!(position, Position::new(1));
}
```

### Python

```python
from kimberlite.testing import MockClient

def test_append():
    client = MockClient()
    client.expect_append(tenant, stream, data).returns(Position(1))

    position = client.append(tenant, stream, data)
    assert position == Position(1)
```

See [Testing Guide](/docs/coding/guides/testing) for application testing.

## Migration

### From PostgreSQL

Replace database client with Kimberlite client:

```rust
// Before: PostgreSQL
let pool = PgPool::connect("postgresql://...").await?;
let row = sqlx::query!("SELECT * FROM users WHERE id = $1", id)
    .fetch_one(&pool)
    .await?;

// After: Kimberlite (SQL)
let client = Client::connect("localhost:3000").await?;
let row = client.query("SELECT * FROM users WHERE id = $1", &[id]).await?;
```

See [Migration Guide](/docs/coding/migration-guide).

## Related Documentation

- **[Python API](/docs/reference/sdk/python-api)** - Python-specific API reference
- **[TypeScript API](/docs/reference/sdk/typescript-api)** - TypeScript-specific API reference
- **[Rust API](/docs/reference/sdk/rust-api)** - Rust-specific API reference
- **[Go API](/docs/reference/sdk/go-api)** - Go-specific API reference
- **[Coding Guides](/docs/coding)** - Application development guides

---

**Key Takeaway:** Kimberlite SDKs provide idiomatic APIs for each language. All SDKs support append, read, and subscribe operations with automatic reconnection and connection pooling.
