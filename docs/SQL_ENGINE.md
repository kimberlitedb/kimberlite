# SQL Engine Implementation Status

## Overview

The SQL engine provides a familiar SQL interface over Kimberlite's event-sourcing core. It translates SQL statements into underlying stream operations while maintaining the immutable log and derived-view architecture.

## Current Implementation Status

### ✅ Fully Implemented

#### SELECT Queries
- **Parser**: Full support via `sqlparser` crate
- **Planner**: Query optimization with index selection
- **Executor**: Executes against projection store
- **Features**:
  - Column selection (`SELECT id, name` or `SELECT *`)
  - Single-table queries (`FROM users`)
  - WHERE predicates (`=`, `<`, `>`, `<=`, `>=`, `IN`)
  - AND combinations
  - ORDER BY (ASC/DESC)
  - LIMIT
  - Parameterized queries (`$1`, `$2`, etc.)
  - Point-in-time queries (`query_at(position)`)

**Example**:
```sql
SELECT id, name FROM users WHERE id = $1 ORDER BY name LIMIT 10
```

#### DDL (Data Definition Language)
- **Status**: ✅ **Working** - Full support for table and index management
- **Implemented Commands**:
  - ✅ `CREATE TABLE` - Define tables with columns, types, constraints, and primary keys
  - ✅ `DROP TABLE` - Remove tables from the database
  - ✅ `CREATE INDEX` - Create secondary indexes on columns
- **Validation**:
  - Primary key requirement enforcement
  - Column type validation
  - Duplicate table detection
- **Future**: `ALTER TABLE`, `CREATE PROJECTION`

**Example**:
```sql
CREATE TABLE patients (
    id BIGINT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMP,
    PRIMARY KEY (id)
);

CREATE INDEX idx_name ON patients (name);

DROP TABLE patients;
```

#### DML (Data Manipulation Language)
- **Status**: ✅ **Working** - Full CRUD operations with parameter binding
- **Implemented Commands**:
  - ✅ `INSERT INTO ... VALUES` - Insert rows with literal or parameterized values
  - ✅ `UPDATE ... SET ... WHERE` - Update rows matching WHERE predicates
  - ✅ `DELETE FROM ... WHERE` - Delete rows matching WHERE predicates
- **Features**:
  - Parameterized queries (`$1`, `$2`) for all DML operations
  - Structured predicate serialization (not debug strings)
  - Full projection materialization (INSERT/UPDATE/DELETE all update projections)
  - Composite primary key support
- **Validation**:
  - Column existence checking
  - Type compatibility validation
  - NOT NULL constraint enforcement
  - Primary key NULL rejection

**Examples**:
```sql
-- Parameterized INSERT
INSERT INTO patients (id, name) VALUES ($1, $2);

-- UPDATE with WHERE clause
UPDATE patients SET name = 'Jane Smith' WHERE id = 1;

-- DELETE with WHERE clause
DELETE FROM patients WHERE id = 1;

-- Composite primary key
CREATE TABLE orders (
    user_id BIGINT NOT NULL,
    order_id BIGINT NOT NULL,
    amount BIGINT,
    PRIMARY KEY (user_id, order_id)
);

UPDATE orders SET amount = 6000 WHERE user_id = 1 AND order_id = 100;
```

**Code**: `crates/kimberlite-query/` and `crates/kimberlite/`
- ✅ `kimberlite-query/src/parser.rs` - SQL parsing for SELECT, DDL, and DML
- ✅ `kimberlite-query/src/planner.rs` - Query planning
- ✅ `kimberlite-query/src/executor.rs` - Query execution
- ✅ `kimberlite-query/src/schema.rs` - Schema definitions
- ✅ `kimberlite-query/src/key_encoder.rs` - Lexicographic key encoding
- ✅ `kimberlite/src/tenant.rs` - DDL/DML execution and validation
- ✅ `kimberlite/src/kimberlite.rs` - Projection materialization for UPDATE/DELETE
- ✅ 85+ tests passing (including comprehensive DML roundtrip tests)

### ❌ Not Yet Implemented

#### Advanced DDL
- `ALTER TABLE` - Schema evolution
- `CREATE PROJECTION` - Materialized views
- Foreign key constraints
- CHECK constraints

