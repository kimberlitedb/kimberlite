---
title: "TypeScript SDK Quickstart"
section: "coding/quickstarts"
slug: "typescript"
order: 3
---

# TypeScript SDK Quickstart

Get started with Kimberlite in TypeScript/Node.js in under 5 minutes.

## Installation

Install via npm (requires Node.js 18+):

```bash
npm install @kimberlite/client
```

Or with Bun:

```bash
bun add @kimberlite/client
```

## Basic Usage

### 1. Connect to Kimberlite

```typescript
import { Client, DataClass } from '@kimberlite/client';

const client = await Client.connect({
  addresses: ['localhost:5432'],
  tenantId: 1n,
  authToken: 'your-token'
});
```

**Best Practice**: Always disconnect when done:

```typescript
try {
  const client = await Client.connect({ /* ... */ });
  // Your code here
} finally {
  await client.disconnect();
}
```

### 2. Create a Stream

```typescript
const streamId = await client.createStream('events', DataClass.PHI);
console.log(`Created stream: ${streamId}`);
```

### 3. Append Events

```typescript
const events = [
  Buffer.from('{"type": "admission", "patient_id": "P123"}'),
  Buffer.from('{"type": "diagnosis", "patient_id": "P123", "code": "I10"}'),
  Buffer.from('{"type": "discharge", "patient_id": "P123"}'),
];

const offset = await client.append(streamId, events);
console.log(`Appended ${events.length} events at offset ${offset}`);
```

### 4. Read Events

```typescript
const events = await client.read(streamId, {
  fromOffset: 0n,
  maxBytes: 1024 * 1024 // 1 MB
});

for (const event of events) {
  console.log(`Offset ${event.offset}: ${event.data.toString('utf-8')}`);
}
```

## Complete Example

```typescript
/**
 * Complete Kimberlite TypeScript example.
 */

import { Client, DataClass, ConnectionError } from '@kimberlite/client';

async function main(): Promise<number> {
  try {
    const client = await Client.connect({
      addresses: ['localhost:5432'],
      tenantId: 1n,
      authToken: 'development-token'
    });

    try {
      // Create stream for PHI data
      const streamId = await client.createStream('patient_events', DataClass.PHI);
      console.log(`✓ Created stream: ${streamId}`);

      // Prepare events
      const events = [
        JSON.stringify({
          type: 'admission',
          patient_id: 'P123',
          timestamp: '2024-01-15T10:00:00Z'
        }),
        JSON.stringify({
          type: 'diagnosis',
          patient_id: 'P123',
          code: 'I10',
          description: 'Essential hypertension'
        }),
      ].map(s => Buffer.from(s));

      // Append events
      const firstOffset = await client.append(streamId, events);
      console.log(`✓ Appended ${events.length} events at offset ${firstOffset}`);

      // Read back
      const readEvents = await client.read(streamId, { fromOffset: firstOffset });
      console.log(`✓ Read ${readEvents.length} events`);

      for (const event of readEvents) {
        const data = JSON.parse(event.data.toString('utf-8'));
        console.log(`  ${event.offset}: ${data.type}`);
      }
    } finally {
      await client.disconnect();
    }

    return 0;
  } catch (error) {
    if (error instanceof ConnectionError) {
      console.error(`Failed to connect: ${error.message}`);
    } else {
      console.error(`Error: ${error}`);
    }
    return 1;
  }
}

main().then(code => process.exit(code));
```

## Common Patterns

### Error Handling

```typescript
import {
  StreamNotFoundError,
  PermissionDeniedError,
  AuthenticationError
} from '@kimberlite/client';

try {
  const streamId = await client.createStream('events', DataClass.PHI);
} catch (error) {
  if (error instanceof PermissionDeniedError) {
    console.error('No permission for PHI data');
  } else if (error instanceof AuthenticationError) {
    console.error('Authentication failed');
  } else {
    throw error;
  }
}
```

### Working with JSON

```typescript
interface LogEntry {
  user_id: number;
  action: string;
  timestamp: string;
}

// Serialize to JSON
const event: LogEntry = { user_id: 123, action: 'login', timestamp: new Date().toISOString() };
await client.append(streamId, [Buffer.from(JSON.stringify(event))]);

// Deserialize from JSON
const events = await client.read(streamId);
for (const event of events) {
  const data: LogEntry = JSON.parse(event.data.toString('utf-8'));
  console.log(data.action);
}
```

### Batch Processing

```typescript
// Process events in batches
let offset = 0n;

while (true) {
  const events = await client.read(streamId, {
    fromOffset: offset,
    maxBytes: 1024 * 1024
  });

  if (events.length === 0) break;

  // Process batch
  await Promise.all(events.map(event => processEvent(event)));

  offset = events[events.length - 1].offset + 1n;
}
```

### Type Safety

```typescript
import { Client, StreamId, Offset, Event, DataClass } from '@kimberlite/client';

async function appendLogs(
  client: Client,
  streamId: StreamId,
  logs: Buffer[]
): Promise<Offset> {
  return await client.append(streamId, logs);
}

async function readRecent(
  client: Client,
  streamId: StreamId
): Promise<Event[]> {
  return await client.read(streamId, { fromOffset: 0n, maxBytes: 1024 });
}
```

### With Express.js

```typescript
import express from 'express';
import { Client, DataClass } from '@kimberlite/client';

const app = express();
const client = await Client.connect({
  addresses: ['localhost:5432'],
  tenantId: 1n
});

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

process.on('SIGTERM', async () => {
  await client.disconnect();
  process.exit(0);
});
```

## Testing

Use Jest for testing:

```typescript
import { Client, DataClass } from '@kimberlite/client';

describe('Kimberlite', () => {
  let client: Client;

  beforeAll(async () => {
    client = await Client.connect({
      addresses: ['localhost:5432'],
      tenantId: 1n
    });
  });

  afterAll(async () => {
    await client.disconnect();
  });

  it('should create stream', async () => {
    const streamId = await client.createStream('test_stream', DataClass.NonPHI);
    expect(streamId).toBeGreaterThan(0n);
  });
});
```

## Next Steps

- [SDK Architecture](..//docs/reference/sdk/overview)
- [Protocol Specification](..//docs/reference/protocol)
- TypeScript examples (coming soon)
- API documentation generated from source
