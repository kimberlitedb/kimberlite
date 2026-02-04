# Connection Pooling Guide

How to efficiently manage connections to Kimberlite clusters across different SDKs.

## Overview

Connection pooling improves performance by reusing connections instead of creating new ones for each operation. This is especially important for:

- Web applications with concurrent requests
- Microservices with high throughput
- Long-running batch processors

## Python SDK

Python SDK handles connection pooling internally through the FFI layer.

### Basic Pattern

```python
from kimberlite import Client

# Create a single client instance for your application
client = Client.connect(
    addresses=["localhost:5432", "localhost:5433"],
    tenant_id=1,
    auth_token="token"
)

# Reuse this client across multiple operations
def handle_request(data):
    stream_id = client.create_stream("events", DataClass.NON_PHI)
    client.append(stream_id, [data])
```

### With Flask

```python
from flask import Flask, request
from kimberlite import Client, DataClass

app = Flask(__name__)

# Initialize client once at startup
client = None

@app.before_first_request
def init_client():
    global client
    client = Client.connect(
        addresses=["localhost:5432"],
        tenant_id=1
    )

@app.route('/events', methods=['POST'])
def post_event():
    data = request.get_json()
    stream_id = int(data['stream_id'])
    events = [e.encode('utf-8') for e in data['events']]

    offset = client.append(stream_id, events)
    return {'offset': int(offset)}

@app.teardown_appcontext
def cleanup(exc):
    if client:
        client.disconnect()
```

### With Django

```python
# settings.py
from kimberlite import Client

KIMBERLITE_CLIENT = None

def get_kimberlite_client():
    global KIMBERLITE_CLIENT
    if KIMBERLITE_CLIENT is None:
        KIMBERLITE_CLIENT = Client.connect(
            addresses=["localhost:5432"],
            tenant_id=1
        )
    return KIMBERLITE_CLIENT

# views.py
from django.conf import settings

def my_view(request):
    client = settings.get_kimberlite_client()
    # Use client...
```

## TypeScript SDK

TypeScript SDK manages connections at the client level.

### Basic Pattern

```typescript
import { Client, DataClass } from '@kimberlite/client';

// Create client once
const client = await Client.connect({
  addresses: ['localhost:5432', 'localhost:5433'],
  tenantId: 1n,
  authToken: 'token'
});

// Reuse across operations
async function handleRequest(data: Buffer) {
  const streamId = await client.createStream('events', DataClass.NonPHI);
  await client.append(streamId, [data]);
}
```

### With Express.js

```typescript
import express from 'express';
import { Client } from '@kimberlite/client';

const app = express();
let client: Client;

// Initialize on startup
async function init() {
  client = await Client.connect({
    addresses: ['localhost:5432'],
    tenantId: 1n
  });
}

app.post('/events', async (req, res) => {
  try {
    const streamId = BigInt(req.body.stream_id);
    const events = req.body.events.map((e: string) => Buffer.from(e));
    const offset = await client.append(streamId, events);
    res.json({ offset: offset.toString() });
  } catch (error) {
    res.status(500).json({ error: (error as Error).message });
  }
});

// Graceful shutdown
process.on('SIGTERM', async () => {
  await client.disconnect();
  process.exit(0);
});

init().then(() => {
  app.listen(3000);  // Express.js app port (NOT Kimberlite server - that's :5432)
});
```

### With NestJS

```typescript
import { Injectable, OnModuleInit, OnModuleDestroy } from '@nestjs/common';
import { Client } from '@kimberlite/client';

@Injectable()
export class KimberliteService implements OnModuleInit, OnModuleDestroy {
  private client: Client;

  async onModuleInit() {
    this.client = await Client.connect({
      addresses: ['localhost:5432'],
      tenantId: 1n
    });
  }

  async onModuleDestroy() {
    await this.client.disconnect();
  }

  async append(streamId: bigint, events: Buffer[]) {
    return await this.client.append(streamId, events);
  }
}
```

## Rust SDK

Rust SDK uses synchronous connections with Send + Sync for thread safety.

