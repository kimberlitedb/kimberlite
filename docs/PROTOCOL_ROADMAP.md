# Kimberlite Wire Protocol Roadmap

**Version**: Draft
**Last Updated**: 2026-01-30

---

## Overview

This document tracks planned enhancements to the Kimberlite wire protocol. These features were identified during the protocol specification audit as valuable additions from the original draft that should be implemented.

---

## Priority 1: Critical for Production

### 1. Optimistic Concurrency Control for Appends

**Status**: Not Implemented
**Priority**: High
**Complexity**: Low

**Description**:
Add `expect_offset` field to `AppendEventsRequest` to enable optimistic concurrency control for event sourcing patterns.

**Current**:
```rust
struct AppendEventsRequest {
    stream_id: StreamId,
    events: Vec<Vec<u8>>,
}
```

**Proposed**:
```rust
struct AppendEventsRequest {
    stream_id: StreamId,
    events: Vec<Vec<u8>>,
    expect_offset: Option<Offset>,  // NEW: Expected current stream offset
}
```

**Semantics**:
- If `expect_offset` is `Some(n)`, server rejects append if current stream offset != n
- Returns new error code `OffsetMismatch` on failure
- Enables safe concurrent appends without distributed locking

**Use Cases**:
- Event sourcing with optimistic locking
- Collaborative editing with conflict detection
- State machine replication with consistency checks

**Implementation Notes**:
- Add to `crates/kmb-wire/src/message.rs`
- Update `crates/kmb-kernel/src/lib.rs` to check offset before append
- Add `OffsetMismatch` error code (suggest code 16)
- Update all SDK examples

**Files to Modify**:
- `crates/kmb-wire/src/message.rs`
- `crates/kmb-kernel/src/lib.rs`
- `crates/kmb-server/src/lib.rs`
- `docs/PROTOCOL.md`

---

### 2. Rich Event Metadata in ReadEvents

**Status**: Not Implemented
**Priority**: High
**Complexity**: Medium

**Description**:
Return structured Event objects with metadata instead of raw bytes in `ReadEventsResponse`.

**Current**:
```rust
struct ReadEventsResponse {
    events: Vec<Vec<u8>>,           // Loses metadata
    next_offset: Option<Offset>,
}
```

**Proposed**:
```rust
struct ReadEventsResponse {
    events: Vec<Event>,             // Rich event objects
    next_offset: Option<Offset>,
}

struct Event {
    offset: Offset,                 // Event's position in stream
    data: Vec<u8>,                  // Event payload
    timestamp: i64,                 // Server timestamp (nanos since epoch)
    checksum: u32,                  // Optional CRC32 of data
}
```

**Benefits**:
- Clients don't need to track offsets manually
- Timestamps enable time-based queries and analytics
- Checksums provide end-to-end data integrity verification
- Better SDK ergonomics

**Implementation Notes**:
- Storage layer already tracks offsets and timestamps
- Add Event struct to `kmb-wire`
- Update server to populate metadata from storage layer
- Maintain backward compat by versioning the protocol

**Files to Modify**:
- `crates/kmb-wire/src/message.rs`
- `crates/kmb-storage/src/lib.rs` (expose metadata)
- `crates/kmb-server/src/lib.rs`
- All SDK clients

---

### 3. Stream Retention Policies

**Status**: Not Implemented
**Priority**: High
**Complexity**: Medium

**Description**:
Add `retention_days` field to `CreateStreamRequest` for compliance-driven data lifecycle management.

**Current**:
```rust
struct CreateStreamRequest {
    name: String,
    data_class: DataClass,
    placement: Placement,
}
```

**Proposed**:
```rust
struct CreateStreamRequest {
    name: String,
    data_class: DataClass,
    placement: Placement,
    retention_days: Option<u32>,    // NEW: None = infinite retention
}
```

**Semantics**:
- `None`: Stream data retained indefinitely
- `Some(n)`: Stream data automatically deleted after n days
- Retention applies to event timestamps, not write time
- Compliance-critical for HIPAA, GDPR requirements

