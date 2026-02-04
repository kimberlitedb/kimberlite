# Storage - Append-Only Log

The append-only log is Kimberlite's storage foundation.

## Design Principles

1. **Append-only:** Never modify or delete entries
2. **Sequential writes:** Always append to the end
3. **Checksummed:** CRC32 on every entry
4. **Hash-chained:** Each entry links to previous
5. **Segment-based:** Split into manageable files

## Directory Structure

```
data/
├── log/
│   ├── 00000000.segment     # Segment 0: positions 0-999999
│   ├── 00000001.segment     # Segment 1: positions 1000000-1999999
│   ├── 00000002.segment     # Segment 2: current active segment
│   └── index.meta           # Segment index metadata
├── checkpoints/
│   ├── checkpoint-1000000   # Checkpoint at position 1M
│   └── checkpoint-2000000   # Checkpoint at position 2M
└── tenants/
    ├── 00000001/            # Tenant 1 data
    └── 00000002/            # Tenant 2 data
```

## Segment Structure

Segments are fixed-size files (default: 1GB):

```
┌─────────────────────────────────────────────────────────────────┐
│ Segment File                                                     │
│                                                                  │
│  ┌──────────┬──────────┬──────────┬──────────┬─────────────┐   │
│  │ Record 0 │ Record 1 │ Record 2 │   ...    │  Record N   │   │
│  └──────────┴──────────┴──────────┴──────────┴─────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Properties:**
- Sequential writes (append-only)
- Preallocated (reduces fragmentation)
- Immutable once sealed
- Checksummed per-segment

## Record Format

Each record is self-describing:

```
┌──────────────────────────────────────────────────────────────────┐
│ Record (on disk)                                                  │
│                                                                   │
│  ┌─────────┬──────────┬───────────┬──────────┬────────────────┐ │
│  │ Length  │ Checksum │ Prev Hash │ Metadata │     Data       │ │
│  │ (4 B)   │ (4 B)    │ (32 B)    │ (var)    │    (var)       │ │
│  └─────────┴──────────┴───────────┴──────────┴────────────────┘ │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

### Field Details

#### Length (4 bytes)
- **Type:** u32 (little-endian)
- **Purpose:** Total record size in bytes
- **Range:** 40 bytes (minimum) to 16MB (maximum)
- **Not checksummed:** Allows reading length before validating

#### Checksum (4 bytes)
- **Type:** CRC32 (IEEE polynomial)
- **Purpose:** Integrity verification
- **Covers:** All fields except length (prev_hash + metadata + data)
- **Validation:** On read, recompute and compare

#### Prev Hash (32 bytes)
- **Type:** SHA-256 hash
- **Purpose:** Hash of previous record (tamper-evident chain)
- **Special case:** First record has all-zero prev_hash

#### Metadata (variable)
- **Position:** u64 (8 bytes)
- **Tenant ID:** u64 (8 bytes)
- **Stream ID:** u64 (8 bytes)
- **Timestamp:** i64 (8 bytes, Unix timestamp in microseconds)
- **Event Type:** u16 (2 bytes)
- **Reserved:** 6 bytes for future use
- **Total:** 40 bytes

#### Data (variable)
- **Type:** Arbitrary bytes
- **Purpose:** Application payload
- **Serialization:** Determined by event type (typically postcard or bincode)
- **Compression:** Optional (zstd for large payloads)

## Hash Chaining

Every record includes the hash of the previous record:

```
Record N-1          Record N            Record N+1
┌─────────┐         ┌─────────┐         ┌─────────┐
│ data    │         │ data    │         │ data    │
│ hash ───┼────────►│ prev    │         │ prev    │
│ = H1    │         │ hash ───┼────────►│ hash    │
│         │         │ = H1    │         │ = H2    │
│         │         │ hash    │         │ hash    │
│         │         │ = H2    │         │ = H3    │
└─────────┘         └─────────┘         └─────────┘

H1 = SHA256(Record N-1)
H2 = SHA256(Record N)
H3 = SHA256(Record N+1)
```

