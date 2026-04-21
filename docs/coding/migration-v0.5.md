# Migrating from v0.4.x to v0.5.0

v0.5.0 is the SDK production launch. It ships a new wire protocol (v2),
structured error types, connection pooling, real-time subscriptions,
admin operations, and GDPR compliance flows across all three SDKs —
Rust, TypeScript, and Python.

This guide covers everything you need to update.

## TL;DR

1. **Upgrade every component at once** — v0.4.0 clients cannot talk to
   v0.5.0 servers and vice versa. Coordinate the upgrade across your
   server fleet and client applications.
2. **Update SDK dependencies** to v0.5.0.
3. **Fix `execute()` call sites** — it now returns `ExecuteResult`, not
   a number.
4. **Rebuild native bindings** if you self-build the SDKs. The
   FFI's `KmbDataClass` gained 5 additional variants, additive.

## Breaking changes

### 1. Wire protocol v1 → v2

`PROTOCOL_VERSION` is now `2`. The 14-byte frame header is unchanged,
but the payload is now a `Message` enum (Request | Response | Push)
instead of a flat Request/Response pair.

**What this means for you**:

- A v0.4.0 client connecting to a v0.5.0 server gets
  `ErrorCode::InvalidRequest` with the message `"unsupported client
  version: 1, server is 2"`.
- A v0.5.0 client connecting to a v0.4.0 server gets the mirror.
- There is **no backward-compatible protocol shim**. Upgrade both
  sides in lockstep.

### 2. `execute()` return type

Previously `execute()` returned a `number` (TypeScript) / `int`
(Python) that was always 1 regardless of actual rows affected — a
pre-existing bug.

v0.5.0 returns a structured result:

| Language | Old | New |
|---|---|---|
| Rust | `execute() → ClientResult<(u64, u64)>` | unchanged — was already `(rows_affected, log_offset)` |
| TypeScript | `execute() → Promise<number>` | `execute() → Promise<ExecuteResult>` |
| Python | `execute() → int` | `execute() → ExecuteResult` |

**Fix**:

```ts
// v0.4.x
const rows = await client.execute("INSERT INTO t VALUES (1)");

// v0.5.0
const result = await client.execute("INSERT INTO t VALUES (1)");
const rows = result.rowsAffected;
```

```python
# v0.4.x
rows = client.execute("INSERT INTO t VALUES (1)")

# v0.5.0
result = client.execute("INSERT INTO t VALUES (1)")
rows = result.rows_affected
```

### 3. Error shapes

TypeScript + Python errors now carry a structured `code: ErrorCode`
field and an `isRetryable()` method. String-matching against error
messages still works but is discouraged.

**Old (fragile)**:
```ts
} catch (e) {
  if (e.message.includes("stream not found")) { /* ... */ }
}
```

**New (robust)**:
```ts
import { StreamNotFoundError } from "@kimberlitedb/client";
} catch (e) {
  if (e instanceof StreamNotFoundError) { /* ... */ }
  // Or:
  if (e.code === "StreamNotFound") { /* ... */ }
}
```

## New capabilities you can adopt incrementally

None of the following are required to upgrade, but they're why you'd
want to.

### Connection pooling

Replace single-client use with a shared `Pool`:

```ts
// v0.5.0 — TypeScript
import { Pool } from "@kimberlitedb/client";

const pool = await Pool.create({ address: "127.0.0.1:5432", tenantId: 1n, maxSize: 8 });
const result = await pool.withClient(c => c.query("SELECT 1"));
```

```python
# v0.5.0 — Python
from kimberlite import Pool

pool = Pool(address="127.0.0.1:5432", tenant_id=1, max_size=8)
with pool.acquire() as client:
    result = client.query("SELECT 1")
```

### Real-time subscribe

```ts
const sub = await client.subscribe(streamId, { initialCredits: 128 });
for await (const event of sub) {
  console.log(event.offset, event.data.toString("utf-8"));
}
```

```python
with client.subscribe(stream_id, initial_credits=128) as sub:
    for event in sub:
        print(event.offset, event.data)
```

### Admin operations

```ts
await client.admin.createTenant(42n, "acme-corp");
const info = await client.admin.serverInfo();
const { key } = await client.admin.issueApiKey({
  subject: "billing-svc",
  tenantId: 42n,
  roles: ["User"],
});
```

```python
client.admin.create_tenant(42, name="acme-corp")
info = client.admin.server_info()
result = client.admin.issue_api_key("billing-svc", tenant_id=42, roles=["User"])
```

### Consent + erasure (GDPR)

```ts
await client.compliance.consent.grant("alice", "Analytics");
if (await client.compliance.consent.check("alice", "Analytics")) {
  // ... process data ...
}

const req = await client.compliance.erasure.request("alice");
await client.compliance.erasure.complete(req.requestId);
```

```python
client.compliance.consent.grant("alice", "Analytics")
if client.compliance.consent.check("alice", "Analytics"):
    # ... process data ...
    pass

req = client.compliance.erasure.request("alice")
audit = client.compliance.erasure.complete(req.request_id)
```

## Version compatibility matrix

| Server | Client | Works? |
|---|---|---|
| 0.4.x | 0.4.x | ✅ |
| 0.4.x | 0.5.0 | ❌ — version mismatch error |
| 0.5.0 | 0.4.x | ❌ — version mismatch error |
| 0.5.0 | 0.5.0 | ✅ |

## Reference

- [SDK parity matrix](../reference/sdk/parity.md) — exhaustive
  feature-by-feature parity across Rust / TypeScript / Python.
- [CHANGELOG v0.5.0](../../CHANGELOG.md) — complete changelog.
- [Framework integration examples](../../examples/README.md) — six
  runnable examples (axum, actix, Express, Next.js, FastAPI, Django).
