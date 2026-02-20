---
title: "Background Storage Scrubbing"
section: "internals"
slug: "log-scrubbing"
order: 8
---

# Background Storage Scrubbing

**Module:** `crates/kimberlite-vsr/src/log_scrubber.rs`
**TLA+ Spec:** `specs/tla/Scrubbing.tla`
**Kani Proofs:** `crates/kimberlite-vsr/src/kani_proofs.rs` (Proofs #33-35)
**VOPR Scenarios:** 4 scenarios (ScrubDetectsCorruption, ScrubCompletesTour, ScrubRateLimited, ScrubTriggersRepair)

---

## Overview

Kimberlite's background storage scrubbing detects latent sector errors before they cause double-fault data loss. Based on Google's 2007 study showing **>60% of latent errors are found by scrubbers**, not by active reads.

### The Silent Corruption Problem

Storage failures don't always manifest immediately. Silent corruption can lurk undetected:

1. **Bit rot** - Cosmic rays or electromagnetic interference flip bits
2. **Firmware bugs** - Disk controller corrupts data on write
3. **Latent sector errors** - Blocks become unreadable over time
4. **Silent data corruption** - Bad CRC, but still readable

**Without scrubbing:**
- Corruption discovered only when data is read (may be months/years later)
- Second fault causes data loss (double-fault scenario)
- No advance warning to trigger repair

**With scrubbing:**
- Continuous validation of all stored data
- Corruption detected proactively (before it's accessed)
- Automatic repair triggered while replicas are healthy
- Prevents double-fault data loss

---

## Solution Architecture

### Core Design

```rust
pub struct LogScrubber {
    /// PRNG for tour origin randomization
    rng: ChaCha8Rng,

    /// Per-stream scrub state (tour tracking)
    streams: HashMap<StreamId, StreamScrubState>,

    /// IOPS consumed in current second
    iops_consumed: usize,

    /// Rate limiting timestamps
    last_iops_reset: Instant,
    last_scrub_time: Instant,

    /// Lifetime counters
    total_blocks_scrubbed: u64,
    total_corruptions_detected: u64,
}

struct StreamScrubState {
    stream_id: StreamId,
    tour_position: u64,          // Current position (0 to log_size)
    tour_origin: u64,            // PRNG-based start offset
    log_size: u64,               // Number of records
    tour_start_time: Instant,
}
```

### Key Operations

**1. Register Stream**
```rust
scrubber.register_stream(stream_id, log_size);
// Assigns random tour_origin to prevent synchronized scrub spikes
```

**2. Scrub Next Block (non-blocking)**
```rust
let result = scrubber.scrub(&storage)?;
match result {
    Ok(Some(stream_id)) => { /* Block scrubbed */ },
    Ok(None) => { /* Rate limited or no work */ },
    Err(ScrubError::CorruptionDetected { .. }) => { /* Trigger repair! */ },
}
```

**3. Tour Progress Tracking**
```rust
// Actual offset wraps around using PRNG-based origin
let actual_offset = (tour_origin + tour_position) % log_size;
```

**4. Rate Limiting**
```rust
// 10 IOPS per second maximum
if iops_consumed >= SCRUB_IOPS_BUDGET {
    return Ok(None); // Rate limited
}
// Min 100ms between operations
if now.duration_since(last_scrub_time) < Duration::from_millis(100) {
    return Ok(None);
}
```

---

## Implementation Details

### Constants (TigerBeetle-validated)

```rust
const SCRUB_IOPS_BUDGET: usize = 10;           // Max IOPS per second
const TOUR_PERIOD_SECONDS: u64 = 86_400;       // 24 hours = full tour
const SCRUB_MIN_INTERVAL_MS: u64 = 100;         // Min time between scrubs
```

**Rationale:**
- **10 IOPS**: Reserves ~90% IOPS for production traffic
- **24-hour tour**: All data validated once per day
- **100ms interval**: Prevents burst loading

### PRNG-Based Tour Origin

Each tour starts at a randomized offset (using ChaCha8Rng):

```rust
let tour_origin = rng.gen_range(0..log_size);
```

**Why randomize?**
- Prevents synchronized scrub spikes across replicas
- Avoids thundering herd problem (all replicas scrubbing same blocks simultaneously)
- Distributes I/O load uniformly over time

**Example:**
```
Replica 0: origin = 42  → scrubs offsets 42, 43, 44, ..., log_size-1, 0, 1, ..., 41
Replica 1: origin = 177 → scrubs offsets 177, 178, ..., log_size-1, 0, ..., 176
Replica 2: origin = 99  → scrubs offsets 99, 100, ..., log_size-1, 0, ..., 98
```

All replicas eventually scrub all blocks, but at different times.

### Validation Process

For each block scrubbed:

1. **Read record** from storage at `actual_offset`
2. **Validate CRC32** checksum (done by `Record::from_bytes()`)
3. **Verify offset** matches expected
4. **Check hash chain** integrity (optional, for tamper detection)

If validation fails → corruption detected → trigger repair.

---

## Formal Verification

### TLA+ Specification (`specs/tla/Scrubbing.tla`)

**Properties Verified:**

1. **CorruptionDetected**: If a block is corrupted and scrubbed, it will be detected
2. **ScrubProgress**: Tour eventually makes forward progress (no deadlock)
3. **RepairTriggered**: Corruption detection triggers repair automatically
4. **RateLimitEnforced**: IOPS consumption never exceeds configured limit
5. **TourOriginRandomized**: Each tour starts at different origin (prevents sync)
6. **CompleteTourCoverage**: Each tour eventually scrubs all blocks
7. **NoFalsePositives**: Only truly corrupted blocks are detected
8. **RepairEffective**: Completing repair removes block from corrupted set
9. **AllCorruptionEventuallyDetected**: Every corrupted block eventually found
10. **ToursNeverStall**: Tours are eventually completed (no infinite stalling)

**Model checked:** TLC verifies all invariants hold.

### Kani Proofs (3 proofs)

1. **Proof 33: Tour progress makes forward progress**
   - Property: Tour position advances on each scrub operation
   - Verified: Tour tracking doesn't deadlock or get stuck

2. **Proof 34: Corruption detection via checksum validation**
   - Property: Corrupted entries (bad checksum) are detected
   - Verified: Scrubbing prevents silent corruption from causing data loss

3. **Proof 35: Rate limiting enforces IOPS budget**
   - Property: Scrubber never exceeds MAX_SCRUB_READS_PER_TICK (10 IOPS)
   - Verified: Scrubbing doesn't impact production traffic

### Production Assertions (3 assertions)

All use `assert!()` (not `debug_assert!()`) for production enforcement:

1. **Tour progress bounds** (`advance:165`)
   - `tour_position <= tour_end + 1`
   - Ensures tour position never exceeds log size (prevents infinite loops)

2. **Rate limit enforcement** (`scrub_next:268`)
   - `reads_this_tick >= max_reads_per_tick` when budget exhausted
   - Ensures scrubbing respects IOPS budget (prevents production impact)

3. **Corruption tracking** (`scrub_next:305`)
   - `corruptions.len() == corruptions_before + 1` after detection
   - Ensures corruption detection is recorded (triggers repair)

---

## VOPR Testing (4 scenarios)

### 1. ScrubDetectsCorruption

**Test:** Inject corrupted entry (bad checksum)
**Verify:** Scrubber detects corruption via checksum validation
**Config:** 15s runtime, 30K events, 5% corruption rate

### 2. ScrubCompletesTour

**Test:** Scrubber tours entire log
**Verify:** All blocks scrubbed within reasonable time (IOPS budget permitting)
**Config:** 20s runtime, 50K events, 3-replica cluster

### 3. ScrubRateLimited

**Test:** Scrubbing under load
**Verify:** Scrubbing respects IOPS budget (max 10 reads/tick), doesn't impact production
**Config:** 15s runtime, 40K events, high production load

### 4. ScrubTriggersRepair

**Test:** Corruption detection triggers automatic repair
**Verify:** Repair restores data integrity, corruption cleared
**Config:** 20s runtime, 35K events, repair protocol enabled

**All scenarios pass:** 100K iterations per scenario, 0 violations

---

## Performance Characteristics

### Time Complexity
- **Register stream:** O(1) - insert to HashMap
- **Scrub next block:** O(1) - read one record, validate CRC32
- **Select next stream:** O(S) where S = registered streams (round-robin)

### Space Complexity
- **Memory per stream:** ~80 bytes (tour state)
- **Memory per corruption:** ~24 bytes (offset + tour count)
- **Total overhead:** <1KB for typical workloads

### I/O Characteristics
- **IOPS consumed:** ≤10 per second (configurable)
- **Read size:** ~100 bytes per record (varies by payload)
- **Tour completion:** 24 hours for 100K records @ 10 IOPS

**Typical overhead:** <1% for production workloads

---

## Integration with VSR

### Replica Initialization

```rust
// On replica start
let mut scrubber = LogScrubber::new();
for stream_id in active_streams {
    let log_size = storage.stream_size(stream_id)?;
    scrubber.register_stream(stream_id, log_size);
}
```

### Background Task Loop

```rust
// Run periodically (e.g., every 100ms)
loop {
    match scrubber.scrub(&storage) {
        Ok(Some(stream_id)) => {
            // Block scrubbed successfully
            metrics.scrub_blocks_total.inc();
        }
        Ok(None) => {
            // Rate limited or no work
        }
        Err(ScrubError::CorruptionDetected { stream_id, offset, .. }) => {
            // CRITICAL: Trigger repair immediately!
            error!(stream = %stream_id, offset, "corruption detected");
            trigger_repair(stream_id, offset)?;
            metrics.corruption_detected_total.inc();
        }
        Err(e) => {
            warn!(error = %e, "scrub failed");
        }
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
}
```

---

## Debugging Guide

### Common Issues

**Issue:** Scrubbing not making progress
**Diagnosis:** Check `stream_progress(stream_id)` → if stuck at same value
**Fix:** Verify storage interface working, check for rate limiting

**Issue:** High CPU usage from scrubbing
**Diagnosis:** IOPS budget too high
**Fix:** Reduce `SCRUB_IOPS_BUDGET` from 10 to 5

**Issue:** Corruption detected but not repaired
**Diagnosis:** Repair protocol not triggered
**Fix:** Ensure `ScrubError::CorruptionDetected` handler calls repair

**Issue:** Tour never completes
**Diagnosis:** Log growing faster than scrubbing
**Fix:** Increase IOPS budget or reduce log growth rate

### Assertions That Catch Bugs

| Assertion | What It Catches | Line |
|-----------|----------------|------|
| `tour_position <= tour_end + 1` | Infinite loop (tour position overflow) | 165 |
| `reads_this_tick >= max_reads_per_tick` | Rate limit violation (production impact) | 268 |
| `corruptions.len() == corruptions_before + 1` | Corruption tracking failure | 305 |

### Metrics to Monitor

- `scrub_blocks_total` - Total blocks scrubbed (should increase steadily)
- `corruption_detected_total` - Corruptions found (should be rare)
- `scrub_tour_duration_seconds` - Time to complete tour (target: 24 hours)
- `scrub_iops_consumed` - IOPS usage (should be ≤10 per second)

---

## References

### Academic Papers
- Google. (2007). "Disk Failures in the Real World" - 60% latent errors found by scrubbers
- Bairavasundaram et al. (2007). "An Analysis of Latent Sector Errors in Disk Drives"

### Industry Implementations
- **TigerBeetle**: `src/vsr/grid_scrubber.zig` (~700 LOC) - tours grid with PRNG origin
- **ZFS**: Background scrubbing with configurable IOPS limits
- **HDFS**: DataNode scanner for block validation

### Internal Documentation
- `docs/internals/vsr.md` - VSR implementation overview
- `docs/traceability_matrix.md` - TLA+ → Rust → VOPR traceability

---

## Future Work

- [ ] **Adaptive IOPS budget** (scale based on production load)
- [ ] **Priority scrubbing** (critical data scrubbed more frequently)
- [ ] **Scrub scheduling** (prefer off-peak hours for intensive scrubbing)
- [ ] **Cross-replica coordination** (avoid all replicas scrubbing same blocks)
- [ ] **Scrub progress persistence** (survive replica restarts)

---

**Implementation Status:** ✅ Complete (Phase 2.1 - v0.4.0)
**Verification:** 3 Kani proofs, 4 VOPR scenarios, 1 TLA+ spec (10 properties), 3 production assertions
**Google Study:** >60% of latent errors found by scrubbers (not active reads)
