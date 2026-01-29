# Kimberlite TypeScript SDK

**Status**: 🚧 In Progress (Phase 11.3)

Promise-based TypeScript client for Kimberlite database.

## Installation

```bash
npm install @kimberlite/client
```

## Quick Start

```typescript
import { Client, DataClass } from '@kimberlite/client';

async function main() {
  const client = await Client.connect({
    addresses: ['localhost:5432'],
    tenantId: 1,
    authToken: 'secret'
  });

  try {
    // Create stream
    const streamId = await client.createStream('events', DataClass.PHI);

    // Append events
    const events = [
      Buffer.from('event1'),
      Buffer.from('event2'),
      Buffer.from('event3')
    ];
    const offset = await client.append(streamId, events);

    // Read events
    const results = await client.read(streamId, { fromOffset: 0, maxEvents: 100 });
    for (const event of results) {
      console.log(`Offset ${event.offset}: ${event.data}`);
    }

    // Query
    const rows = await client.query(
      'SELECT * FROM events WHERE timestamp > ?',
      [1704067200]
    );
    for (const row of rows) {
      console.log(row.id, row.data);
    }
  } finally {
    await client.disconnect();
  }
}
```

## Features

- Full TypeScript type inference (no `any`)
- Promise-based async API
- Auto-generated `.d.ts` type definitions
- Works in Node.js 18+ and Bun

## Documentation

- API Reference (coming soon)
- [Protocol Specification](../../docs/PROTOCOL.md)
- [SDK Architecture](../../docs/SDK.md)

## Development Status

Phase 11.3 deliverables:
- [ ] N-API bindings
- [ ] Promise-based async API
- [ ] Full TypeScript types
- [ ] npm package with pre-built binaries
- [ ] Unit and integration tests
- [ ] npm publishing