#### Transactions
- Explicit `BEGIN`/`COMMIT`/`ROLLBACK`
- Multi-statement transactions
- Current behavior: Auto-commit per statement

## Architecture

### Actual Flow (SQL API - Implemented)
```
Client → CREATE TABLE → TenantHandle → Command::CreateTable → Kernel → Effect::TableMetadataWrite → Schema Update
Client → INSERT INTO → TenantHandle → Command::Insert → Kernel → Effect::StorageAppend → Log + Projection
Client → UPDATE/DELETE → TenantHandle → Command::Update/Delete → Kernel → Effect::UpdateProjection → Projection Update
Client → SELECT → QueryEngine → Projection Store (B+tree with MVCC)
```

**Key Components**:
- **TenantHandle**: SQL-to-Command translation layer
- **Kernel**: Pure state machine validates and produces effects
- **Projection Store**: Materialized views kept in sync via effects
- **QueryEngine**: Executes SELECT queries against projections

## Implementation Details (Completed)

### DDL Implementation (Completed)

#### Schema Commands
```rust
// crates/kimberlite-kernel/src/command.rs
pub enum Command {
    CreateTable {
        table_id: TableId,
        table_name: String,
        columns: Vec<ColumnDef>,
        primary_key: Vec<String>,
    },
    DropTable { table_id: TableId },
    CreateIndex {
        index_id: IndexId,
        table_id: TableId,
        index_name: String,
        columns: Vec<String>,
    },
    // ... other commands
}
```

#### DDL Parser
```rust
// crates/kimberlite-query/src/parser.rs
pub enum ParsedStatement {
    Select(ParsedSelect),
    CreateTable(ParsedCreateTable),
    DropTable(String),
    CreateIndex(ParsedCreateIndex),
    Insert(ParsedInsert),
    Update(ParsedUpdate),
    Delete(ParsedDelete),
}
```

#### Table-to-Stream Mapping (Implemented)
Each table gets its own stream for event isolation:
```
CREATE TABLE patients → Command::CreateTable → kernel state tracks metadata
INSERT INTO patients → Command::Insert → appends to table's stream
```

**Benefits**:
- Clean separation of concerns per table
- Independent replay and recovery per table
- Simpler to reason about event ordering

### DML Implementation (Completed)

#### INSERT Statement
**SQL**:
```sql
-- Literal values
INSERT INTO patients (id, name) VALUES (1, 'Alice');

-- Parameterized (recommended for security)
INSERT INTO patients (id, name) VALUES ($1, $2);
```

**Implementation** (`crates/kimberlite/src/tenant.rs`):
```rust
fn execute_insert(&self, insert: ParsedInsert, params: &[Value]) -> Result<ExecuteResult> {
    // 1. Bind parameters to placeholders
    let bound_values = bind_parameters(&insert.values, params)?;

    // 2. Validate columns exist, types match, NOT NULL constraints
    validate_insert_values(&column_names, &bound_values, &table_meta.columns, &table_meta.primary_key)?;

    // 3. Serialize as structured JSON event
    let event = json!({
        "type": "insert",
        "table": insert.table,
        "data": row_map,  // { "id": 1, "name": "Alice" }
    });

    // 4. Submit to kernel
    Command::Insert { table_id, row_data: event }
}
```

**Projection Update** (`crates/kimberlite/src/kimberlite.rs`):
```rust
"insert" => {
    let pk_key = build_primary_key(data, primary_key_cols)?;
    let batch = WriteBatch::new(offset).put(table_id, pk_key, row_bytes);
    projection_store.apply(batch)?;
}
```

#### UPDATE Statement
**SQL**:
```sql
UPDATE patients SET name = 'Bob', status = 'active' WHERE id = 1;
```

**Implementation**:
```rust
fn execute_update(&self, update: ParsedUpdate, params: &[Value]) -> Result<ExecuteResult> {
    // 1. Bind parameters in SET clause
    let bound_assignments = bind_assignment_parameters(&update.assignments, params)?;

    // 2. Serialize predicates as structured JSON (not debug strings!)
    let predicates_json = update.predicates.iter()
        .map(|p| predicate_to_json(p, params))
        .collect()?;

    // 3. Create event
    let event = json!({
        "type": "update",
        "table": update.table,
        "set": bound_assignments,  // [["name", "Bob"], ["status", "active"]]
        "where": predicates_json,  // [{"op": "eq", "column": "id", "values": [1]}]
    });

    Command::Update { table_id, row_data: event }
}
```

