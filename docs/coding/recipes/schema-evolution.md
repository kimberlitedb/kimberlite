---
title: "Schema Evolution (ALTER TABLE)"
section: "coding/recipes"
slug: "schema-evolution"
order: 6
---

# Schema Evolution with ALTER TABLE

Kimberlite supports schema evolution through `ALTER TABLE ADD COLUMN` and
`ALTER TABLE DROP COLUMN`. Because the event log is immutable, schema
changes are **forward-only** at the catalog layer and **zero-cost** at
the storage layer:

- Existing events on disk are never rewritten.
- The planner materialises `NULL` for a new column when reading rows
  persisted before the `ADD`.
- `DROP COLUMN` removes the column from the current-state projection;
  the original values are still present in the append-only log and
  reachable via time-travel queries that predate the drop.

Every ALTER TABLE bumps an internal `schema_version` counter by exactly
one. This counter is what the row-layout cache is keyed on — changing
the schema is O(1) at read time because cached layouts are invalidated
automatically.

## ADD COLUMN with NULL materialisation

Rows inserted before an `ADD COLUMN` transparently surface `NULL` for
the new column:

```rust,ignore
use kimberlite::{Kimberlite, TenantId, Value};

let dir = tempfile::tempdir()?;
let db = Kimberlite::open(dir.path())?;
let tenant = db.tenant(TenantId::new(1));

// Initial schema
tenant.execute(
    "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    &[],
)?;

// Insert under the original schema
tenant.execute(
    "INSERT INTO patients (id, name) VALUES ($1, $2)",
    &[Value::BigInt(1), Value::Text("Alice".into())],
)?;

// Evolve: add an email column
tenant.execute("ALTER TABLE patients ADD COLUMN email TEXT", &[])?;

// Pre-ALTER row has NULL for email — no backfill needed
let rs = tenant.query("SELECT id, name, email FROM patients", &[])?;
assert_eq!(rs.rows.len(), 1);
assert!(matches!(rs.rows[0][2], Value::Null));

// Post-ALTER inserts can populate the new column
tenant.execute(
    "INSERT INTO patients (id, name, email) VALUES ($1, $2, $3)",
    &[
        Value::BigInt(2),
        Value::Text("Bob".into()),
        Value::Text("bob@example.com".into()),
    ],
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## DROP COLUMN with replayable event log

`DROP COLUMN` is a projection-level rename: the underlying row events
on disk still contain the dropped value, so time-travel queries that
predate the drop still see it. Current-state queries simply hide it:

```rust,ignore
use kimberlite::{Kimberlite, TenantId, Value};

let dir = tempfile::tempdir()?;
let db = Kimberlite::open(dir.path())?;
let tenant = db.tenant(TenantId::new(1));

tenant.execute(
    "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    &[],
)?;
tenant.execute("ALTER TABLE patients ADD COLUMN tmp TEXT", &[])?;
tenant.execute(
    "INSERT INTO patients (id, name, tmp) VALUES ($1, $2, $3)",
    &[
        Value::BigInt(1),
        Value::Text("Alice".into()),
        Value::Text("sensitive".into()),
    ],
)?;

tenant.execute("ALTER TABLE patients DROP COLUMN tmp", &[])?;

// The remaining columns still answer queries cleanly — the row
// event's other fields replay without issue.
let rs = tenant.query("SELECT id, name FROM patients", &[])?;
assert_eq!(rs.rows.len(), 1);
assert_eq!(rs.rows[0][0], Value::BigInt(1));
assert_eq!(rs.rows[0][1], Value::Text("Alice".into()));

// Selecting the dropped column surfaces a clear error (not silent NULL).
let err = tenant.query("SELECT tmp FROM patients", &[]).unwrap_err();
assert!(format!("{err}").to_lowercase().contains("column")
    || format!("{err}").contains("tmp"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Idempotence: re-adding the same column is rejected

`ALTER TABLE ADD COLUMN` rejects a column name that already exists on
the table. The kernel returns a typed `ColumnAlreadyExists` error — it
never silently succeeds or bumps `schema_version`:

```rust,ignore
use kimberlite::{Kimberlite, TenantId};

let dir = tempfile::tempdir()?;
let db = Kimberlite::open(dir.path())?;
let tenant = db.tenant(TenantId::new(1));

tenant.execute(
    "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    &[],
)?;

// First ADD succeeds
tenant.execute("ALTER TABLE patients ADD COLUMN email TEXT", &[])?;

// Second ADD on the same column name is rejected
let err = tenant
    .execute("ALTER TABLE patients ADD COLUMN email TEXT", &[])
    .unwrap_err();
let msg = format!("{err}").to_lowercase();
assert!(
    msg.contains("already") || msg.contains("email"),
    "error must mention duplicate/column name: {msg}",
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Primary-key columns cannot be dropped

Dropping a column that participates in the primary key is rejected
structurally, because it would orphan every persisted row key.

```rust,ignore
use kimberlite::{Kimberlite, TenantId};

let dir = tempfile::tempdir()?;
let db = Kimberlite::open(dir.path())?;
let tenant = db.tenant(TenantId::new(1));

tenant.execute(
    "CREATE TABLE patients (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    &[],
)?;

let err = tenant
    .execute("ALTER TABLE patients DROP COLUMN id", &[])
    .unwrap_err();
assert!(format!("{err}").to_lowercase().contains("primary"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Schema survives within a session (open → ALTER → query)

While a `Kimberlite` handle is live, every ALTER TABLE is immediately
visible to subsequent queries — the projection's row-layout cache is
keyed on `(table_id, schema_version)` so the cache is invalidated
without a global flush:

```rust,ignore
use kimberlite::{Kimberlite, TenantId, Value};

let dir = tempfile::tempdir()?;
let db = Kimberlite::open(dir.path())?;
let tenant = db.tenant(TenantId::new(1));

tenant.execute(
    "CREATE TABLE notes (id BIGINT PRIMARY KEY, body TEXT NOT NULL)",
    &[],
)?;

// Two ALTERs back-to-back. Each strictly advances schema_version.
tenant.execute("ALTER TABLE notes ADD COLUMN author TEXT", &[])?;
tenant.execute("ALTER TABLE notes ADD COLUMN tag TEXT", &[])?;

// Both new columns are present in the very next SELECT.
tenant.execute(
    "INSERT INTO notes (id, body, author, tag) VALUES ($1, $2, $3, $4)",
    &[
        Value::BigInt(1),
        Value::Text("body".into()),
        Value::Text("ada".into()),
        Value::Text("release".into()),
    ],
)?;
let rs = tenant.query(
    "SELECT id, body, author, tag FROM notes WHERE id = 1",
    &[],
)?;
assert_eq!(rs.rows.len(), 1);
assert_eq!(rs.rows[0][2], Value::Text("ada".into()));
assert_eq!(rs.rows[0][3], Value::Text("release".into()));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## See also

- [SQL DDL Reference — ALTER TABLE](../../reference/sql/ddl.md#alter-table)
- [Schema Migrations guide](../guides/migrations.md)
- [Time-travel queries](time-travel-queries.md) — reading rows at a pre-ALTER
  position still sees the old shape.
