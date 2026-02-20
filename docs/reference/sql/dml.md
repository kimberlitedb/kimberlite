---
title: "SQL DML Reference"
section: "reference/sql"
slug: "dml"
order: 3
---

# SQL DML Reference

Data Manipulation Language for inserting, updating, and deleting data.

**Status:** Planned for v0.8.0

## INSERT

Insert rows into a projection (appends events to log).

### Syntax

```sql
INSERT INTO projection_name (column, ...)
  VALUES (value, ...), ...;

INSERT INTO projection_name (column, ...)
  SELECT ... FROM ...;
```

### Examples

**Single row:**

```sql
INSERT INTO patients (id, name, date_of_birth)
  VALUES (123, 'Alice Johnson', '1985-03-15');
```

**Multiple rows:**

```sql
INSERT INTO patients (id, name, date_of_birth)
  VALUES
    (123, 'Alice Johnson', '1985-03-15'),
    (124, 'Bob Smith', '1990-07-22'),
    (125, 'Carol White', '1978-11-30');
```

**Insert from SELECT:**

```sql
INSERT INTO active_patients (id, name)
  SELECT id, name
  FROM all_patients
  WHERE status = 'active';
```

**Insert with RETURNING:**

```sql
INSERT INTO patients (id, name)
  VALUES (123, 'Alice Johnson')
  RETURNING id, position, timestamp;
```

**Output:**
```
 id  | position | timestamp
-----+----------+-------------------------
 123 | 12346    | 2024-01-15 10:30:00.123
```

### Behavior

- Appends event to underlying log
- Updates projection asynchronously (~10ms)
- Returns immediately (does not wait for projection)

**Wait for projection:**

```rust
let result = client.query(
    "INSERT INTO patients (id, name) VALUES (?, ?) RETURNING position",
    &[&123, &"Alice"]
)?;
let position = result[0].get::<u64>("position");

// Wait for projection to include this position
client.wait_for_position(Position::new(position))?;

// Now query will include inserted row
let row = client.query("SELECT * FROM patients WHERE id = 123")?;
```

### Constraints

- **Unique indexes** enforced at projection level
- **Foreign keys** NOT supported (use application logic)
- **Check constraints** NOT supported

**Unique constraint:**

```sql
CREATE UNIQUE INDEX patients_id_idx ON patients (id);

-- This will succeed
INSERT INTO patients (id, name) VALUES (123, 'Alice');

-- This will fail (duplicate key)
INSERT INTO patients (id, name) VALUES (123, 'Bob');
-- Error: duplicate key value violates unique constraint "patients_id_idx"
```

## UPDATE

Update rows in a projection (appends delta event to log).

### Syntax

```sql
UPDATE projection_name
  SET column = value, ...
  WHERE condition
  [RETURNING ...];
```

### Examples

**Update single row:**

```sql
UPDATE patients
  SET status = 'inactive'
  WHERE id = 123;
```

**Update multiple rows:**

```sql
UPDATE patients
  SET status = 'inactive', updated_at = CURRENT_TIMESTAMP
  WHERE last_visit < '2023-01-01';
```

**Update with RETURNING:**

```sql
UPDATE patients
  SET status = 'inactive'
  WHERE id = 123
  RETURNING id, status, position;
```

**Conditional update:**

```sql
UPDATE patients
  SET visit_count = visit_count + 1
  WHERE id = 123
    AND status = 'active';
```

### Behavior

- Appends delta event to log (not full row)
- Projection reconstructs current state from deltas
- Cannot update primary key (use DELETE + INSERT instead)

**Delta event format:**

```json
{
  "op": "update",
  "projection": "patients",
  "key": {"id": 123},
  "changes": {
    "status": "inactive",
    "updated_at": "2024-01-15T10:30:00Z"
  }
}
```

### WHERE Clause Required

UPDATE without WHERE is dangerous and requires confirmation:

```sql
-- ❌ Fails by default
UPDATE patients SET status = 'inactive';
-- Error: UPDATE without WHERE clause. Add WHERE 1=1 to confirm.

-- ✅ Explicit confirmation
UPDATE patients SET status = 'inactive' WHERE 1=1;
```

## DELETE

Delete rows from a projection (appends tombstone to log).

### Syntax

```sql
DELETE FROM projection_name
  WHERE condition
  [RETURNING ...];
```

### Examples