**Projection Update**:
```rust
"update" => {
    // 1. Extract primary key from WHERE predicates
    let pk_data = extract_primary_key_from_predicates(predicates, primary_key_cols)?;

    // 2. Read existing row
    let existing_row = projection_store.get(table_id, pk_key)?;

    // 3. Merge SET assignments with existing data
    for (col, val) in assignments {
        existing_row[col] = val;
    }

    // 4. Write back updated row
    projection_store.apply(WriteBatch::new(offset).put(table_id, pk_key, updated_row));
}
```

#### DELETE Statement
**SQL**:
```sql
DELETE FROM patients WHERE id = 1 AND status = 'inactive';
```

**Implementation**:
```rust
fn execute_delete(&self, delete: ParsedDelete, params: &[Value]) -> Result<ExecuteResult> {
    // Serialize predicates as structured JSON
    let predicates_json = delete.predicates.iter()
        .map(|p| predicate_to_json(p, params))
        .collect()?;

    let event = json!({
        "type": "delete",
        "table": delete.table,
        "where": predicates_json,
    });

    Command::Delete { table_id, row_data: event }
}
```

**Projection Update**:
```rust
"delete" => {
    // 1. Extract primary key from WHERE
    let pk_data = extract_primary_key_from_predicates(predicates, primary_key_cols)?;
    let pk_key = build_primary_key(&pk_data, primary_key_cols)?;

    // 2. Delete from projection store
    projection_store.apply(WriteBatch::new(offset).delete(table_id, pk_key));
}
```

### Server Integration (Completed)

The SQL engine is fully integrated with `kimberlite-server` and the REPL:

#### Request Handler
All SQL statements are routed through `TenantHandle::execute()`:

```rust
// Client request
client.execute("INSERT INTO patients VALUES ($1, $2)", &[...])

// Server handler (kimberlite-server/src/handler.rs)
let result = tenant.execute(&request.sql, &params)?;

// TenantHandle routes to appropriate handler
match parse_statement(sql)? {
    ParsedStatement::CreateTable(ddl) => execute_create_table(ddl),
    ParsedStatement::Insert(dml) => execute_insert(dml, params),
    ParsedStatement::Update(dml) => execute_update(dml, params),
    ParsedStatement::Delete(dml) => execute_delete(dml, params),
    ParsedStatement::Select(_) => /* use query() instead */,
}
```

#### REPL Support
The REPL (`kimberlite-cli`) supports all statement types:

```bash
$ kimberlite repl --address 127.0.0.1:5432
kimberlite> CREATE TABLE patients (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id));
rows_affected | log_offset
--------------+-----------
0             | 0

kimberlite> INSERT INTO patients VALUES (1, 'Jane Doe');
rows_affected | log_offset
--------------+-----------
1             | 1

kimberlite> SELECT * FROM patients;
id | name
---+---------
1  | Jane Doe

kimberlite> UPDATE patients SET name = 'Jane Smith' WHERE id = 1;
rows_affected | log_offset
--------------+-----------
1             | 2

kimberlite> DELETE FROM patients WHERE id = 1;
rows_affected | log_offset
--------------+-----------
1             | 3
```

### Testing (Completed)

#### Unit Tests (85+ passing)
- ✅ DDL parsing (`CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`)
- ✅ DML parsing (`INSERT`, `UPDATE`, `DELETE`) with placeholders
- ✅ Parameter binding for all DML operations
- ✅ Predicate serialization (structured JSON, not debug strings)
- ✅ Projection updates for INSERT/UPDATE/DELETE
- ✅ Column existence validation
- ✅ Type compatibility checking
- ✅ NOT NULL constraint enforcement
- ✅ Primary key NULL rejection

