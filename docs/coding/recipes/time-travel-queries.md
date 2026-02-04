# Time-Travel Queries

Query data as it existed at any point in time.

## What is Time-Travel?

Time-travel queries let you see historical state:

```sql
-- What does the database look like NOW?
SELECT * FROM patients WHERE id = 123;

-- What did it look like on January 15th?
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE id = 123;
```

This is possible because Kimberlite's append-only log preserves all history.

## Why Time-Travel?

**Compliance & Auditing:**
- "Show me what you knew on date X" (regulatory audits)
- "When did this value change?" (investigations)
- "Prove you followed the process" (compliance)

**Debugging:**
- "What state caused this bug?" (reproduce issues)
- "When did the data get corrupted?" (root cause analysis)

**Analytics:**
- "How has this metric changed over time?" (trends)
- "Compare Q1 vs Q2 data" (historical comparison)

## Syntax

### AS OF TIMESTAMP

Query at a specific timestamp:

```sql
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-15 10:30:00'
WHERE region = 'us-east';
```

**Returns:** Data as it existed at that timestamp.

### AS OF POSITION

Query at a specific log position:

```sql
SELECT * FROM patients
AS OF POSITION 1000
WHERE region = 'us-east';
```

**Returns:** Data as it existed after 1000 operations.

**Use when:** You know the exact position from an error message or log.

## Examples

### Example 1: Audit "Who Changed What When"

```sql
-- Current state
SELECT * FROM patients WHERE id = 123;
-- Result: name = "Alice Smith-Johnson" (married)

-- State on Jan 1
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-01'
WHERE id = 123;
-- Result: name = "Alice Smith" (before marriage)

-- Find when it changed
SELECT timestamp, name FROM __events
WHERE entity_type = 'Patient' AND entity_id = 123
ORDER BY timestamp;
-- Result: Changed at 2024-01-15 14:30:00
```

### Example 2: Debugging Incorrect State

```sql
-- Bug: Patient has wrong birth date today
SELECT * FROM patients WHERE id = 123;
-- Result: date_of_birth = '1990-03-15' (wrong!)

-- Check historical data
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-10'
WHERE id = 123;
-- Result: date_of_birth = '1985-03-15' (correct)

-- Find the breaking change
SELECT * FROM __events
WHERE entity_type = 'Patient' AND entity_id = 123
  AND timestamp > '2024-01-10'
  AND timestamp < NOW()
ORDER BY timestamp;
-- Result: Bad update at 2024-01-12 09:15:00
```

### Example 3: Compliance - Prove Process Was Followed

Auditor: "Prove patient consent was obtained before procedure."

```sql
-- Check consent at time of procedure
SELECT consent_status FROM patient_consents
AS OF TIMESTAMP '2024-01-15 09:00:00'  -- Procedure time
WHERE patient_id = 123 AND consent_type = 'surgery';
-- Result: 'granted' (obtained at 2024-01-14 16:30:00)
```

### Example 4: Compare Snapshots

```sql
-- How many patients in January?
SELECT COUNT(*) FROM patients
AS OF TIMESTAMP '2024-01-31 23:59:59';
-- Result: 1250

-- How many patients in February?
SELECT COUNT(*) FROM patients
AS OF TIMESTAMP '2024-02-29 23:59:59';
-- Result: 1310

-- Growth: 60 new patients
```

### Example 5: Reconstruct Deleted Data

```sql
-- Patient was deleted today
SELECT * FROM patients WHERE id = 123;
-- Result: (empty)

-- But we can see it yesterday
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-14'
WHERE id = 123;
-- Result: Full patient record

-- Find when it was deleted
SELECT * FROM __events
WHERE entity_type = 'Patient' AND entity_id = 123
  AND event_type = 'Deleted'
ORDER BY timestamp DESC
LIMIT 1;
-- Result: Deleted at 2024-01-15 10:30:00 by user_id=456
```

## Programmatic Access

### Rust

```rust
use kimberlite::Client;
use chrono::Utc;

let client = Client::connect("localhost:5432")?;

// Query at specific timestamp
let timestamp = Utc.ymd(2024, 1, 15).and_hms(10, 30, 0);
let patients = client
    .query("SELECT * FROM patients WHERE region = ?")
    .bind("us-east")
    .as_of_timestamp(timestamp)
    .fetch_all()?;

// Query at specific position
let patients = client
    .query("SELECT * FROM patients WHERE region = ?")
    .bind("us-east")
    .as_of_position(1000)
    .fetch_all()?;
```