**Delete single row:**

```sql
DELETE FROM patients
  WHERE id = 123;
```

**Delete multiple rows:**

```sql
DELETE FROM patients
  WHERE status = 'inactive'
    AND last_visit < '2020-01-01';
```

**Delete with RETURNING:**

```sql
DELETE FROM patients
  WHERE id = 123
  RETURNING id, name, position;
```

### Behavior

- Appends tombstone event to log
- Projection removes row asynchronously
- Log retains full history (tombstone is just a marker)

**Tombstone event format:**

```json
{
  "op": "delete",
  "projection": "patients",
  "key": {"id": 123}
}
```

### WHERE Clause Required

DELETE without WHERE requires confirmation:

```sql
-- ❌ Fails by default
DELETE FROM patients;
-- Error: DELETE without WHERE clause. Add WHERE 1=1 to confirm.

-- ✅ Explicit confirmation
DELETE FROM patients WHERE 1=1;
```

### Time-Travel After DELETE

Deleted rows remain in log history:

```sql
-- Current state (deleted)
SELECT * FROM patients WHERE id = 123;
-- (empty result)

-- Historical state (before delete)
SELECT * FROM patients
AS OF TIMESTAMP '2024-01-14'
WHERE id = 123;
-- Returns row as it existed before deletion
```

## UPSERT (INSERT ... ON CONFLICT)

Insert or update if row exists.

### Syntax

```sql
INSERT INTO projection_name (column, ...)
  VALUES (value, ...)
  ON CONFLICT (key_column)
  DO UPDATE SET column = value, ...;
```

### Examples

**Upsert single row:**

```sql
INSERT INTO patients (id, name, status)
  VALUES (123, 'Alice Johnson', 'active')
  ON CONFLICT (id)
  DO UPDATE SET
    name = EXCLUDED.name,
    status = EXCLUDED.status,
    updated_at = CURRENT_TIMESTAMP;
```

**Upsert with condition:**

```sql
INSERT INTO patients (id, visit_count)
  VALUES (123, 1)
  ON CONFLICT (id)
  DO UPDATE SET
    visit_count = patients.visit_count + 1
  WHERE patients.status = 'active';
```

**Upsert with DO NOTHING:**

```sql
INSERT INTO patients (id, name)
  VALUES (123, 'Alice Johnson')
  ON CONFLICT (id)
  DO NOTHING;
```

### EXCLUDED Table

`EXCLUDED` refers to the row that would have been inserted:

```sql
INSERT INTO patients (id, name, status)
  VALUES (123, 'Alice Johnson', 'active')
  ON CONFLICT (id)
  DO UPDATE SET
    name = EXCLUDED.name,      -- Use new value
    status = EXCLUDED.status,  -- Use new value
    updated_at = CURRENT_TIMESTAMP;  -- Computed value
```

## Transactions

Group multiple statements into atomic unit (v0.9.0+).

### Syntax

```sql
BEGIN;
  -- statements
  COMMIT | ROLLBACK;
```

### Examples

**Basic transaction:**

```sql
BEGIN;
  INSERT INTO patients (id, name) VALUES (123, 'Alice');
  INSERT INTO appointments (patient_id, date) VALUES (123, '2024-02-01');
COMMIT;
```

**Transaction with rollback:**

```sql
BEGIN;
  UPDATE patients SET status = 'inactive' WHERE id = 123;
  DELETE FROM appointments WHERE patient_id = 123;

  -- Oops, wrong patient!
ROLLBACK;
```

**Transaction with error handling:**

```rust
let tx = client.begin_transaction()?;

match tx.execute("INSERT INTO patients (id, name) VALUES (?, ?)", &[&123, &"Alice"]) {
    Ok(_) => {
        tx.execute("INSERT INTO appointments (patient_id, date) VALUES (?, ?)", &[&123, &"2024-02-01"])?;
        tx.commit()?;
    }
    Err(e) => {
        tx.rollback()?;
        return Err(e);
    }
}
```

### Isolation Levels

Kimberlite supports two isolation levels:

| Level | Behavior |
|-------|----------|
| **Read Committed** (default) | See committed changes from other transactions |
| **Serializable** | Full serializability (snapshot isolation + SSI) |

**Set isolation level:**

```sql
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;
  -- statements
COMMIT;
```

### Transaction Boundaries