### Basic Pattern

```rust
use kimberlite::Kimberlite;
use kmb_types::TenantId;
use std::sync::Arc;

// Create once and share via Arc
let db = Arc::new(Kimberlite::open("./data")?);
let tenant = db.tenant(TenantId::new(1));

// Clone Arc for each thread
let tenant_clone = tenant.clone();
std::thread::spawn(move || {
    tenant_clone.append(stream_id, events)?;
});
```

### With Actix Web

```rust
use actix_web::{web, App, HttpServer};
use kimberlite::Kimberlite;
use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize once
    let db = Arc::new(Kimberlite::open("./data").unwrap());

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone()))
            .route("/events", web::post().to(post_event))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}

async fn post_event(
    db: web::Data<Arc<Kimberlite>>,
    req: web::Json<EventRequest>,
) -> impl Responder {
    let tenant = db.tenant(TenantId::new(1));
    let offset = tenant.append(req.stream_id, req.events.clone())?;
    web::Json(json!({ "offset": offset }))
}
```

## Best Practices

### 1. Single Client per Application

Create one client instance and reuse it:

```python
# ✅ GOOD
client = Client.connect(...)
for i in range(1000):
    client.append(stream_id, [data])

# ❌ BAD
for i in range(1000):
    client = Client.connect(...)
    client.append(stream_id, [data])
    client.disconnect()
```

### 2. Graceful Shutdown

Always disconnect on application shutdown:

```typescript
process.on('SIGTERM', async () => {
  await client.disconnect();
  process.exit(0);
});
```

### 3. Health Checks

Implement periodic health checks:

```python
import threading
import time

def health_check():
    while True:
        try:
            # Ping operation
            client.read(health_stream_id, from_offset=0, max_bytes=1)
        except Exception as e:
            logger.error(f"Health check failed: {e}")
            # Reconnect logic here
        time.sleep(30)

threading.Thread(target=health_check, daemon=True).start()
```

### 4. Error Recovery

Implement retry logic with exponential backoff:

```typescript
async function withRetry<T>(
  fn: () => Promise<T>,
  maxRetries = 3
): Promise<T> {
  let lastError;
  for (let i = 0; i < maxRetries; i++) {
    try {
      return await fn();
    } catch (error) {
      lastError = error;
      await new Promise(resolve => setTimeout(resolve, Math.pow(2, i) * 100));
    }
  }
  throw lastError;
}

const offset = await withRetry(() => client.append(streamId, events));
```

## Multi-Cluster Setup

For high availability, connect to multiple cluster addresses:

```python
client = Client.connect(
    addresses=[
        "cluster1.example.com:5432",
        "cluster2.example.com:5432",
        "cluster3.example.com:5432",
    ],
    tenant_id=1
)
```

The client will automatically:
- Discover the cluster leader
- Failover to a new leader if needed
- Retry on transient failures

## Performance Tips

1. **Batch Operations**: Group multiple appends into single calls
2. **Concurrent Reads**: Multiple read operations can run in parallel
3. **Connection Limits**: Don't create more clients than needed
4. **Keep-Alive**: FFI layer maintains persistent connections
5. **Resource Cleanup**: Always disconnect when done

## Monitoring

Track connection metrics:

```python
import time

class MonitoredClient:
    def __init__(self, client):
        self.client = client
        self.operations = 0
        self.errors = 0
        self.start_time = time.time()

    def append(self, stream_id, events):
        try:
            result = self.client.append(stream_id, events)
            self.operations += 1
            return result
        except Exception as e:
            self.errors += 1
            raise

    def stats(self):
        elapsed = time.time() - self.start_time
        return {
            'operations': self.operations,
            'errors': self.errors,
            'ops_per_sec': self.operations / elapsed if elapsed > 0 else 0,
            'error_rate': self.errors / self.operations if self.operations > 0 else 0
        }
```

## See Also

- [Quickstart Guide - Python](quickstart-python.md)
- [Quickstart Guide - TypeScript](quickstart-typescript.md)
- [Protocol Specification](../PROTOCOL.md)
