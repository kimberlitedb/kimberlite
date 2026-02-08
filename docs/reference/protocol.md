# Kimberlite Wire Protocol Specification

**Version**: 1
**Status**: Production
**Authors**: Kimberlite Team
**Last Updated**: 2026-01-30

---

## Overview

This document specifies the Kimberlite wire protocol used for client-server communication. Third-party SDK implementers can use this specification to build clients in any programming language.

**Protocol Characteristics**:
- **Transport**: TCP with optional TLS 1.3
- **Serialization**: Bincode (compact binary format)
- **Session**: Stateful connection with handshake
- **Multiplexing**: Multiple concurrent requests over single connection
- **Ordering**: Request-response pairing via request IDs

---

## Connection Lifecycle

### 1. TCP Connection

Client establishes TCP connection to server:

```
Client                                Server
  |                                     |
  |---- TCP SYN (port 5432) ----------->|
  |<--- TCP SYN-ACK --------------------|
  |---- TCP ACK ----------------------->|
  |                                     |
  |---- TLS ClientHello (optional) ---->|
  |<--- TLS ServerHello ----------------|
  |---- TLS Finished ------------------>|
  |<--- TLS Finished -------------------|
  |                                     |
```

**Default Port**: 5432 (PostgreSQL convention for familiarity)
**TLS**: Optional (disabled by default for local dev, required for production)

### 2. Handshake

First message after connection MUST be a `Handshake` request:

```rust
struct HandshakeRequest {
    client_version: u16,        // Current: 1
    auth_token: Option<String>, // Opaque token (JWT, API key, etc.)
}
```

Server responds with `HandshakeResponse`:

```rust
struct HandshakeResponse {
    server_version: u16,        // Server protocol version
    authenticated: bool,        // Whether auth succeeded
    capabilities: Vec<String>,  // Server capabilities (e.g., "query_at", "sync")
}
```

**Authentication**:
- `auth_token` can be `None` for local development
- Production deployments should require valid JWT or API key
- Server sets `authenticated: false` if token is invalid

### 3. Request/Response Loop

After successful handshake, client sends requests and receives responses:

```
Client                                Server
  |                                     |
  |---- HandshakeRequest -------------->|
  |<--- HandshakeResponse (auth=true) --|
  |                                     |
  |---- CreateStreamRequest (id=1) ---->|
  |---- AppendEventsRequest (id=2) ---->|
  |<--- CreateStreamResponse (id=1) ----|
  |<--- AppendEventsResponse (id=2) ----|
  |                                     |
```

**Key Properties**:
- Responses may arrive out of order (use `request_id` to match)
- Client must handle concurrent responses
- Each request includes tenant context

### 4. Disconnection

Either side can close the TCP connection:
- **Graceful**: Client sends all pending requests, waits for responses, then closes
- **Abrupt**: Connection loss (network failure, server restart)

**Retries**: Client should implement exponential backoff with jitter for connection failures.

---

## Frame Format

All messages are framed with a header followed by payload:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Magic (0x56444220)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Version (u16) |       Payload Length (u32)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        CRC32 Checksum                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
|                        Payload (Bincode)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

**Field Descriptions**:

| Field | Size | Description |
|-------|------|-------------|
| Magic | 4 bytes | `0x56444220` ("VDB " in ASCII) |
| Version | 2 bytes | Protocol version (current: 1) |
| Payload Length | 4 bytes | Length of payload in bytes (max: 16 MiB) |
| CRC32 Checksum | 4 bytes | CRC32 checksum of payload |
| Payload | Variable | Bincode-serialized message |

**Constants**:
- `MAGIC`: `0x56444220`
- `MAX_PAYLOAD_SIZE`: 16 MiB (16,777,216 bytes)
- `FRAME_HEADER_SIZE`: 14 bytes

**Validation**:
1. Check magic number (reject if mismatch)
2. Check version (reject if unsupported)
3. Check payload length (reject if > MAX_PAYLOAD_SIZE)
4. Read payload bytes
5. Verify CRC32 checksum (reject if mismatch)
6. Deserialize payload with Bincode

---

## Message Types

### Request Message

```rust
struct Request {
    id: RequestId,              // Unique per connection (u64)
    tenant_id: TenantId,        // Tenant context (u64)
    payload: RequestPayload,
}

enum RequestPayload {
    Handshake(HandshakeRequest),
    CreateStream(CreateStreamRequest),
    AppendEvents(AppendEventsRequest),
    Query(QueryRequest),
    QueryAt(QueryAtRequest),
    ReadEvents(ReadEventsRequest),
    Subscribe(SubscribeRequest),
    Sync(SyncRequest),
}
```

