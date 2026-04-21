---
title: "Correlated Subqueries"
section: "coding/recipes"
slug: "correlated-subqueries"
order: 6
---

# Correlated Subqueries

Kimberlite's SQL engine supports `EXISTS`, `NOT EXISTS`, `IN (SELECT)`,
and `NOT IN (SELECT)` with **correlated** inner queries — the inner
SELECT may reference columns from the enclosing outer query.

See `docs/reference/sql/correlated-subqueries.md` for the full design
(scope stack, decorrelation heuristics, cardinality guard).

## Healthcare golden query

The motivating use case: return every patient with an active
`HealthcareDelivery` consent. The inner `SELECT 1 FROM consent_current c
WHERE c.subject_id = p.id` depends on the outer patient row — a
classical correlated EXISTS.

```rust,ignore
use bytes::Bytes;
use kimberlite_query::{
    ColumnDef, DataType, QueryEngine, Schema, SchemaBuilder, Value,
};
use kimberlite_query::key_encoder::encode_key;
use kimberlite_store::{Key, ProjectionStore, StoreError, TableId, WriteBatch, WriteOp};
use kimberlite_types::Offset;
use std::collections::HashMap;
use std::ops::Range;

// Minimal in-memory ProjectionStore (for the doc-test only; production
// code uses `kimberlite::BTreeStore` or the clustered store).
#[derive(Default)]
struct MemStore {
    tables: HashMap<TableId, Vec<(Key, Bytes)>>,
    position: Offset,
}

impl ProjectionStore for MemStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<(), StoreError> {
        for op in batch.operations() {
            if let WriteOp::Put { table, key, value } = op {
                let t = self.tables.entry(*table).or_default();
                t.push((key.clone(), value.clone()));
                t.sort_by(|a, b| a.0.cmp(&b.0));
            }
        }
        self.position = batch.position();
        Ok(())
    }
    fn applied_position(&self) -> Offset { self.position }
    fn get(&mut self, table: TableId, key: &Key) -> Result<Option<Bytes>, StoreError> {
        Ok(self
            .tables
            .get(&table)
            .and_then(|t| t.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v.clone()))
    }
    fn get_at(&mut self, t: TableId, k: &Key, _p: Offset) -> Result<Option<Bytes>, StoreError> {
        self.get(t, k)
    }
    fn scan(
        &mut self,
        table: TableId,
        range: Range<Key>,
        limit: usize,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        Ok(self
            .tables
            .get(&table)
            .map(|t| {
                t.iter()
                    .filter(|(k, _)| k >= &range.start && k < &range.end)
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    fn scan_at(
        &mut self,
        t: TableId,
        r: Range<Key>,
        l: usize,
        _p: Offset,
    ) -> Result<Vec<(Key, Bytes)>, StoreError> {
        self.scan(t, r, l)
    }
    fn sync(&mut self) -> Result<(), StoreError> { Ok(()) }
}

fn insert_json(
    store: &mut MemStore,
    table_id: TableId,
    pk: Value,
    obj: serde_json::Value,
) {
    let key = encode_key(&[pk]);
    let bytes = Bytes::from(serde_json::to_vec(&obj).unwrap());
    let t = store.tables.entry(table_id).or_default();
    t.push((key, bytes));
    t.sort_by(|a, b| a.0.cmp(&b.0));
}

fn main() {
    let schema: Schema = SchemaBuilder::new()
        .table(
            "patient_current",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text),
            ],
            vec!["id".into()],
        )
        .table(
            "consent_current",
            TableId::new(2),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("subject_id", DataType::BigInt).not_null(),
                ColumnDef::new("purpose", DataType::Text),
                ColumnDef::new("withdrawn_at", DataType::Timestamp),
            ],
            vec!["id".into()],
        )
        .build();

    let mut store = MemStore::default();

    // Three patients. Alice and Bob have active HealthcareDelivery
    // consents; Charlie's was withdrawn.
    insert_json(
        &mut store,
        TableId::new(1),
        Value::BigInt(1),
        serde_json::json!({"id": 1, "name": "Alice"}),
    );
    insert_json(
        &mut store,
        TableId::new(1),
        Value::BigInt(2),
        serde_json::json!({"id": 2, "name": "Bob"}),
    );
    insert_json(
        &mut store,
        TableId::new(1),
        Value::BigInt(3),
        serde_json::json!({"id": 3, "name": "Charlie"}),
    );

    // Alice: active HealthcareDelivery.
    insert_json(
        &mut store,
        TableId::new(2),
        Value::BigInt(10),
        serde_json::json!({
            "id": 10, "subject_id": 1, "purpose": "HealthcareDelivery",
            "withdrawn_at": null,
        }),
    );
    // Bob: active HealthcareDelivery.
    insert_json(
        &mut store,
        TableId::new(2),
        Value::BigInt(11),
        serde_json::json!({
            "id": 11, "subject_id": 2, "purpose": "HealthcareDelivery",
            "withdrawn_at": null,
        }),
    );
    // Charlie: consent exists but withdrawn.
    insert_json(
        &mut store,
        TableId::new(2),
        Value::BigInt(12),
        serde_json::json!({
            "id": 12, "subject_id": 3, "purpose": "HealthcareDelivery",
            "withdrawn_at": 1_700_000_000_000_000_000i64,
        }),
    );

    let engine = QueryEngine::new(schema);

    // The golden query — correlated EXISTS against a filtered inner.
    let result = engine
        .query(
            &mut store,
            "SELECT id FROM patient_current p \
             WHERE EXISTS ( \
               SELECT id FROM consent_current c \
               WHERE c.subject_id = p.id \
                 AND c.purpose = 'HealthcareDelivery' \
                 AND c.withdrawn_at IS NULL \
             ) \
             ORDER BY id",
            &[],
        )
        .unwrap();

    // Alice (1) + Bob (2). Charlie (3)'s consent is withdrawn.
    let ids: Vec<i64> = result
        .rows
        .iter()
        .map(|r| match &r[0] {
            Value::BigInt(n) => *n,
            other => panic!("expected BigInt, got {other:?}"),
        })
        .collect();
    assert_eq!(ids, vec![1, 2]);
}
```

## How it plans

The query above is routed through the semi-join decorrelation path
(single equijoin `c.subject_id = p.id`, additional static predicates
on inner columns). Internally it is rewritten to:

```sql
SELECT id FROM patient_current p
WHERE p.id IN (
  SELECT c.subject_id FROM consent_current c
  WHERE c.purpose = 'HealthcareDelivery'
    AND c.withdrawn_at IS NULL
)
ORDER BY id
```

— which uses the same pre-execute fast path as v0.5.0 uncorrelated
subqueries. No per-outer-row re-planning is needed.

## Fallback: non-equijoin correlations

When an inner predicate references an outer column in a non-equijoin
position — e.g. `WHERE c.created_at > p.last_seen` — decorrelation
isn't provable, and the engine falls back to a correlated loop. See
`docs/reference/sql/correlated-subqueries.md` for when that happens
and the cardinality guard that prevents runaway cost.