- Transaction appends single atomic batch to log
- All statements succeed or all fail (no partial commits)
- Projections update after transaction commits

## Batch Operations

For bulk operations, use batch APIs:

### Rust

```rust
// Batch insert (single log append)
let rows = vec![
    (123, "Alice Johnson"),
    (124, "Bob Smith"),
    (125, "Carol White"),
];

client.batch_insert("patients", &["id", "name"], rows)?;
```

### Python

```python
# Batch insert
rows = [
    (123, "Alice Johnson"),
    (124, "Bob Smith"),
    (125, "Carol White"),
]

client.batch_insert("patients", ["id", "name"], rows)
```

**Performance:** 10-100x faster than individual INSERTs for >100 rows.

## Constraints and Validation

### Supported

- ✅ **NOT NULL** - Column cannot be null
- ✅ **UNIQUE** - Unique index
- ✅ **DEFAULT** - Default value

```sql
CREATE PROJECTION patients AS
  SELECT
    data->>'id' AS id NOT NULL,
    data->>'name' AS name NOT NULL,
    data->>'status' AS status DEFAULT 'active'
  FROM events;

CREATE UNIQUE INDEX patients_id_idx ON patients (id);
```

### NOT Supported (v0.8.0)

- ❌ **FOREIGN KEY** - Use application logic
- ❌ **CHECK** - Use application validation
- ❌ **REFERENCES** - Use application logic

**Workaround for foreign keys:**

```rust
// Application-level foreign key check
fn insert_appointment(client: &Client, patient_id: u64, date: &str) -> Result<()> {
    // Check patient exists
    let exists = client.query(
        "SELECT 1 FROM patients WHERE id = ? LIMIT 1",
        &[&patient_id]
    )?;

    if exists.is_empty() {
        return Err(Error::ForeignKeyViolation("patient_id"));
    }

    // Insert appointment
    client.execute(
        "INSERT INTO appointments (patient_id, date) VALUES (?, ?)",
        &[&patient_id, &date]
    )?;

    Ok(())
}
```

## Performance

| Operation | Throughput | Latency (P99) |
|-----------|------------|---------------|
| INSERT (single) | 50k/sec | 2ms |
| INSERT (batch 1000) | 500k/sec | 20ms |
| UPDATE (single) | 40k/sec | 2ms |
| DELETE (single) | 40k/sec | 2ms |
| Transaction (2 writes) | 25k/sec | 4ms |

**Best practices:**
- Batch operations when inserting >100 rows
- Use transactions for atomicity, not performance
- Index columns used in WHERE clauses

## Best Practices

### 1. Use Batch Operations

```rust
// ❌ Slow: Individual inserts
for row in rows {
    client.execute("INSERT INTO patients (id, name) VALUES (?, ?)", &[&row.id, &row.name])?;
}

// ✅ Fast: Batch insert
client.batch_insert("patients", &["id", "name"], rows)?;
```

### 2. Always Use WHERE in UPDATE/DELETE

```sql
-- ❌ Dangerous: Updates all rows
UPDATE patients SET status = 'inactive';

-- ✅ Safe: Explicit filter
UPDATE patients SET status = 'inactive' WHERE id = 123;

-- ✅ Intentional: Explicit confirmation
UPDATE patients SET status = 'inactive' WHERE 1=1;
```

### 3. Use UPSERT for Idempotency

```sql
-- ✅ Safe to retry (idempotent)
INSERT INTO patients (id, name)
  VALUES (123, 'Alice')
  ON CONFLICT (id)
  DO UPDATE SET name = EXCLUDED.name;

-- ❌ Fails on retry (not idempotent)
INSERT INTO patients (id, name)
  VALUES (123, 'Alice');
```

### 4. Use RETURNING for Feedback

```sql
-- ✅ Get confirmation
INSERT INTO patients (id, name)
  VALUES (123, 'Alice')
  RETURNING id, position, timestamp;
```

## Related Documentation

- **[SQL Overview](overview.md)** - SQL architecture
- **[DDL Reference](ddl.md)** - CREATE/DROP PROJECTION
- **[Query Reference](queries.md)** - SELECT syntax
- **[Coding Recipes](../../coding/recipes/)** - Application patterns

---

**Key Takeaway:** INSERT/UPDATE/DELETE append events to the log. Projections update asynchronously. Use batch operations for bulk inserts. Transactions provide atomicity across multiple statements.
