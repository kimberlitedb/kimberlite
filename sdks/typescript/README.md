# Kimberlite TypeScript SDK

Promise-based TypeScript client for Kimberlite — the compliance-first database
for regulated industries.

Backed by a Rust N-API native addon (via [napi-rs](https://napi.rs)) so there's
no `node-gyp`, no Rust toolchain, and no compile step for end-users.

## Requirements

- **Node.js**: 18, 20, 22, or 24 (N-API v8, ABI-stable across all four).
- **OS**: macOS (arm64/x64), Linux (x64/arm64-gnu), Windows (x64-msvc).
  Prebuilt binaries are shipped inside the npm package.

## Installation

```bash
npm install @kimberlite/client
```

## Quick Start

### Stream Operations

```typescript
import { Client, DataClass } from '@kimberlite/client';

async function main() {
  const client = await Client.connect({
    addresses: ['localhost:5432'],
    tenantId: 1n,
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
    const results = await client.read(streamId, { fromOffset: 0n, maxBytes: 1024 });
    for (const event of results) {
      console.log(`Offset ${event.offset}: ${event.data}`);
    }
  } finally {
    await client.disconnect();
  }
}
```

### SQL Queries

```typescript
import { Client, ValueBuilder } from '@kimberlite/client';

async function queryExample() {
  const client = await Client.connect({
    addresses: ['localhost:5432'],
    tenantId: 1n
  });

  try {
    // Create table
    await client.execute(`
      CREATE TABLE users (
        id BIGINT PRIMARY KEY,
        name TEXT,
        email TEXT,
        active BOOLEAN,
        created_at TIMESTAMP
      )
    `);

    // Insert data with parameterized queries
    await client.execute(
      'INSERT INTO users (id, name, email, active, created_at) VALUES ($1, $2, $3, $4, $5)',
      [
        ValueBuilder.bigint(1),
        ValueBuilder.text('Alice'),
        ValueBuilder.text('alice@example.com'),
        ValueBuilder.boolean(true),
        ValueBuilder.timestamp(1609459200_000_000_000n) // 2021-01-01 UTC
      ]
    );

    // Query data
    const result = await client.query(
      'SELECT * FROM users WHERE active = $1',
      [ValueBuilder.boolean(true)]
    );

    for (const row of result.rows) {
      const idIdx = result.columns.indexOf('id');
      const nameIdx = result.columns.indexOf('name');
      console.log(`User ${row[idIdx]}: ${row[nameIdx]}`);
    }

    // Point-in-time query (compliance audit)
    const historicalOffset = 1000n;
    const historicalResult = await client.queryAt(
      'SELECT COUNT(*) FROM users',
      [],
      historicalOffset
    );
  } finally {
    await client.disconnect();
  }
}
```

## Features

### Stream Operations
- Create and manage event streams
- Append events with automatic batching
- Read events with offset-based pagination
- Full TypeScript type inference (no `any`)

### SQL Query Engine
- Core SQL: SELECT (aggregates, GROUP BY/HAVING, UNION, INNER/LEFT JOIN, CTEs, subqueries, window functions), INSERT, UPDATE, DELETE, DDL
- Parameterized queries with type-safe Value objects
- Point-in-time queries (`AT OFFSET`) for compliance audits; `AS OF TIMESTAMP` planned v0.6
- All SQL types: NULL, BIGINT, TEXT, BOOLEAN, TIMESTAMP

### TypeScript Integration
- Promise-based async API
- Discriminated union types for Values
- Hand-written `.d.ts` matching the napi-rs surface
- Strict mode compatible
- Works on Node.js 18, 20, 22, 24

### Compliance Features
- Query historical state at any log position
- Immutable audit trail
- Data classification across 8 types (PHI, PII, PCI, Financial, Sensitive, Deidentified, Confidential, Public)

## Usage Examples

### Working with Value Types

```typescript
import { ValueBuilder, valueToDate, valueToString } from '@kimberlite/client';

// Create values
const nullVal = ValueBuilder.null();
const intVal = ValueBuilder.bigint(42);
const textVal = ValueBuilder.text('Hello, 世界!');
const boolVal = ValueBuilder.boolean(true);
const tsVal = ValueBuilder.timestamp(1609459200_000_000_000n);

// From JavaScript Date
const date = new Date('2024-01-01T12:00:00Z');
const tsFromDate = ValueBuilder.fromDate(date);

// Convert timestamp back to Date
const dateBack = valueToDate(tsVal);
console.log(dateBack?.toISOString()); // "2021-01-01T00:00:00.000Z"

// String representation
console.log(valueToString(textVal)); // "Hello, 世界!"
```

### CRUD Operations

```typescript
// CREATE
await client.execute(`
  CREATE TABLE products (
    id BIGINT PRIMARY KEY,
    name TEXT,
    price BIGINT,
    in_stock BOOLEAN
  )
`);

// INSERT
await client.execute(
  'INSERT INTO products (id, name, price, in_stock) VALUES ($1, $2, $3, $4)',
  [
    ValueBuilder.bigint(1),
    ValueBuilder.text('Widget'),
    ValueBuilder.bigint(1999),
    ValueBuilder.boolean(true)
  ]
);

// UPDATE
await client.execute(
  'UPDATE products SET price = $1 WHERE id = $2',
  [ValueBuilder.bigint(2499), ValueBuilder.bigint(1)]
);

// DELETE
await client.execute(
  'DELETE FROM products WHERE id = $1',
  [ValueBuilder.bigint(1)]
);

// SELECT
const result = await client.query(
  'SELECT * FROM products WHERE in_stock = $1',
  [ValueBuilder.boolean(true)]
);
for (const row of result.rows) {
  console.log(row);
}
```

### Compliance Audit Example

```typescript
import { Offset } from '@kimberlite/client';

// Record initial state
const checkpointOffset: Offset = 1000n; // From previous log_position() call

// Make changes
await client.execute(
  'UPDATE users SET email = $1 WHERE id = $2',
  [ValueBuilder.text('newemail@example.com'), ValueBuilder.bigint(1)]
);

// Later: Audit what the state was at checkpoint
const historicalResult = await client.queryAt(
  'SELECT email FROM users WHERE id = $1',
  [ValueBuilder.bigint(1)],
  checkpointOffset
);
// Returns the old email, proving what the state was at that point in time
```

### Type Guards and Type Safety

```typescript
import { isBigInt, isText, ValueType } from '@kimberlite/client';

const result = await client.query('SELECT id, name FROM users');

for (const row of result.rows) {
  const idVal = row[0];
  const nameVal = row[1];

  // Type-safe access
  if (isBigInt(idVal) && isText(nameVal)) {
    console.log(`User ${idVal.value}: ${nameVal.value}`);
  }

  // Or use switch with discriminated unions
  switch (nameVal.type) {
    case ValueType.Text:
      console.log(`Name: ${nameVal.value}`);
      break;
    case ValueType.Null:
      console.log('Name: NULL');
      break;
  }
}
```

## Documentation

- API Reference (coming soon)
- [Protocol Specification](../../docs/PROTOCOL.md)
- [SDK Architecture](../../docs/SDK.md)
- [Query Examples](examples/query-example.ts)

## Installation (Development)

```bash
# From repo root
cd sdks/typescript
npm install
npm run build:native   # compiles crates/kimberlite-node for your host platform
npm run build          # tsc
npm test
```

`npm run build:native` invokes `scripts/build-native.sh`, which runs
`cargo build --release -p kimberlite-node` and copies the resulting
dynamic library into `native/kimberlite-node.<triple>.node`. In CI, the
release workflow builds this per platform (darwin-arm64, darwin-x64,
linux-x64-gnu, linux-arm64-gnu, win32-x64-msvc) and bundles all five
binaries into the published npm package.

## Architecture

```
@kimberlite/client (npm)
  └─ src/ (TS: Client, ValueBuilder, errors, types)
       └─ src/native.ts  ─►  native/index.js  ─►  kimberlite-node.<triple>.node
                                                       │
                                                       ▼ napi-rs
                                                 crates/kimberlite-node (Rust, #[napi])
                                                       │
                                                       ▼
                                                 crates/kimberlite-client (Rust RPC client)
                                                       │
                                                       ▼ TCP + VDB wire protocol
                                                 Kimberlite server
```
