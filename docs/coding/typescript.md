---
title: "TypeScript Client"
section: "coding"
slug: "typescript"
order: 2
---

# TypeScript Client

Build Kimberlite applications in TypeScript and Node.js.

## Prerequisites

- Node.js 16 or later
- npm or yarn package manager
- Running Kimberlite cluster (see [Start](/docs/start))

## Install

```bash
npm install @kimberlite/client
```

Or with yarn:

```bash
yarn add @kimberlite/client
```

## Quick Verification

Create `test.ts`:

```typescript
import { Client } from '@kimberlite/client';

console.log('Kimberlite TypeScript client imported successfully!');
```

Compile and run:

```bash
npx ts-node test.ts
```

## Sample Projects

### Basic: Create Table and Query Data

```typescript
import { Client } from '@kimberlite/client';

async function main() {
  // Connect to cluster
  const client = new Client({ addresses: ['localhost:3000'] });

  // Create table
  await client.execute(`
    CREATE TABLE users (
      id INT PRIMARY KEY,
      email TEXT NOT NULL,
      created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
  `);

  // Insert data
  await client.execute(
    'INSERT INTO users (id, email) VALUES (?, ?)',
    [1, 'alice@example.com']
  );

  // Query data
  const result = await client.query('SELECT * FROM users');
  for (const row of result) {
    console.log(`User ${row.id}: ${row.email}`);
  }

  await client.close();
}

main().catch(console.error);
```

### Compliance: Enable RBAC and Test Access Control

```typescript
import { Client, PermissionDeniedError } from '@kimberlite/client';

async function main() {
  // Connect as admin
  const adminClient = new Client({
    addresses: ['localhost:3000'],
    user: 'admin',
    password: 'admin-password'
  });

  // Create role with limited permissions
  await adminClient.execute(`
    CREATE ROLE data_analyst;
    GRANT SELECT ON patients TO data_analyst;
  `);

  // Create user with role
  await adminClient.execute(`
    CREATE USER analyst1
    WITH PASSWORD 'analyst-password'
    WITH ROLE data_analyst;
  `);

  await adminClient.close();

  // Connect as analyst
  const analystClient = new Client({
    addresses: ['localhost:3000'],
    user: 'analyst1',
    password: 'analyst-password'
  });

  // This works (SELECT granted)
  const result = await analystClient.query('SELECT * FROM patients');
  console.log(`Found ${result.length} patients`);

  // This fails (no INSERT permission)
  try {
    await analystClient.execute(
      `INSERT INTO patients VALUES (99, 'Unauthorized', '000-00-0000')`
    );
  } catch (err) {
    if (err instanceof PermissionDeniedError) {
      console.log(`Access denied: ${err.message}`);
    }
  }

  await analystClient.close();
}

main().catch(console.error);
```

### Multi-Tenant: Tenant Isolation Example

```typescript
import { Client } from '@kimberlite/client';

async function main() {
  // Connect to tenant 1
  const tenant1Client = new Client({
    addresses: ['localhost:3000'],
    tenantId: 1
  });

  // Create data for tenant 1
  await tenant1Client.execute(`
    CREATE TABLE orders (id INT, customer TEXT, amount DECIMAL);
    INSERT INTO orders VALUES (1, 'Alice', 99.99);
  `);

  // Connect to tenant 2
  const tenant2Client = new Client({
    addresses: ['localhost:3000'],
    tenantId: 2
  });

  // Tenant 2 cannot see tenant 1's data
  let result = await tenant2Client.query('SELECT * FROM orders');
  console.log(`Tenant 2 sees ${result.length} orders`); // 0

  // Create separate data for tenant 2
  await tenant2Client.execute(`INSERT INTO orders VALUES (1, 'Bob', 149.99)`);
  result = await tenant2Client.query('SELECT * FROM orders');
  console.log(`Tenant 2 sees ${result.length} orders`); // 1

  await tenant1Client.close();
  await tenant2Client.close();
}

main().catch(console.error);
```

### Compliance: Data Classification and Masking

```typescript
import { Client } from '@kimberlite/client';

async function main() {
  const client = new Client({ addresses: ['localhost:3000'] });

  // Create table with PHI data
  await client.execute(`
    CREATE TABLE patients (
      id INT PRIMARY KEY,
      name TEXT NOT NULL,
      ssn TEXT NOT NULL,
      diagnosis TEXT
    );
  `);

  // Classify sensitive columns
  await client.execute(
    `ALTER TABLE patients MODIFY COLUMN ssn SET CLASSIFICATION 'PHI'`
  );
  await client.execute(
    `ALTER TABLE patients MODIFY COLUMN diagnosis SET CLASSIFICATION 'MEDICAL'`
  );

  // Insert data
  await client.execute(`
    INSERT INTO patients VALUES
      (1, 'Alice Johnson', '123-45-6789', 'Hypertension'),
      (2, 'Bob Smith', '987-65-4321', 'Diabetes');
  `);

  // Create masking rule
  await client.execute(`CREATE MASK ssn_mask ON patients.ssn USING REDACT`);

  // Query - SSN is automatically masked
  const result = await client.query('SELECT * FROM patients');
  for (const row of result) {
    console.log(`${row.name}: SSN=${row.ssn}`); // SSN shows as ****
  }

  // View classifications
  const classifications = await client.query('SHOW CLASSIFICATIONS FOR patients');
  for (const cls of classifications) {
    console.log(`${cls.column}: ${cls.classification}`);
  }

  await client.close();
}

main().catch(console.error);
```

### Time-Travel: Query Historical State

```typescript
import { Client } from '@kimberlite/client';

async function main() {
  const client = new Client({ addresses: ['localhost:3000'] });

  // Insert initial data
  await client.execute(`
    CREATE TABLE inventory (product_id INT, quantity INT);
    INSERT INTO inventory VALUES (1, 100);
  `);

  // Wait a moment
  await new Promise(resolve => setTimeout(resolve, 2000));
  const checkpoint = new Date();

  // Update inventory
  await client.execute('UPDATE inventory SET quantity = 75 WHERE product_id = 1');

  // Query current state
  let result = await client.query('SELECT * FROM inventory WHERE product_id = 1');
  console.log(`Current quantity: ${result[0].quantity}`); // 75

  // Query historical state
  result = await client.query(
    'SELECT * FROM inventory AS OF TIMESTAMP ? WHERE product_id = 1',
    [checkpoint]
  );
  console.log(`Historical quantity: ${result[0].quantity}`); // 100

  await client.close();
}

main().catch(console.error);
```

## API Reference

### Creating a Client

```typescript
import { Client, ClientConfig } from '@kimberlite/client';

// Basic connection
const client = new Client({
  addresses: ['localhost:3000']
});

// With authentication
const clientWithAuth = new Client({
  addresses: ['localhost:3000'],
  user: 'username',
  password: 'password'
});

// With tenant isolation
const tenantClient = new Client({
  addresses: ['localhost:3000'],
  tenantId: 1
});

// With TLS
const secureClient = new Client({
  addresses: ['localhost:3000'],
  tls: {
    enabled: true,
    caCert: '/path/to/ca.pem'
  }
});

// Full configuration
const config: ClientConfig = {
  addresses: ['localhost:3000', 'localhost:3001', 'localhost:3002'],
  user: 'admin',
  password: 'password',
  tenantId: 1,
  tls: {
    enabled: true,
    caCert: '/path/to/ca.pem',
    clientCert: '/path/to/client.pem',
    clientKey: '/path/to/client-key.pem'
  },
  timeout: 5000,
  maxRetries: 3
};

const client = new Client(config);
```

### Executing Queries

```typescript
// DDL (CREATE, ALTER, DROP)
await client.execute(`
  CREATE TABLE products (
    id INT PRIMARY KEY,
    name TEXT NOT NULL,
    price DECIMAL
  )
`);

// DML (INSERT, UPDATE, DELETE)
const rowsAffected = await client.execute(
  'INSERT INTO products VALUES (?, ?, ?)',
  [1, 'Widget', 19.99]
);
console.log(`Inserted ${rowsAffected} rows`);

// Batch insert
const rows = [
  [2, 'Gadget', 29.99],
  [3, 'Doohickey', 39.99]
];
await client.executeMany('INSERT INTO products VALUES (?, ?, ?)', rows);
```

### Querying Data

```typescript
// Simple query
const result = await client.query('SELECT * FROM products');
for (const row of result) {
  console.log(`${row.name}: $${row.price}`);
}

// Parameterized query
const filtered = await client.query(
  'SELECT * FROM products WHERE price > ?',
  [25.0]
);

// Typed results with interface
interface Product {
  id: number;
  name: string;
  price: number;
}

const products = await client.query<Product>('SELECT * FROM products');
for (const product of products) {
  console.log(product.name); // TypeScript knows this is a string
}

// Streaming large results
const stream = client.queryStream('SELECT * FROM large_table');
for await (const row of stream) {
  process(row);
}
```

### Transactions

```typescript
// Explicit transaction
await client.begin();
try {
  await client.execute('UPDATE accounts SET balance = balance - 100 WHERE id = 1');
  await client.execute('UPDATE accounts SET balance = balance + 100 WHERE id = 2');
  await client.commit();
} catch (err) {
  await client.rollback();
  throw err;
}

// Transaction helper (automatic rollback on exception)
await client.transaction(async (tx) => {
  await tx.execute('UPDATE accounts SET balance = balance - 100 WHERE id = 1');
  await tx.execute('UPDATE accounts SET balance = balance + 100 WHERE id = 2');
  // Automatically committed
});
```

### Error Handling

```typescript
import {
  ConnectionError,
  AuthenticationError,
  PermissionDeniedError,
  QueryError,
  ConstraintViolationError
} from '@kimberlite/client';

try {
  const client = new Client({ addresses: ['localhost:3000'] });
  await client.execute('INSERT INTO users VALUES (1, \'alice@example.com\')');
} catch (err) {
  if (err instanceof ConnectionError) {
    console.error('Failed to connect to cluster');
  } else if (err instanceof AuthenticationError) {
    console.error('Invalid credentials');
  } else if (err instanceof PermissionDeniedError) {
    console.error('No permission for this operation');
  } else if (err instanceof ConstraintViolationError) {
    console.error(`Constraint violation: ${err.message}`);
  } else if (err instanceof QueryError) {
    console.error(`Query error: ${err.message}`);
  }
}
```

### Prepared Statements

```typescript
// Prepare statement
const stmt = await client.prepare(
  'INSERT INTO logs (timestamp, message) VALUES (?, ?)'
);

// Execute multiple times
await stmt.execute([new Date(), 'User logged in']);
await stmt.execute([new Date(), 'User logged out']);

// Batch execution
const rows = [
  [new Date(), 'Event 1'],
  [new Date(), 'Event 2'],
  [new Date(), 'Event 3']
];
await stmt.executeMany(rows);

await stmt.close();
```

### Working with Types

```typescript
// Insert with proper types
await client.execute(`
  INSERT INTO transactions (
    id,
    amount,
    timestamp,
    metadata
  ) VALUES (?, ?, ?, ?)
`, [
  1,
  99.99,
  new Date(),
  { source: 'web', ip: '192.168.1.1' }
]);

// Query with typed interface
interface Transaction {
  id: number;
  amount: number;
  timestamp: Date;
  metadata: {
    source: string;
    ip: string;
  };
}

const result = await client.query<Transaction>(
  'SELECT * FROM transactions WHERE id = 1'
);

const tx = result[0];
console.log(`Amount: ${tx.amount}`); // number
console.log(`Timestamp: ${tx.timestamp}`); // Date
console.log(`Source: ${tx.metadata.source}`); // string
```

## Testing

Use Jest or Mocha for testing with Kimberlite:

### Jest

```typescript
import { Client } from '@kimberlite/client';

describe('Kimberlite Tests', () => {
  let client: Client;

  beforeEach(async () => {
    client = new Client({ addresses: ['localhost:3000'] });

    // Clean database
    const tables = await client.query('SHOW TABLES');
    for (const table of tables) {
      await client.execute(`DROP TABLE IF EXISTS ${table.name}`);
    }
  });

  afterEach(async () => {
    await client.close();
  });

  test('create table', async () => {
    await client.execute(`
      CREATE TABLE test_table (
        id INT PRIMARY KEY,
        name TEXT NOT NULL
      )
    `);

    const tables = await client.query('SHOW TABLES');
    expect(tables.some(t => t.name === 'test_table')).toBe(true);
  });

  test('insert and query', async () => {
    await client.execute('CREATE TABLE users (id INT, email TEXT)');
    await client.execute('INSERT INTO users VALUES (1, \'test@example.com\')');

    const result = await client.query('SELECT * FROM users WHERE id = 1');
    expect(result).toHaveLength(1);
    expect(result[0].email).toBe('test@example.com');
  });
});
```

## Examples

Complete example applications are available in the repository:

- `examples/typescript/basic/` - Simple CRUD application
- `examples/typescript/compliance/` - HIPAA-compliant healthcare app
- `examples/typescript/multi-tenant/` - Multi-tenant SaaS application
- `examples/typescript/express/` - Express.js REST API
- `examples/typescript/nestjs/` - NestJS application

## Next Steps

- [Python Client](/docs/coding/python) - Python SDK
- [Rust Client](/docs/coding/rust) - Native Rust SDK
- [Go Client](/docs/coding/go) - Go SDK
- [CLI Reference](/docs/reference/cli) - Command-line tools
- [SQL Reference](/docs/reference/sql/overview) - SQL dialect

## Further Reading

- [SDK Architecture](/docs/reference/sdk/overview) - How SDKs work
- [Protocol Specification](/docs/reference/protocol) - Wire protocol details
- [Compliance Guide](/docs/concepts/compliance) - 23 frameworks explained
- [RBAC Guide](/docs/concepts/rbac) - Role-based access control