**Implementation Notes**:
- Add background compaction job to enforce retention
- Track oldest retained offset per stream
- Update `ReadEvents` to respect retention window
- Add metrics for retention enforcement

**Files to Modify**:
- `crates/kmb-wire/src/message.rs`
- `crates/kmb-kernel/src/lib.rs`
- `crates/kmb-storage/src/lib.rs` (compaction logic)
- Add new `kmb-compaction` crate for background jobs

---

## Priority 2: Enhanced Functionality

### 4. Subscribe Operation (Real-time Streaming)

**Status**: Not Implemented
**Priority**: Medium
**Complexity**: High

**Description**:
Add server-initiated push for real-time event streaming (like Kafka consumers).

**Proposed API**:
```rust
// Request
struct SubscribeRequest {
    stream_id: StreamId,
    from_offset: Offset,
    consumer_group: Option<String>,  // For load balancing
}

// Initial Response
struct SubscribeResponse {
    subscription_id: u64,
}

// Server-initiated push (new message type)
struct SubscriptionEvent {
    subscription_id: u64,
    events: Vec<Event>,
}

// Unsubscribe
struct UnsubscribeRequest {
    subscription_id: u64,
}
```

**Challenges**:
- Requires bidirectional messaging (breaks request/response model)
- Need backpressure mechanism to avoid overwhelming clients
- Subscription lifecycle management (reconnection, cleanup)
- Consumer group coordination for load balancing

**Implementation Strategy**:
- Phase 1: Single-subscriber push (no consumer groups)
- Phase 2: Add consumer group coordination
- Phase 3: Add backpressure via credit-based flow control

**Files to Add**:
- `crates/kmb-subscription/` - New crate for subscription management
- `crates/kmb-wire/src/subscription.rs`

---

### 5. Checkpoint Operation (Compliance Snapshots)

**Status**: Not Implemented
**Priority**: Medium
**Complexity**: High

**Description**:
Create immutable point-in-time snapshots of entire tenant database for compliance audits.

**Proposed API**:
```rust
// Request
struct CheckpointRequest {
    tenant_id: TenantId,
    label: Option<String>,           // e.g., "Q4-2025-audit"
}

// Response
struct CheckpointResponse {
    checkpoint_id: u128,             // Unique checkpoint ID
    position: Offset,                // Log position of checkpoint
    timestamp: i64,                  // When checkpoint was created
}

// List checkpoints
struct ListCheckpointsRequest {
    tenant_id: TenantId,
}

struct ListCheckpointsResponse {
    checkpoints: Vec<CheckpointInfo>,
}

struct CheckpointInfo {
    checkpoint_id: u128,
    label: Option<String>,
    position: Offset,
    timestamp: i64,
    size_bytes: u64,
}
```

**Use Cases**:
- Regulatory audits requiring point-in-time snapshots
- Disaster recovery with known-good states
- Testing against production data snapshots

**Implementation Notes**:
- Leverage existing `QueryAt` for point-in-time queries
- Store checkpoint metadata in system stream
- S3/object storage for checkpoint archival
- Retention policy for checkpoints (match stream retention)

**Files to Add**:
- `crates/kmb-checkpoint/` - New crate
- Update `docs/COMPLIANCE.md` with checkpoint procedures

---

### 6. DeleteStream Operation

**Status**: Not Implemented
**Priority**: Medium
**Complexity**: Medium

**Description**:
Soft-delete streams with compliance retention period.

**Proposed API**:
```rust
// Request
struct DeleteStreamRequest {
    stream_id: StreamId,
    retention_override: Option<u32>,  // Days to retain after delete
}

// Response
struct DeleteStreamResponse {
    deleted: bool,
    purge_date: i64,                 // When data will be physically deleted
}
```

**Semantics**:
- Marks stream as deleted (prevents reads/writes)
- Physical deletion deferred until retention period expires
- Audit log retains deletion event forever
- Deleted streams show in `ListStreams` with `deleted: true` flag

**Implementation Notes**:
- Add `deleted_at` timestamp to stream metadata
- Background job for physical purge after retention
- Update access control to block deleted streams
- Add `STREAM_DELETED` error code

