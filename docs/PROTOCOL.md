# Kimberlite Wire Protocol Specification

**Version**: 0.1.0
**Status**: Draft
**Authors**: Kimberlite Team
**Last Updated**: 2026-01-30

---

## Overview

This document specifies the Kimberlite wire protocol used for client-server communication. Third-party SDK implementers can use this specification to build clients in any programming language without relying on the official FFI library.

**Protocol Characteristics**:
- **Transport**: TCP with optional TLS 1.3
- **Serialization**: Bincode (MessagePack-like binary format)
- **Session**: Stateful connection with authentication
- **Multiplexing**: Multiple concurrent requests over single connection
- **Ordering**: Per-stream FIFO, cross-stream unordered

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

### 2. Authentication

First message after connection MUST be an `AuthRequest`:

```rust
struct AuthRequest {
    protocol_version: u32,  // Current: 1
    tenant_id: u64,
    auth_token: String,     // Opaque token (JWT, API key, etc.)
    client_info: ClientInfo,
}

struct ClientInfo {
    name: String,           // "kimberlite-python", "kimberlite-go", etc.
    version: String,        // SDK version
}
```

Server responds with `AuthResponse`:

```rust
enum AuthResponse {
    Success {
        cluster_id: u128,
        leader_hint: Option<SocketAddr>,
    },
    Failure {
        error_code: ErrorCode,
        message: String,
    },
}
```

**Error Codes**:
- `TENANT_NOT_FOUND`: Invalid tenant ID
- `AUTH_FAILED`: Invalid or expired token
- `PROTOCOL_VERSION_MISMATCH`: Unsupported protocol version

### 3. Request/Response Loop

After successful authentication, client sends requests and receives responses:

```
Client                                Server
  |                                     |
  |---- AuthRequest ------------------->|
  |<--- AuthResponse (Success) ---------|
  |                                     |
  |---- CreateStreamRequest (id=1) ---->|
  |---- AppendRequest (id=2) ---------->|
  |<--- CreateStreamResponse (id=1) ----|
  |<--- AppendResponse (id=2) ----------|
  |                                     |
```

**Key Properties**:
- Responses may arrive out of order (use `request_id` to match)
- Client must handle concurrent responses
- No limit on concurrent requests (server-side concurrency control)

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
|                        Magic (0x4B4D4200)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Version (u16) |  Reserved     |       Payload Length (u32)    |
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
| Magic | 4 bytes | `0x4B4D4200` ("KMB\0" in ASCII) |
| Version | 2 bytes | Protocol version (current: 1) |
| Reserved | 2 bytes | Reserved for future use (must be 0) |
| Payload Length | 4 bytes | Length of payload in bytes (max: 16 MB) |
| CRC32 Checksum | 4 bytes | CRC32C checksum of payload (IEEE polynomial) |
| Payload | Variable | Bincode-serialized message |

**Constants**:
- `MAX_PAYLOAD_SIZE`: 16 MB (16,777,216 bytes)
- `HEADER_SIZE`: 16 bytes

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
    request_id: u64,        // Unique per connection, monotonic increment
    operation: Operation,
}

enum Operation {
    CreateStream(CreateStreamRequest),
    Append(AppendRequest),
    Read(ReadRequest),
    Query(QueryRequest),
    Subscribe(SubscribeRequest),
    Checkpoint(CheckpointRequest),
    DeleteStream(DeleteStreamRequest),
}
```

### Response Message

```rust
struct Response {
    request_id: u64,        // Matches request
    result: Result<OperationResult, Error>,
}

enum OperationResult {
    CreateStream { stream_id: u64 },
    Append { first_offset: u64 },
    Read { events: Vec<Event> },
    Query { rows: Vec<Row> },
    Subscribe { subscription_id: u64 },
    Checkpoint { checkpoint_id: u128 },
    DeleteStream { deleted: bool },
}

struct Error {
    code: ErrorCode,
    message: String,
    metadata: HashMap<String, String>,  // Optional context
}
```

---

## Operations

### 1. CreateStream

**Request**:
```rust
struct CreateStreamRequest {
    name: String,               // Max 256 chars, alphanumeric + underscore
    data_class: DataClass,
    retention_days: Option<u32>, // None = infinite retention
}