### Response Message

```rust
struct Response {
    request_id: RequestId,      // Matches request
    payload: ResponsePayload,
}

enum ResponsePayload {
    Error(ErrorResponse),
    Handshake(HandshakeResponse),
    CreateStream(CreateStreamResponse),
    AppendEvents(AppendEventsResponse),
    Query(QueryResponse),
    QueryAt(QueryAtResponse),
    ReadEvents(ReadEventsResponse),
    Subscribe(SubscribeResponse),
    Sync(SyncResponse),
}

struct ErrorResponse {
    code: ErrorCode,            // Error code (u16)
    message: String,            // Human-readable error message
}
```

---

## Operations

### 1. Handshake

**Request**:
```rust
struct HandshakeRequest {
    client_version: u16,
    auth_token: Option<String>,
}
```

**Response**:
```rust
struct HandshakeResponse {
    server_version: u16,
    authenticated: bool,
    capabilities: Vec<String>,
}
```

**Capabilities**:
- `"query"`: Server supports SQL queries
- `"append"`: Server supports event appending
- `"subscribe"`: Server supports real-time event subscriptions
- `"query_at"`: Server supports point-in-time queries
- `"sync"`: Server supports explicit sync operations
- `"cluster"`: Server is part of a cluster (has leader/follower)

**Errors**:
- `AuthenticationFailed`: Invalid or expired token
- `UnsupportedVersion`: Client version not supported

### 2. CreateStream

**Request**:
```rust
struct CreateStreamRequest {
    name: String,               // Max 256 chars, alphanumeric + underscore
    data_class: DataClass,      // PHI, NonPHI, or Deidentified
    placement: Placement,       // Global or Regional
}

enum DataClass {
    PHI = 0,           // Protected Health Information
    NonPHI = 1,        // Non-PHI data
    Deidentified = 2,  // De-identified data
}

enum Placement {
    Global = 0,        // Data can live anywhere
    Regional = 1,      // Data must stay in specific region
}
```

**Response**:
```rust
struct CreateStreamResponse {
    stream_id: StreamId,        // The created stream ID (u64)
}
```

**Errors**:
- `StreamAlreadyExists`: Stream name already exists in tenant
- `TenantNotFound`: Tenant ID does not exist

### 3. AppendEvents

**Request**:
```rust
struct AppendEventsRequest {
    stream_id: StreamId,
    events: Vec<Vec<u8>>,       // Batch of events (opaque bytes)
}
```

**Response**:
```rust
struct AppendEventsResponse {
    first_offset: Offset,       // Offset of first event in batch (u64)
    count: u32,                 // Number of events appended
}
```

**Semantics**:
- Events are assigned sequential offsets starting at `first_offset`
- Maximum batch size: 10,000 events or 4 MiB total payload (whichever is smaller)
- Events in batch have offsets: `[first_offset, first_offset+1, ..., first_offset+count-1]`

**Errors**:
- `StreamNotFound`: Stream ID does not exist
- `InvalidRequest`: Batch too large or empty

**Note**: Optimistic concurrency control is implemented in the kernel but not yet exposed in the wire protocol. The kernel supports an `expected_offset` field in `AppendBatch` commands that validates the stream hasn't advanced before appending. This will be added to the wire protocol in a future version with error code 16 (`OffsetMismatch`). See [ROADMAP.md](../../ROADMAP.md#protocol-enhancements) for details.

### 4. Query

**Request**:
```rust
struct QueryRequest {
    sql: String,
    params: Vec<QueryParam>,
}

enum QueryParam {
    Null,
    BigInt(i64),
    Text(String),
    Boolean(bool),
    Timestamp(i64),             // Nanoseconds since Unix epoch
}
```

**Response**:
```rust
struct QueryResponse {
    columns: Vec<String>,       // Column names
    rows: Vec<Vec<QueryValue>>,
}

enum QueryValue {
    Null,
    BigInt(i64),
    Text(String),
    Boolean(bool),
    Timestamp(i64),
}
```

**SQL Dialect**: Subset of SQL-92 with DuckDB extensions
- **Supported**: SELECT, WHERE, GROUP BY, ORDER BY, LIMIT, JOINs
- **Partially Supported**: INSERT (via append-only semantics), CREATE TABLE (as stream projection)
- **Unsupported**: UPDATE, DELETE (append-only log)