### Python

```python
from kimberlite import Client
from datetime import datetime

client = Client("localhost:5432")

# Query at specific timestamp
timestamp = datetime(2024, 1, 15, 10, 30, 0)
patients = client.query(
    "SELECT * FROM patients WHERE region = ?",
    ["us-east"],
    as_of_timestamp=timestamp
)

# Query at specific position
patients = client.query(
    "SELECT * FROM patients WHERE region = ?",
    ["us-east"],
    as_of_position=1000
)
```

### TypeScript

```typescript
import { Client } from 'kimberlite';

const client = new Client('localhost:5432');

// Query at specific timestamp
const timestamp = new Date('2024-01-15T10:30:00Z');
const patients = await client.query(
  'SELECT * FROM patients WHERE region = ?',
  ['us-east'],
  { asOfTimestamp: timestamp }
);

// Query at specific position
const patients = await client.query(
  'SELECT * FROM patients WHERE region = ?',
  ['us-east'],
  { asOfPosition: 1000 }
);
```

## How It Works

### MVCC (Multi-Version Concurrency Control)

Each row has multiple versions:

```
Row for patient_id=123:
┌─────────────────────────────────────────────────┐
│ Version 1: created_at=100, name="Alice Smith"   │
├─────────────────────────────────────────────────┤
│ Version 2: created_at=500, name="Alice Johnson" │
├─────────────────────────────────────────────────┤
│ Version 3: created_at=800, deleted=true         │
└─────────────────────────────────────────────────┘
```

**Query at position 600:**
- Skip version 1 (created_at=100 < 600)
- Return version 2 (created_at=500 < 600 < 800)
- Skip version 3 (created_at=800 > 600)

### Performance

**Time complexity:** O(versions) per row
- Typical: 1-5 versions per row → same as regular query
- Worst case: 1000s of versions → slower

**Optimization:** Projections compact old versions automatically.

## Limitations

### Cannot Modify Historical Data

```sql
-- This is NOT allowed
UPDATE patients
AS OF TIMESTAMP '2024-01-15'
SET name = 'Alice Smith'
WHERE id = 123;
-- Error: Cannot modify historical data
```

Time-travel is **read-only**. The log is immutable.

### No Cross-Temporal Joins

```sql
-- This is NOT supported
SELECT
  p1.name AS current_name,
  p2.name AS past_name
FROM patients p1
JOIN patients AS OF TIMESTAMP '2024-01-01' p2 ON p1.id = p2.id;
-- Error: Cross-temporal joins not supported
```

**Workaround:** Run two separate queries.

### Performance on Old Data

Querying very old data may require scanning many segments:

```sql
-- Fast (recent data, likely in cache)
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-14';

-- Slow (old data, requires disk reads)
SELECT * FROM patients
AS OF TIMESTAMP '2020-01-01';
```

**Mitigation:** Use checkpoints (planned for v0.6.0).

## Retention Policy

How long can you time-travel?

**Default:** As long as data is retained (7-10 years typical).

**Compaction:** Old segments can be compacted, but checkpoints preserve access.

**Legal hold:** Data under legal hold is never compacted.

See [Compliance](../../concepts/compliance.md) for retention policies.

## Use Cases

| Use Case | Query Pattern |
|----------|---------------|
| Audit trail | `AS OF TIMESTAMP` + `WHERE entity_id = X` |
| Debugging | Compare current vs historical state |
| Compliance | Prove state at specific time |
| Analytics | Trend analysis over time |
| Recovery | Restore accidentally deleted data |
| Investigation | Root cause analysis |

## Related Documentation

- **[Data Model](../../concepts/data-model.md)** - How MVCC works
- **[Compliance](../../concepts/compliance.md)** - Audit trail requirements
- **[Audit Trails](audit-trails.md)** - Full audit logging patterns

---

**Key Takeaway:** Time-travel queries let you see data as it existed at any point in time. This is natural in Kimberlite because the log preserves all history.