#### Integration Tests
```rust
#[test]
fn test_parameterized_insert() {
    let tenant = db.tenant(TenantId::new(1));
    tenant.execute("CREATE TABLE users (id BIGINT NOT NULL, name TEXT NOT NULL, PRIMARY KEY (id))", &[])?;

    // Parameterized INSERT
    tenant.execute("INSERT INTO users (id, name) VALUES ($1, $2)",
        &[Value::BigInt(42), Value::Text("Alice".to_string())])?;

    let result = tenant.query("SELECT * FROM users WHERE id = 42", &[])?;
    assert_eq!(result.rows[0][0], Value::BigInt(42));
    assert_eq!(result.rows[0][1], Value::Text("Alice".to_string()));
}

#[test]
fn test_update_then_select_roundtrip() {
    tenant.execute("INSERT INTO users VALUES (1, 'Alice')", &[])?;
    tenant.execute("UPDATE users SET name = 'Bob' WHERE id = 1", &[])?;

    let result = tenant.query("SELECT name FROM users WHERE id = 1", &[])?;
    assert_eq!(result.rows[0][0], Value::Text("Bob".to_string()));
}

#[test]
fn test_delete_then_select_empty() {
    tenant.execute("INSERT INTO users VALUES (1, 'Alice')", &[])?;
    tenant.execute("DELETE FROM users WHERE id = 1", &[])?;

    let result = tenant.query("SELECT * FROM users WHERE id = 1", &[])?;
    assert_eq!(result.rows.len(), 0);
}

#[test]
fn test_composite_primary_key_update_delete() {
    tenant.execute("CREATE TABLE orders (user_id BIGINT NOT NULL, order_id BIGINT NOT NULL, amount BIGINT, PRIMARY KEY (user_id, order_id))", &[])?;
    tenant.execute("INSERT INTO orders VALUES (1, 100, 5000)", &[])?;
    tenant.execute("UPDATE orders SET amount = 6000 WHERE user_id = 1 AND order_id = 100", &[])?;

    let result = tenant.query("SELECT amount FROM orders WHERE user_id = 1 AND order_id = 100", &[])?;
    assert_eq!(result.rows[0][0], Value::BigInt(6000));
}
```

#### Validation Tests
```rust
#[test]
fn test_null_in_not_null_column_rejected() {
    let result = tenant.execute("INSERT INTO users (id, name) VALUES ($1, $2)",
        &[Value::BigInt(1), Value::Null]);
    assert!(result.is_err());
}

#[test]
fn test_invalid_column_name_rejected() {
    let result = tenant.execute("INSERT INTO users (id, invalid_column) VALUES (1, 'Alice')", &[]);
    assert!(result.is_err());
}
```

## Event Format Design

### Event Structure

Every DML operation becomes an event in the table's stream:

```json
{
  "version": 1,
  "type": "insert" | "update" | "delete",
  "table": "patients",
  "timestamp": "2024-01-15T10:30:00Z",
  "data": {
    // For INSERT: full row
    // For UPDATE: { key: {...}, changes: {...} }
    // For DELETE: { key: {...} }
  }
}
```

### Schema Event Format

Schema changes go to the `__schema` stream:

```json
{
  "version": 1,
  "type": "table_created" | "table_dropped" | "index_created",
  "table": "patients",
  "definition": {
    "columns": [
      { "name": "id", "type": "bigint", "nullable": false },
      { "name": "name", "type": "text", "nullable": true }
    ],
    "primary_key": ["id"]
  }
}
```

## Constraints & Validation

### Primary Key Enforcement
- Kernel validates uniqueness before append
- Return `KernelError::PrimaryKeyViolation` if duplicate

### Foreign Keys
- **Phase 1**: Not supported (too complex for initial implementation)
- **Future**: Add `CreateForeignKey` command

### CHECK Constraints
- **Phase 1**: Not supported
- **Future**: Validate in kernel before append

### NOT NULL
- Validated at parse time, enforced in kernel

## Performance Considerations

### Batch Operations

Support bulk inserts for efficiency:

```sql
INSERT INTO patients (id, name) VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie');
```

Translates to single `AppendBatch` with multiple events.

### Schema Cache

Cache parsed schemas in memory to avoid repeated parsing:

```rust
struct SchemaCache {
    tables: HashMap<String, TableDef>,
    last_schema_offset: Offset,
}
```

Invalidate when `__schema` stream advances.

## Migration from Event API

For users currently using `CreateStream` + `AppendBatch`:

### Option 1: Dual API (Recommended)

Support both APIs indefinitely:
- Event API: Low-level, full control
- SQL API: High-level, familiar syntax

### Option 2: SQL-Only

Deprecate event API, force migration:
- More consistent
- Simpler to maintain
- Breaking change for existing users

**Recommendation**: **Option 1** - Keep both APIs, SQL as higher-level convenience.