**Errors**:
- `QueryParseError`: Invalid SQL syntax
- `QueryExecutionError`: Runtime error (e.g., division by zero)
- `TableNotFound`: Referenced table/view does not exist

### 5. QueryAt

**Request**:
```rust
struct QueryAtRequest {
    sql: String,
    params: Vec<QueryParam>,
    position: Offset,           // Log position to query at
}
```

**Response**:
```rust
pub type QueryAtResponse = QueryResponse;
```

**Semantics**:
- Executes query as if database state is at specified log position
- Used for point-in-time compliance queries
- Position must be at a committed transaction boundary

**Errors**:
- `PositionAhead`: Position is beyond current log head
- `ProjectionLag`: Projections not caught up to requested position (retry)
- Same errors as `Query`

### 6. ReadEvents

**Request**:
```rust
struct ReadEventsRequest {
    stream_id: StreamId,
    from_offset: Offset,
    max_bytes: u64,             // Max bytes to return (prevents OOM)
}
```

**Response**:
```rust
struct ReadEventsResponse {
    events: Vec<Vec<u8>>,       // Raw event bytes
    next_offset: Option<Offset>, // Next offset for pagination (None if at end)
}
```

**Semantics**:
- Returns events in offset order starting from `from_offset`
- Stops when `max_bytes` would be exceeded (returns fewer events if needed)
- If `from_offset` is beyond stream end, returns empty array with `next_offset: None`
- For pagination, use returned `next_offset` as `from_offset` in next request

**Errors**:
- `StreamNotFound`: Stream ID does not exist
- `InvalidOffset`: Offset is invalid (negative)

### 7. Subscribe

**Request**:
```rust
struct SubscribeRequest {
    stream_id: StreamId,
    from_offset: Offset,        // Starting offset for subscription
    initial_credits: u32,       // Credit-based flow control
    consumer_group: Option<String>, // Consumer group for coordination
}
```

**Response**:
```rust
struct SubscribeResponse {
    subscription_id: u64,       // Unique subscription identifier
    start_offset: Offset,       // Confirmed start offset
    credits: u32,               // Granted credits
}
```

**Semantics**:
- Creates a real-time subscription to a stream starting at `from_offset`
- Server validates that the stream exists before establishing the subscription
- `subscription_id` is deterministic (derived from tenant + stream) for idempotent reconnection
- Credits control flow: client requests more credits as it processes events
- Consumer groups enable coordinated consumption across multiple clients

**Errors**:
- `StreamNotFound`: Stream ID does not exist
- `InvalidOffset`: Starting offset is invalid

### 8. Sync

**Request**:
```rust
struct SyncRequest {}
```

**Response**:
```rust
struct SyncResponse {
    success: bool,
}
```

**Semantics**:
- Forces all buffered writes to disk (fsync)
- Used to ensure durability before critical operations
- Blocks until sync completes

**Errors**:
- `StorageError`: Underlying storage sync failed

---

## Error Codes

| Code | Name | Description | Retryable |
|------|------|-------------|-----------|
| 0 | `Unknown` | Unknown error | No |
| 1 | `InternalError` | Server internal error | Yes |
| 2 | `InvalidRequest` | Invalid request format or parameters | No |
| 3 | `AuthenticationFailed` | Authentication failed | No |
| 4 | `TenantNotFound` | Tenant ID does not exist | No |
| 5 | `StreamNotFound` | Stream ID does not exist | No |
| 6 | `TableNotFound` | Table/view not found in query | No |
| 7 | `QueryParseError` | Invalid SQL syntax | No |
| 8 | `QueryExecutionError` | Query runtime error | No |
| 9 | `PositionAhead` | Position beyond current log | No |
| 10 | `StreamAlreadyExists` | Stream name already exists | No |
| 11 | `InvalidOffset` | Invalid stream offset | No |
| 12 | `StorageError` | Storage layer error | Yes |
| 13 | `ProjectionLag` | Projections not caught up | Yes |
| 14 | `RateLimited` | Rate limit exceeded | Yes |
| 15 | `NotLeader` | Server is not cluster leader | Yes |