**Files to Modify**:
- `crates/kmb-wire/src/message.rs`
- `crates/kmb-kernel/src/lib.rs`
- `crates/kmb-storage/src/lib.rs`

---

## Priority 3: Performance & Scale

### 7. Compression Support

**Status**: Not Implemented
**Priority**: Low
**Complexity**: Medium

**Description**:
Add optional compression to reduce bandwidth and storage costs.

**Proposed**:
- Add `compression` field to frame header (1 byte)
- Support LZ4 (fast) and Zstd (high compression) codecs
- Negotiate compression during handshake via capabilities
- Client specifies preferred compression in requests

**Compression Levels**:
- `None`: No compression (default)
- `LZ4`: Fast compression for latency-sensitive workloads
- `Zstd`: High compression for batch workloads

**Frame Header Change** (breaks protocol version):
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Magic (0x56444220)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
| Version (u16) | Comp  |       Payload Length (u32)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        CRC32 Checksum                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

**Implementation Notes**:
- Requires protocol version bump to v2
- Benchmark compression ratio vs CPU cost
- Add compression metrics to observability

---

### 8. Batch Query Operation

**Status**: Not Implemented
**Priority**: Low
**Complexity**: Low

**Description**:
Execute multiple SQL statements in a single request to reduce round-trips.

**Proposed API**:
```rust
struct BatchQueryRequest {
    queries: Vec<QueryRequest>,
    stop_on_error: bool,             // Stop batch on first error
}

struct BatchQueryResponse {
    results: Vec<Result<QueryResponse, ErrorResponse>>,
}
```

**Use Cases**:
- Reduce latency for multi-statement transactions
- Batch analytics queries
- Schema migrations with multiple DDL statements

---

### 9. Streaming Read (Large Result Sets)

**Status**: Not Implemented
**Priority**: Low
**Complexity**: High

**Description**:
Server-initiated push for large query results to avoid OOM on client.

**Current Problem**:
- Large `QueryResponse` can exceed 16 MiB frame limit
- Client must manually paginate with LIMIT/OFFSET (slow)

**Proposed**:
- Add `streaming: bool` to `QueryRequest`
- Server sends multiple `QueryChunk` messages
- Client acknowledges each chunk (backpressure)

**Requires**:
- Subscribe-style bidirectional messaging
- Flow control mechanism
- Chunk reassembly on client

---

## Implementation Priorities

### Q1 2026
- [ ] Optimistic concurrency control (expect_offset)
- [ ] Rich Event metadata in ReadEvents
- [ ] Stream retention policies

### Q2 2026
- [ ] Subscribe operation (Phase 1: single subscriber)
- [ ] Checkpoint operation
- [ ] DeleteStream operation

### Q3 2026
- [ ] Compression support (protocol v2)
- [ ] Subscribe with consumer groups (Phase 2)

### Q4 2026
- [ ] Batch query operation
- [ ] Streaming read for large results

---

## Breaking Changes

The following features require protocol version bump:

- **Version 2**: Compression support (frame header change)
- **Version 3**: Subscribe operation (bidirectional messaging)

All other features can be added to version 1 as they use optional fields or new operation types.

---

## Testing Requirements

For each new feature:
1. Unit tests in `kmb-wire` for serialization
2. Integration tests in `kmb-server` for operation handling
3. Property tests with `proptest` for edge cases
4. Update SDK test suites (Go, Python, TypeScript, Rust)
5. Add example to `docs/guides/`

---

## Documentation Updates

For each feature:
- [ ] Update `docs/PROTOCOL.md` with new operation spec
- [ ] Add migration guide if breaking change
- [ ] Update SDK README files with examples
- [ ] Add to CHANGELOG.md

---

## Metrics & Observability

Add metrics for:
- Optimistic concurrency failures (offset mismatches)
- Subscription throughput and lag
- Checkpoint creation time and size
- Compression ratios and CPU overhead
- Retention enforcement (bytes deleted)

---

## References

- Original protocol draft: `/private/tmp/.../protocol-audit-findings.md`
- Implementation: `crates/kmb-wire/`
- Current spec: `docs/PROTOCOL.md`