enum DataClass {
    PHI = 0,           // Protected Health Information
    NonPHI = 1,        // Non-PHI data
    Deidentified = 2,  // De-identified data
}
```

**Response**:
```rust
struct CreateStreamResponse {
    stream_id: u64,
}
```

**Errors**:
- `STREAM_ALREADY_EXISTS`: Stream name already exists in tenant
- `INVALID_DATA_CLASS`: Unknown data class value
- `PERMISSION_DENIED`: Tenant lacks permission for data class

### 2. Append

**Request**:
```rust
struct AppendRequest {
    stream_id: u64,
    events: Vec<Vec<u8>>,       // Batch of events (opaque bytes)
    expect_offset: Option<u64>, // Optimistic concurrency control
}
```

**Response**:
```rust
struct AppendResponse {
    first_offset: u64,  // Offset of first event in batch
}
```

**Semantics**:
- Events are assigned sequential offsets starting at `first_offset`
- If `expect_offset` is provided, server rejects if current stream offset != expected
- Maximum batch size: 10,000 events or 4 MB total payload (whichever is smaller)

**Errors**:
- `STREAM_NOT_FOUND`: Stream ID does not exist
- `OFFSET_MISMATCH`: `expect_offset` does not match current offset
- `BATCH_TOO_LARGE`: Exceeds max batch size

### 3. Read

**Request**:
```rust
struct ReadRequest {
    stream_id: u64,
    from_offset: u64,
    max_events: u32,            // Max: 10,000
}
```

**Response**:
```rust
struct ReadResponse {
    events: Vec<Event>,
}

struct Event {
    offset: u64,
    data: Vec<u8>,
    timestamp: i64,             // Unix timestamp (microseconds)
}
```

**Semantics**:
- Returns events in offset order
- If `from_offset` is beyond current stream end, returns empty array
- Returns up to `max_events` (may return fewer if stream ends)

**Errors**:
- `STREAM_NOT_FOUND`: Stream ID does not exist
- `PERMISSION_DENIED`: Caller lacks read permission

### 4. Query

**Request**:
```rust
struct QueryRequest {
    sql: String,
    params: Vec<QueryParam>,
}

enum QueryParam {
    Null,
    Int64(i64),
    String(String),
    Bytes(Vec<u8>),
    Bool(bool),
}
```

**Response**:
```rust
struct QueryResponse {
    columns: Vec<ColumnDef>,
    rows: Vec<Vec<QueryValue>>,
}

struct ColumnDef {
    name: String,
    type_name: String,          // "INT64", "STRING", "BYTES", "BOOL"
}