**Note on Future Error Codes**:
- Error codes 16+ are reserved for future use
- Error code 16 (`OffsetMismatch`) is planned for optimistic concurrency control (see [ROADMAP.md](../../ROADMAP.md#protocol-enhancements))

**Retry Policy**:
- **Retryable errors**: Use exponential backoff (100ms, 200ms, 400ms, ...)
- **Non-retryable errors**: Fail immediately, report to caller
- For `NotLeader`, client should discover and reconnect to leader

---

## Cluster Behavior

### Leader Discovery

Kimberlite clusters use a single-leader model:
- All writes go to leader
- Reads may go to followers (eventual consistency)

**Discovery Protocol**:
1. Client connects to any server in cluster
2. If server is not leader for write operations, it returns `NotLeader` error
3. Error message may include leader hint (e.g., "not leader, try 192.168.1.10:5432")
4. Client reconnects to leader
5. Client caches leader address for future connections

**Recommended Client Behavior**:
- Maintain connection pool with all cluster addresses
- Health-check all connections every 30 seconds
- On `NotLeader`, parse error message for leader hint
- Cache leader address for fast-path reconnection

---

## Postcard Serialization

Kimberlite uses [Postcard](https://github.com/jamesmunns/postcard) for efficient, stable binary serialization.

**Key Properties**:
- **Variable-length integers**: Efficient varint encoding (smaller payloads)
- **Stable wire format**: Guaranteed compatibility across versions
- **Zero-copy deserialization**: Minimal allocation overhead
- **Strings**: Length-prefixed (varint length + UTF-8 bytes)
- **Vectors**: Length-prefixed (varint length + elements)
- **Enums**: Discriminant (varint) + variant data
- **Option**: Discriminant (0 = None, 1 = Some) + value if Some
- **No_std compatible**: Works in constrained environments

### Example: CreateStreamRequest

```rust
CreateStreamRequest {
    name: "events",
    data_class: DataClass::PHI,
    placement: Placement::Global,
}
```

**Binary Encoding** (hex):
```
06 00 00 00 00 00 00 00  // name length (6)
65 76 65 6E 74 73        // "events" (UTF-8)
00 00 00 00              // data_class discriminant (0 = PHI)
00 00 00 00              // placement discriminant (0 = Global)
```

### Example: AppendEventsRequest

```rust
AppendEventsRequest {
    stream_id: StreamId(42),
    events: vec![
        vec![0x01, 0x02, 0x03],
        vec![0x04, 0x05],
    ],
}
```

**Binary Encoding** (hex):
```
2A 00 00 00 00 00 00 00  // stream_id (42)
02 00 00 00 00 00 00 00  // events.len (2)
03 00 00 00 00 00 00 00  // events[0].len (3)
01 02 03                 // events[0] bytes
02 00 00 00 00 00 00 00  // events[1].len (2)
04 05                    // events[1] bytes
```

### Example: Request with Tenant Context

```rust
Request {
    id: RequestId(1),
    tenant_id: TenantId(42),
    payload: RequestPayload::Handshake(HandshakeRequest {
        client_version: 1,
        auth_token: None,
    }),
}
```

**Binary Encoding** (hex):
```
01 00 00 00 00 00 00 00  // id (1)
2A 00 00 00 00 00 00 00  // tenant_id (42)
00 00 00 00              // payload discriminant (0 = Handshake)
01 00                    // client_version (1)
00 00 00 00              // auth_token discriminant (0 = None)
```

---

## Implementation Checklist

### Client Requirements

- [ ] TCP connection with optional TLS
- [ ] Frame parsing (magic, version, length, CRC32)
- [ ] Bincode deserialization
- [ ] Request ID generation (monotonic u64)
- [ ] Response matching (request_id → pending request)
- [ ] Error handling (map error codes to exceptions/errors)
- [ ] Handshake on connection
- [ ] Tenant ID management
- [ ] Leader discovery and failover (cluster mode)
- [ ] Connection pooling
- [ ] Health checks

### Server Requirements

- [ ] TCP listener with TLS support
- [ ] Handshake handling (version negotiation)
- [ ] Frame writing (header + CRC32 + payload)
- [ ] Bincode serialization
- [ ] Request routing to operation handlers
- [ ] Error response generation
- [ ] Tenant isolation
- [ ] Leader election (cluster mode)
- [ ] Rate limiting

---

## Security Considerations

### Authentication

- **Production**: REQUIRED TLS + token-based auth (JWT recommended)
- **Development**: TLS optional, token may be empty

### Authorization

- **Tenant Isolation**: All operations scoped to tenant_id in request
- **Stream Permissions**: Per-stream access control (future feature)
- **Data Class Restrictions**: PHI streams may require elevated privileges

### Transport Security

- **TLS Version**: 1.3 only (1.2 deprecated)
- **Cipher Suites**: AES-256-GCM, ChaCha20-Poly1305
- **Certificate Validation**: Client must verify server certificate

### Denial of Service

- **Rate Limiting**: Server enforces per-tenant request rate limits
- **Payload Size**: Max 16 MiB per message
- **Connection Limits**: Max 1,000 concurrent connections per tenant
- **Batch Limits**: Max 10,000 events or 4 MiB per append

---

## Versioning

### Protocol Version

Current version: **1**

**Version Negotiation**:
1. Client sends `client_version: 1` in `HandshakeRequest`
2. Server responds with `server_version: 1` in `HandshakeResponse`
3. If versions incompatible, server returns `UnsupportedVersion` error

**Backward Compatibility**:
- Minor changes (new optional fields) do not increment version
- Breaking changes (field removal, type changes) increment version
- Server may support multiple versions simultaneously

---

## Example Session

### Complete Client-Server Exchange

**1. Connect**
```
Client -> Server: TCP SYN
Server -> Client: TCP SYN-ACK
Client -> Server: TCP ACK
```

**2. Handshake**
```
Client -> Server:
  Frame Header:
    Magic: 0x56444220
    Version: 1
    Payload Length: 64
    CRC32: 0xABCD1234
  Payload (Bincode):
    Request {
      id: 1,
      tenant_id: 42,
      payload: Handshake(HandshakeRequest {
        client_version: 1,
        auth_token: Some("secret-token")
      })
    }

Server -> Client:
  Frame Header: ...
  Payload:
    Response {
      request_id: 1,
      payload: Handshake(HandshakeResponse {
        server_version: 1,
        authenticated: true,
        capabilities: ["query_at", "sync"]
      })
    }
```

**3. Create Stream**
```
Client -> Server:
  Request {
    id: 2,
    tenant_id: 42,
    payload: CreateStream(CreateStreamRequest {
      name: "events",
      data_class: DataClass::PHI,
      placement: Placement::Global
    })
  }

Server -> Client:
  Response {
    request_id: 2,
    payload: CreateStream(CreateStreamResponse {
      stream_id: StreamId(100)
    })
  }
```

**4. Append Events**
```
Client -> Server:
  Request {
    id: 3,
    tenant_id: 42,
    payload: AppendEvents(AppendEventsRequest {
      stream_id: StreamId(100),
      events: vec![b"event1".to_vec(), b"event2".to_vec()]
    })
  }

Server -> Client:
  Response {
    request_id: 3,
    payload: AppendEvents(AppendEventsResponse {
      first_offset: Offset(0),
      count: 2
    })
  }
```

**5. Query**
```
Client -> Server:
  Request {
    id: 4,
    tenant_id: 42,
    payload: Query(QueryRequest {
      sql: "SELECT COUNT(*) as count FROM events",
      params: vec![]
    })
  }

Server -> Client:
  Response {
    request_id: 4,
    payload: Query(QueryResponse {
      columns: vec!["count".to_string()],
      rows: vec![
        vec![QueryValue::BigInt(2)]
      ]
    })
  }
```

**6. Disconnect**
```
Client -> Server: TCP FIN
Server -> Client: TCP FIN-ACK
```

---

## References

- **Bincode Specification**: https://github.com/bincode-org/bincode/blob/trunk/docs/spec.md
- **CRC32 Algorithm**: Uses `crc32fast` crate (Castagnoli polynomial)
- **TLS 1.3**: RFC 8446

---

## Changelog

### Version 1.1 (2026-02-09)
- Added Subscribe operation for real-time event streaming with credit-based flow control
- Added consumer group support for coordinated subscription
- Updated handshake capabilities to advertise `"query"`, `"append"`, `"subscribe"`

### Version 1 (2026-01-30)
- Initial production protocol specification
- Core operations: Handshake, CreateStream, AppendEvents, Query, QueryAt, ReadEvents, Sync
- 16 error codes covering common failure modes
- Bincode serialization with fixed-width encoding
- Multi-tenant request structure
- Point-in-time queries via QueryAt
- Cluster support with leader discovery

---

## License

This specification is licensed under CC BY 4.0. Implementations may use any license.
