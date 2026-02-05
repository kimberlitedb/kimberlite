# TypeScript API Reference

TypeScript/JavaScript SDK for Kimberlite.

**Package:** `@kimberlite/client`  
**Status:** Beta (v0.4.0)  
**Node.js:** 16+

## Installation

```bash
npm install @kimberlite/client
# or
yarn add @kimberlite/client
```

## Client

### connect()

```typescript
import { Client, TenantId, StreamId } from '@kimberlite/client';

// Basic connection
const client = await Client.connect('localhost:7000');

// With authentication
const client = await Client.connect('localhost:7000', {
  tenantId: new TenantId(1),
  apiKey: 'your-api-key'
});

// With options
const client = await Client.connect('localhost:7000', {
  timeout: 30000,
  maxRetries: 3,
  compression: true
});
```

**Options:**
- `tenantId?: TenantId` - Tenant for authentication
- `apiKey?: string` - API key for authentication
- `timeout?: number` - Request timeout in ms (default: 30000)
- `maxRetries?: number` - Maximum retry attempts (default: 3)
- `compression?: boolean` - Enable compression (default: false)

### append()

```typescript
const position = await client.append(
  new TenantId(1),
  new StreamId(1, 100),
  Buffer.from('event data')
);
```

**Parameters:**
- `tenantId: TenantId` - Tenant identifier
- `streamId: StreamId` - Stream identifier
- `data: Buffer` - Event data

**Returns:** `Promise<Position>`

### appendBatch()

```typescript
const events = [
  Buffer.from('event1'),
  Buffer.from('event2'),
  Buffer.from('event3')
];
const positions = await client.appendBatch(
  new TenantId(1),
  new StreamId(1, 100),
  events
);
```

**Returns:** `Promise<Position[]>`

### readStream()

```typescript
const events = await client.readStream(
  new TenantId(1),
  new StreamId(1, 100)
);

for (const event of events) {
  console.log(`Position: ${event.position}, Data: ${event.data}`);
}
```

**Returns:** `Promise<Event[]>`

### readFromPosition()

```typescript
const events = await client.readFromPosition(
  new Position(1000),
  { limit: 100 }
);
```

**Options:**
- `limit?: number` - Maximum events to read

**Returns:** `Promise<Event[]>`

### subscribe()

```typescript
const subscription = await client.subscribe(
  new TenantId(1),
  new StreamId(1, 100)
);

for await (const event of subscription) {
  console.log(`New event: ${event.data}`);
  if (shouldStop) {
    await subscription.close();
    break;
  }
}
```

**Returns:** `AsyncIterable<Event>`

### close()

```typescript
await client.close();
```

## Types

### TenantId

```typescript
const tenant = new TenantId(1);
console.log(tenant.value);  // 1
```

### StreamId

```typescript
const stream = new StreamId(1, 100);
console.log(stream.tenantId);      // 1
console.log(stream.streamNumber);  // 100
```

### Position

```typescript
const pos = new Position(1000);
console.log(pos.value);  // 1000
```

### Event

```typescript
interface Event {
  position: Position;
  tenantId: TenantId;
  streamId: StreamId;
  timestamp: Date;
  data: Buffer;
}
```

## Error Handling

```typescript
import {
  KimberliteError,
  UnauthorizedError,
  NetworkError,
  TimeoutError
} from '@kimberlite/client';

try {
  const position = await client.append(tenant, stream, data);
} catch (error) {
  if (error instanceof UnauthorizedError) {
    console.log('Authentication failed');
  } else if (error instanceof NetworkError) {
    console.log(`Network error: ${error.message}`);
  } else if (error instanceof TimeoutError) {
    console.log('Request timed out');
  } else if (error instanceof KimberliteError) {
    console.log(`Error: ${error.message}`);
  }
}
```

## Testing

```typescript
import { MockClient } from '@kimberlite/client/testing';

test('append event', async () => {
  const client = new MockClient();
  client.expectAppend(
    new TenantId(1),
    new StreamId(1, 100),
    Buffer.from('data')
  ).returns(new Position(1));

  const position = await client.append(
    new TenantId(1),
    new StreamId(1, 100),
    Buffer.from('data')
  );

  expect(position).toEqual(new Position(1));
});
```

## Examples

See [TypeScript Quickstart](../../coding/quickstarts/typescript.md) for complete examples.