**Verification:**

```rust
fn verify_chain(segment: &Segment) -> Result<()> {
    let mut prev_hash = Hash::zero();  // Genesis

    for record in segment.records() {
        // Verify checksum
        if !record.verify_checksum() {
            return Err(Error::ChecksumMismatch);
        }

        // Verify hash chain
        if record.prev_hash != prev_hash {
            return Err(Error::ChainBroken {
                expected: prev_hash,
                actual: record.prev_hash,
            });
        }

        prev_hash = record.compute_hash();
    }

    Ok(())
}
```

## Write Path

Appending to the log:

```rust
impl AppendOnlyLog {
    pub fn append(&mut self, stream_id: StreamId, data: &[u8]) -> Result<Offset> {
        // 1. Get current segment (or create new one)
        let segment = self.get_active_segment()?;

        // 2. Compute previous hash
        let prev_hash = self.last_hash.unwrap_or(Hash::zero());

        // 3. Build record
        let record = Record {
            length: (40 + data.len()) as u32,
            checksum: 0,  // Computed next
            prev_hash,
            metadata: Metadata {
                position: self.next_position,
                tenant_id: stream_id.tenant_id,
                stream_id: stream_id.stream,
                timestamp: self.clock.now_micros(),
                event_type: EventType::Data,
                reserved: [0; 6],
            },
            data: data.to_vec(),
        };

        // 4. Compute checksum
        let checksum = compute_crc32(&record);
        record.checksum = checksum;

        // 5. Write to segment
        segment.append(&record)?;

        // 6. Update state
        self.next_position += 1;
        self.last_hash = Some(record.compute_hash());

        // 7. Fsync (if durable writes enabled)
        if self.config.durable {
            segment.fsync()?;
        }

        Ok(Offset::new(self.next_position - 1))
    }
}
```

**Performance optimizations:**
- Batch writes: Append multiple records before fsync
- Buffered IO: Use 4KB buffers aligned to page size
- Direct IO: Use `O_DIRECT` to bypass page cache (optional)
- Group commit: Batch fsync for multiple appends

## Read Path

Reading from the log:

```rust
impl AppendOnlyLog {
    pub fn read_at(&self, stream_id: StreamId, offset: Offset) -> Result<Entry> {
        // 1. Find segment containing this offset
        let segment_id = offset.as_u64() / SEGMENT_SIZE;
        let segment = self.get_segment(segment_id)?;

        // 2. Find record in segment
        let record = segment.read_at_offset(offset)?;

        // 3. Verify checksum
        if !record.verify_checksum() {
            return Err(Error::CorruptedEntry { offset });
        }

        // 4. Verify stream_id matches
        if record.metadata.stream_id != stream_id.stream {
            return Err(Error::WrongStream { offset });
        }

        // 5. Return entry
        Ok(Entry {
            offset,
            data: record.data,
            timestamp: record.metadata.timestamp,
        })
    }
}
```

## Segment Management

### Segment Creation

New segment created when:
- Current segment reaches 1GB (default)
- Manually triggered via `seal_segment()`

```rust
fn create_new_segment(&mut self) -> Result<SegmentId> {
    let segment_id = self.next_segment_id;
    let path = format!("data/log/{:08}.segment", segment_id);

    // Preallocate segment file
    let file = File::create(&path)?;
    file.set_len(SEGMENT_SIZE)?;  // 1GB

    // Create segment index
    let segment = Segment::new(segment_id, file)?;
    self.segments.insert(segment_id, segment);

    self.next_segment_id += 1;
    Ok(segment_id)
}
```

### Segment Sealing

Once sealed, segments become immutable:

```rust
fn seal_segment(&mut self, segment_id: SegmentId) -> Result<()> {
    let segment = self.segments.get_mut(&segment_id)?;

    // 1. Fsync to ensure durability
    segment.fsync()?;

    // 2. Write segment metadata (hash of entire segment)
    let segment_hash = segment.compute_hash()?;
    segment.write_metadata(segment_hash)?;

    // 3. Mark as sealed (read-only)
    segment.seal()?;

    // 4. Update index
    self.index.add_segment(segment_id, segment_hash);

    Ok(())
}
```

