---
title: "TypeScript API Reference"
section: "reference/sdk"
slug: "typescript-api"
order: 4
---

# TypeScript API Reference

**Package:** `@kimberlitedb/client`
**Supported Node versions:** 18, 20, 22, 24 (N-API v8)
**Source:** [`sdks/typescript/`](https://github.com/kimberlitedb/kimberlite/tree/main/sdks/typescript)

For a walkthrough with runnable examples, see the
[TypeScript Guide](/docs/coding/typescript). This page is the compact
reference.

## Install

```bash
npm install @kimberlitedb/client
```

Ships prebuilt napi-rs binaries for darwin-arm64/x64, linux-x64/arm64-gnu,
and win32-x64-msvc. No `node-gyp` or Rust toolchain required.

## `Client`

### `Client.connect(config)`

Static async factory. Returns a connected `Client`.

```ts
const client: Client = await Client.connect({
  addresses: string[] | string,    // "host:port"; first used
  tenantId: bigint,
  authToken?: string,
  readTimeoutMs?: number,
  writeTimeoutMs?: number,
  bufferSizeBytes?: number,
});
```

### Instance methods

| Method | Returns | Notes |
|---|---|---|
| `client.tenantId` (getter) | `bigint` | The tenant this client is bound to. |
| `client.createStream(name, dataClass)` | `Promise<bigint>` | Stream ID. |
| `client.createStreamWithPlacement(name, dataClass, placement)` | `Promise<bigint>` | Placement: `Global` / `UsEast1` / `ApSoutheast2`. |
| `client.append(streamId, Buffer[], expectedOffset?)` | `Promise<bigint>` | First appended offset. Optimistic concurrency on `expectedOffset`. |
| `client.read(streamId, { fromOffset?, maxBytes? })` | `Promise<Event[]>` | `Event = { offset: bigint, data: Buffer }`. |
| `client.query(sql, Value[])` | `Promise<QueryResult>` | `$1, $2, ...` placeholders. |
| `client.queryAt(sql, Value[], position)` | `Promise<QueryResult>` | Time-travel query at log offset. |
| `client.execute(sql, Value[])` | `Promise<number>` | Rows affected. |
| `client.sync()` | `Promise<void>` | Flush server to disk. |
| `client.disconnect()` | `Promise<void>` | Idempotent. |

## `DataClass` enum

String-valued, mirrors the Rust `DataClass`:

```ts
enum DataClass {
  PHI = 'PHI',
  Deidentified = 'Deidentified',
  PII = 'PII',
  Sensitive = 'Sensitive',
  PCI = 'PCI',
  Financial = 'Financial',
  Confidential = 'Confidential',
  Public = 'Public',
}
```

## `Value` system

```ts
type Value =
  | { type: ValueType.Null }
  | { type: ValueType.BigInt;    value: bigint }
  | { type: ValueType.Text;      value: string }
  | { type: ValueType.Boolean;   value: boolean }
  | { type: ValueType.Timestamp; value: bigint };  // nanos since epoch
```

### Builders

| Call | Result |
|---|---|
| `ValueBuilder.null()` | NULL |
| `ValueBuilder.bigint(n)` | BIGINT (accepts `number` or `bigint`) |
| `ValueBuilder.text(s)` | TEXT |
| `ValueBuilder.boolean(b)` | BOOLEAN |
| `ValueBuilder.timestamp(nanos)` | TIMESTAMP (nanoseconds since epoch) |
| `ValueBuilder.fromDate(d)` | TIMESTAMP from a JavaScript `Date` |

### Helpers

| Function | Purpose |
|---|---|
| `valueToString(v)` | Human-readable string |
| `valueToDate(v)` | `Date` if TIMESTAMP, else `null` |
| `valueEquals(a, b)` | Structural equality |
| `isNull(v)` / `isBigInt(v)` / `isText(v)` / `isBoolean(v)` / `isTimestamp(v)` | Type guards |

## `QueryResult`

```ts
interface QueryResult {
  columns: string[];
  rows: Value[][];   // rows[i][j] is the cell in row i, column j
}
```

Cells are in column order. Use `columns.indexOf(name)` to look up by name.

## Errors

All errors extend `KimberliteError` (which extends `Error`). Pattern-match
with `instanceof`:

- `ConnectionError` — TCP connection issues, remote closed socket.
- `TimeoutError` — Read/write timeouts.
- `AuthenticationError` — Handshake or auth rejected.
- `StreamNotFoundError` — Unknown stream/table.
- `PermissionDeniedError` — RBAC/ABAC denial.
- `InternalError` — Wire protocol or generic server error.
- `ClusterUnavailableError` — No quorum.
- `KimberliteError` — Base class / catch-all.

## Point-in-time queries

`queryAt(sql, params, positionOffset)` runs the query against the log as of
a specific offset. Pair with `execute`'s affected-row count (for UPDATE) to
diff state at two points in history.

The SQL dialect also accepts inline temporal clauses:

```sql
SELECT * FROM patients AS OF TIMESTAMP '2024-01-15 10:30:00';
SELECT * FROM patients AT OFFSET 4200;
```

See [SQL Reference](/docs/reference/sql/queries).

## Supported SQL

Full list in the [SQL Reference](/docs/reference/sql/overview). Highlights
for SDK consumers:

- SELECT with WHERE / ORDER BY / LIMIT / OFFSET / GROUP BY / HAVING
- INSERT / UPDATE / DELETE with RETURNING
- INNER JOIN + LEFT JOIN (multi-table)
- CTEs (non-recursive), subqueries, UNION / UNION ALL, DISTINCT
- `CASE WHEN`, `BETWEEN`, `LIKE`, `IN`
- Parameterized queries via `$1, $2, ...`

Not supported yet (v0.4): window functions, `WITH RECURSIVE`, multi-statement
transactions with rollback.

## See also

- [TypeScript Guide](/docs/coding/typescript) — narrative walkthrough with examples
- [SDK Overview](/docs/reference/sdk/overview) — how SDKs relate to the wire protocol
- [Protocol Specification](/docs/reference/protocol)