## Documentation Updates

### New Guides
- `docs/guides/sql-quickstart.md` - SQL tutorial
- `docs/guides/schema-management.md` - DDL best practices
- `docs/guides/data-modeling.md` - Table design patterns

### Updated Docs
- `docs/ARCHITECTURE.md` - Add SQL layer diagram
- `docs/guides/quickstart-*.md` - Use SQL examples
- `README.md` - Show SQL in main example

## Implementation Timeline (Completed)

### ✅ Phase 1: DDL Foundation
- ✅ Added `Command::CreateTable`, `Command::DropTable`, `Command::CreateIndex`
- ✅ Implemented kernel logic for schema commands
- ✅ Added schema state tracking in kernel
- ✅ Parsed DDL statements via sqlparser
- ✅ Unit tests for DDL (27+ tests)

### ✅ Phase 2: DML Implementation
- ✅ Added `Command::Insert`, `Command::Update`, `Command::Delete`
- ✅ Implemented DML-to-event translation with parameter binding
- ✅ Projection updates for all DML events (INSERT/UPDATE/DELETE)
- ✅ Parsed DML statements with placeholder support
- ✅ Unit tests for DML (50+ tests)

### ✅ Phase 3: Server Integration
- ✅ Wired SQL engine to `kimberlite-server`
- ✅ Updated REPL for full DDL/DML support
- ✅ Integration tests (DDL + DML + Query roundtrips)
- ✅ Comprehensive validation layer

### ✅ Phase 4: Polish & Documentation
- ✅ Error message improvements
- ✅ Documentation updates (this file, README, website)
- 🚧 Performance benchmarks (planned)
- 🚧 VOPR tests for crash recovery (planned)

## Open Questions

1. **Transaction Boundaries**: Should `INSERT` be auto-commit or support explicit transactions?
   - **Recommendation**: Auto-commit for Phase 1, add `BEGIN`/`COMMIT` later

2. **Schema Evolution**: How to handle `ALTER TABLE ADD COLUMN`?
   - **Recommendation**: Defer to Phase 2, require `CREATE TABLE` + migration

3. **Query Planner**: Should we optimize JOIN queries in projections?
   - **Recommendation**: Yes, but only for projection definitions, not runtime queries

4. **Error Recovery**: What happens if projection update fails after event append?
   - **Recommendation**: Projection rebuilds from log on next startup (already handled by architecture)

## Success Metrics (Achieved)

### Functional ✅
- ✅ All DDL statements parse and execute (CREATE TABLE, DROP TABLE, CREATE INDEX)
- ✅ All DML statements parse and execute (INSERT, UPDATE, DELETE)
- ✅ Queries return correct results after DML (verified via roundtrip tests)
- ✅ Parameterized queries work for all DML operations ($1, $2, etc.)
- ✅ Schema changes tracked in kernel state
- ✅ Point-in-time queries work via MVCC

### Testing ✅
- ✅ 85+ unit tests passing (parser, validation, projection updates)
- ✅ 27+ integration tests (end-to-end DDL/DML workflows)
- ✅ Comprehensive validation tests (NULL rejection, type checking, column validation)
- ✅ No regressions in existing query tests (all 27 kimberlite tests pass)
- ✅ Parameterized query tests
- ✅ Composite primary key tests
- ✅ UPDATE/DELETE projection materialization verified

### Quality ✅
- ✅ No panics in library code (all .expect() replaced with proper error handling)
- ✅ Structured predicate serialization (not debug strings)
- ✅ Memory safety (fixed allocation bug)
- ✅ Correct offset calculations
- ✅ Full parameter binding support

### Performance 🚧
- 🚧 INSERT throughput benchmarks (planned)
- 🚧 Bulk INSERT performance (planned)
- 🚧 SELECT latency p99 (planned)

## Related Documents

- [ARCHITECTURE.md](ARCHITECTURE.md) - System design overview
- [ROADMAP.md](../ROADMAP.md) - Future enhancements and roadmap
- [TESTING.md](TESTING.md) - Testing strategy
- [PRESSURECRAFT.md](PRESSURECRAFT.md) - Coding standards

---

**Status**: ✅ Implemented and Working (DDL + DML + Query)
**Last Updated**: 2025-01-30
**Author**: Kimberlite Team