### Segment Compaction (Future)

Old segments can be compacted:

```rust
// Planned for v0.6.0
fn compact_segments(&mut self, segments: &[SegmentId]) -> Result<()> {
    // 1. Create snapshot at oldest segment's start position
    let snapshot = self.create_snapshot(segments[0])?;

    // 2. Replay log from snapshot to latest
    let compacted = self.replay_from_snapshot(snapshot)?;

    // 3. Write compacted segment
    let new_segment = self.write_compacted(compacted)?;

    // 4. Delete old segments
    for segment_id in segments {
        self.delete_segment(*segment_id)?;
    }

    Ok(())
}
```

## Checkpoints

Checkpoints capture state at a specific position:

```
┌────────────────────────────────────────────────────────────┐
│ Checkpoint File                                             │
│                                                             │
│  ┌────────────────────────────────────────────────────┐    │
│  │ Metadata                                            │    │
│  │  - Position: 1000000                                │    │
│  │  - Timestamp: 2024-01-15 10:30:00                   │    │
│  │  - Hash: abc123...                                  │    │
│  └────────────────────────────────────────────────────┘    │
│                                                             │
│  ┌────────────────────────────────────────────────────┐    │
│  │ State Snapshot                                      │    │
│  │  - Tenants: [...]                                   │    │
│  │  - Tables: [...]                                    │    │
│  │  - Projections: [...]                               │    │
│  └────────────────────────────────────────────────────┘    │
│                                                             │
└────────────────────────────────────────────────────────────┘
```

**Usage:**
- Speed up recovery (replay from checkpoint, not from genesis)
- Enable log compaction (delete segments before checkpoint)

**Status:** Planned for v0.6.0

## Performance Characteristics

**Write throughput:**
- Single-threaded: 50k-100k ops/sec
- Bottleneck: Disk bandwidth (500 MB/s SSD → ~50k ops/sec at 10KB/op)
- With group commit: 100k-200k ops/sec

**Read throughput:**
- Sequential scan: 1-2 GB/sec (SSD bandwidth limited)
- Random reads: 50k-100k ops/sec

**Latency:**
- Write (no fsync): <1ms
- Write (with fsync): 1-10ms (depends on disk)
- Read (from page cache): <0.1ms
- Read (from disk): 0.5-5ms

## Fault Tolerance

### Corruption Detection

Every read verifies:
1. CRC32 checksum (detect bit flips)
2. Hash chain (detect tampering)

### Torn Writes

If power fails mid-write:

```rust
fn recover_from_torn_write(&mut self) -> Result<()> {
    let segment = self.get_active_segment()?;

    // Scan segment, stop at first invalid record
    while let Some(record) = segment.read_next()? {
        if !record.verify_checksum() {
            // Torn write detected, truncate here
            segment.truncate_at(record.offset)?;
            return Ok(());
        }
    }

    Ok(())
}
```

### Disk Failure

If disk fails:
- VSR replicates to other nodes
- Repair from healthy replica

See [Consensus](../../concepts/consensus.md) for details.

## Testing

Storage is tested extensively:

- **Unit tests:** CRC32, hash chaining, segment management
- **Property tests:** Append/read round-trips
- **Corruption tests:** Inject bit flips, verify detection
- **VOPR scenarios:** 3 scenarios test corruption handling

See [VOPR Scenarios](/docs-internal/vopr/scenarios.md) - Phase 2.

## Related Documentation

- **[Data Model](../../concepts/data-model.md)** - How the log maps to events
- **[Cryptography](crypto.md)** - Hash algorithms used
- **[Testing Overview](../testing/overview.md)** - Storage testing strategies

---

**Key Takeaway:** The append-only log is the foundation of Kimberlite. Sequential writes, checksums, and hash chains provide durability, integrity, and tamper evidence.