enum QueryValue {
    Null,
    Int64(i64),
    String(String),
    Bytes(Vec<u8>),
    Bool(bool),
}
```

**SQL Dialect**: Subset of SQL-92 with DuckDB extensions
- **Supported**: SELECT, WHERE, GROUP BY, ORDER BY, LIMIT, JOINs
- **Unsupported**: INSERT, UPDATE, DELETE, DDL (use stream operations instead)

**Errors**:
- `QUERY_SYNTAX_ERROR`: Invalid SQL syntax
- `QUERY_EXECUTION_ERROR`: Runtime error (e.g., division by zero)
- `PERMISSION_DENIED`: Query accesses forbidden streams

### 5. Subscribe

**Request**:
```rust
struct SubscribeRequest {
    stream_id: u64,
    from_offset: u64,
}
```

**Response (initial)**:
```rust
struct SubscribeResponse {
    subscription_id: u64,
}
```

**Event Stream**:
After initial response, server sends `SubscriptionEvent` messages:

```rust
struct SubscriptionEvent {
    subscription_id: u64,
    events: Vec<Event>,
}
```

**Semantics**:
- Server pushes new events as they are appended
- Client does not send additional requests (server-initiated push)
- Subscription remains active until client disconnects or sends `Unsubscribe`

**Errors**:
- `STREAM_NOT_FOUND`: Stream ID does not exist
- `PERMISSION_DENIED`: Caller lacks read permission

### 6. Checkpoint

**Request**:
```rust
struct CheckpointRequest {
    tenant_id: u64,
}
```

**Response**:
```rust
struct CheckpointResponse {
    checkpoint_id: u128,        // Unique checkpoint ID
    timestamp: i64,             // Unix timestamp (microseconds)
}
```

**Semantics**:
- Creates immutable snapshot of all tenant streams
- Used for point-in-time queries and compliance audits
- Checkpoint persists for tenant's retention period

**Errors**:
- `PERMISSION_DENIED`: Caller lacks checkpoint permission
- `CHECKPOINT_IN_PROGRESS`: Previous checkpoint not yet complete

### 7. DeleteStream

**Request**:
```rust
struct DeleteStreamRequest {
    stream_id: u64,
}
```

**Response**:
```rust
struct DeleteStreamResponse {
    deleted: bool,
}
```

**Semantics**:
- Marks stream as deleted (not immediate physical deletion)
- Stream data retained for compliance period
- Deleted streams cannot be read or appended

**Errors**:
- `STREAM_NOT_FOUND`: Stream ID does not exist
- `PERMISSION_DENIED`: Caller lacks delete permission

---

## Error Codes

| Code | Name | Description | Retryable |
|------|------|-------------|-----------|
| 0 | `OK` | Success | N/A |
| 1 | `INTERNAL_ERROR` | Server internal error | Yes |
| 2 | `STREAM_NOT_FOUND` | Stream ID does not exist | No |
| 3 | `STREAM_ALREADY_EXISTS` | Stream name already exists | No |
| 4 | `TENANT_NOT_FOUND` | Tenant ID does not exist | No |
| 5 | `AUTH_FAILED` | Authentication failed | No |
| 6 | `PERMISSION_DENIED` | Operation not permitted | No |
| 7 | `INVALID_DATA_CLASS` | Unknown data class | No |
| 8 | `OFFSET_OUT_OF_RANGE` | Offset beyond stream end | No |
| 9 | `OFFSET_MISMATCH` | Optimistic concurrency failure | Yes |
| 10 | `BATCH_TOO_LARGE` | Batch exceeds size limit | No |
| 11 | `QUERY_SYNTAX_ERROR` | Invalid SQL syntax | No |
| 12 | `QUERY_EXECUTION_ERROR` | Query runtime error | No |
| 13 | `TIMEOUT` | Operation timed out | Yes |
| 14 | `CLUSTER_UNAVAILABLE` | No available replicas | Yes |
| 15 | `PROTOCOL_VERSION_MISMATCH` | Unsupported protocol version | No |

**Retry Policy**:
- **Retryable errors**: Use exponential backoff (100ms, 200ms, 400ms, ...)
- **Non-retryable errors**: Fail immediately, report to caller

---

## Cluster Behavior

### Leader Discovery

Kimberlite clusters use a single-leader model:
- All writes go to leader
- Reads may go to followers (eventual consistency)

**Discovery Protocol**:
1. Client connects to any server in cluster
2. If server is not leader, it returns `CLUSTER_UNAVAILABLE` error with leader hint
3. Client reconnects to leader
4. Client caches leader address for future connections

**Leader Hint** (in `AuthResponse`):
```rust
struct AuthResponse {
    leader_hint: Option<SocketAddr>,  // "192.168.1.10:5432"
}
```

### Failover

On leader failure:
1. Cluster elects new leader (Raft consensus)
2. Clients receive `CLUSTER_UNAVAILABLE` error
3. Clients reconnect and repeat leader discovery
4. New leader hint provided in `AuthResponse`

**Recommended Client Behavior**:
- Maintain connection pool with all cluster addresses
- Health-check all connections every 30 seconds
- On `CLUSTER_UNAVAILABLE`, try different server
- Cache leader address for fast-path reconnection

---

## Bincode Serialization

Kimberlite uses [Bincode](https://github.com/bincode-org/bincode) with the following configuration:

```rust
bincode::config::standard()
    .with_little_endian()
    .with_fixed_int_encoding()
    .with_limit(16_777_216)  // 16 MB max
```

**Key Properties**:
- **Little-endian**: All integers are little-endian
- **Fixed-width integers**: No varint encoding (u64 is always 8 bytes)
- **Strings**: Length-prefixed (u64 length + UTF-8 bytes)
- **Vectors**: Length-prefixed (u64 length + elements)
- **Enums**: Discriminant (u32) + variant data

### Example: CreateStreamRequest

```rust
CreateStreamRequest {
    name: "events",
    data_class: DataClass::PHI,
    retention_days: None,
}
```

**Binary Encoding** (hex):
```
06 00 00 00 00 00 00 00  // name length (6)
65 76 65 6E 74 73        // "events" (UTF-8)
00 00 00 00              // data_class discriminant (0 = PHI)
00 00 00 00              // retention_days discriminant (0 = None)
```

### Example: AppendRequest

```rust
AppendRequest {
    stream_id: 42,
    events: vec![
        vec![0x01, 0x02, 0x03],
        vec![0x04, 0x05],
    ],
    expect_offset: Some(100),
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
01 00 00 00              // expect_offset discriminant (1 = Some)
64 00 00 00 00 00 00 00  // expect_offset value (100)
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
- [ ] Leader discovery and failover
- [ ] Connection pooling
- [ ] Health checks

### Server Requirements

- [ ] TCP listener with TLS support
- [ ] Authentication (tenant ID + token validation)
- [ ] Frame writing (header + CRC32 + payload)
- [ ] Bincode serialization
- [ ] Request routing to operation handlers
- [ ] Error response generation
- [ ] Leader election (Raft)
- [ ] Subscription management (push events to clients)

---

## Security Considerations

### Authentication

- **Production**: REQUIRED TLS + token-based auth (JWT recommended)
- **Development**: TLS optional, token may be empty string

### Authorization

- **Tenant Isolation**: All operations scoped to authenticated tenant
- **Stream Permissions**: Per-stream read/write/delete ACLs
- **Data Class Restrictions**: PHI streams require elevated privileges

### Transport Security

- **TLS Version**: 1.3 only (1.2 deprecated)
- **Cipher Suites**: AES-256-GCM, ChaCha20-Poly1305
- **Certificate Validation**: Client must verify server certificate

### Denial of Service

- **Rate Limiting**: Server enforces per-tenant request rate limits
- **Payload Size**: Max 16 MB per message
- **Connection Limits**: Max 1,000 concurrent connections per tenant

---

## Versioning

### Protocol Version

Current version: **1**

**Version Negotiation**:
1. Client sends `protocol_version: 1` in `AuthRequest`
2. Server checks compatibility
3. If unsupported, server returns `PROTOCOL_VERSION_MISMATCH`

**Backward Compatibility**:
- Minor changes (new optional fields) do not increment version
- Breaking changes (field removal, type changes) increment version
- Server may support multiple versions simultaneously

### Future Extensions

Planned for version 2:
- Compression (LZ4, Zstd)
- Batch queries (multiple SQL statements in single request)
- Streaming reads (server-initiated push for historical data)

---

## Example Session

### Complete Client-Server Exchange

**1. Connect**
```
Client -> Server: TCP SYN
Server -> Client: TCP SYN-ACK
Client -> Server: TCP ACK
```

**2. Authenticate**
```
Client -> Server:
  Frame Header:
    Magic: 0x4B4D4200
    Version: 1
    Payload Length: 128
    CRC32: 0x12345678
  Payload (Bincode):
    AuthRequest {
      protocol_version: 1,
      tenant_id: 42,
      auth_token: "secret-token",
      client_info: ClientInfo {
        name: "kimberlite-python",
        version: "0.1.0"
      }
    }

Server -> Client:
  Frame Header: ...
  Payload:
    AuthResponse::Success {
      cluster_id: 0x123e4567e89b12d3,
      leader_hint: Some("192.168.1.10:5432")
    }
```

**3. Create Stream**
```
Client -> Server:
  Request {
    request_id: 1,
    operation: CreateStream(CreateStreamRequest {
      name: "events",
      data_class: DataClass::PHI,
      retention_days: None
    })
  }

Server -> Client:
  Response {
    request_id: 1,
    result: Ok(CreateStream { stream_id: 100 })
  }
```

**4. Append Events**
```
Client -> Server:
  Request {
    request_id: 2,
    operation: Append(AppendRequest {
      stream_id: 100,
      events: vec![b"event1".to_vec(), b"event2".to_vec()],
      expect_offset: None
    })
  }

Server -> Client:
  Response {
    request_id: 2,
    result: Ok(Append { first_offset: 0 })
  }
```

**5. Query**
```
Client -> Server:
  Request {
    request_id: 3,
    operation: Query(QueryRequest {
      sql: "SELECT COUNT(*) FROM events",
      params: vec![]
    })
  }

Server -> Client:
  Response {
    request_id: 3,
    result: Ok(Query {
      columns: vec![
        ColumnDef { name: "count", type_name: "INT64" }
      ],
      rows: vec![
        vec![QueryValue::Int64(2)]
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
- **CRC32C Algorithm**: RFC 3720 (iSCSI)
- **TLS 1.3**: RFC 8446
- **Raft Consensus**: https://raft.github.io/

---

## Changelog

### Version 1 (2026-01-30)
- Initial protocol specification
- 7 core operations (CreateStream, Append, Read, Query, Subscribe, Checkpoint, DeleteStream)
- 15 error codes
- Bincode serialization
- Leader discovery and failover

---

## License

This specification is licensed under CC BY 4.0. Implementations may use any license.
