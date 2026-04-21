//! AUDIT-2026-04 S3.1 — translate parsed DML AST into the wire-event
//! payloads that `kimberlite-kernel`'s `Command::Insert` /
//! `Command::Update` / `Command::Delete` carry as `row_data`.
//!
//! # Why a separate module?
//!
//! The DML execution path historically lived in `kimberlite/src/tenant.rs`
//! (~600 LoC of inline JSON-event construction). That code is correct
//! but is reused nowhere else — VOPR scenarios, oracle drivers, and
//! the upcoming `KernelHandle`-based test helpers all want to *plan*
//! a DML statement without going through the full TenantHandle stack.
//!
//! This module is the planner-side counterpart to `executor.rs`:
//! takes a `ParsedInsert/Update/Delete` plus bound parameters, returns
//! a typed `DmlPlan` that downstream callers (the runtime, tests,
//! tooling) can submit through any `KernelHandle` impl.
//!
//! ## Layering
//!
//! Stays free of a hard dependency on `kimberlite-kernel`: the
//! `KernelHandle` trait below treats the kernel's `Command` as an
//! opaque associated type. The runtime adapter in
//! `kimberlite/src/tenant.rs` knows the concrete `Command` enum and
//! provides the trait impl.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use kimberlite_types::TenantId;
use serde_json::{Value as JsonValue, json};

use crate::error::{QueryError, Result};
use crate::parser::{ParsedDelete, ParsedInsert, ParsedUpdate, Predicate, PredicateValue};
use crate::value::Value;

// ============================================================================
// DmlPlan
// ============================================================================

/// A planned DML statement: the table id is resolved by the caller
/// (since it lives in kernel/runtime metadata), but everything else
/// — column-to-value mapping, WHERE predicates, primary-key
/// extraction — is decided here.
///
/// Not derived `PartialEq`/`Eq` because [`Predicate`] doesn't impl
/// them; tests assert via field-by-field destructuring instead.
#[derive(Debug, Clone)]
pub struct DmlPlan {
    /// Tenant the statement targets. Threaded through so the
    /// `KernelHandle` impl can build the matching `Command` variant
    /// without re-reading the request context.
    pub tenant_id: TenantId,
    /// Target table name. The runtime resolves this to a `TableId`.
    pub table: String,
    /// Operation specifics.
    pub op: DmlOp,
}

/// The operation-specific payload of a [`DmlPlan`].
///
/// Kept as a separate enum so `DmlPlan` carries the
/// tenant + table fields uniformly while pattern-matching on the
/// op stays exhaustive.
#[derive(Debug, Clone)]
pub enum DmlOp {
    /// `INSERT INTO table (col1, col2, ...) VALUES (v1, v2, ...)`.
    /// The `rows` vector carries one map per VALUES tuple.
    Insert { rows: Vec<BTreeMap<String, Value>> },
    /// `UPDATE table SET col = v WHERE pk = ...`.
    /// The runtime is expected to evaluate `where_predicates`
    /// against the projection store to pick the rows; no full-table
    /// updates are emitted (that would be a separate plan node).
    Update {
        assignments: BTreeMap<String, Value>,
        where_predicates: Vec<Predicate>,
    },
    /// `DELETE FROM table WHERE pk = ...`. Same `where_predicates`
    /// semantics as `Update`.
    Delete { where_predicates: Vec<Predicate> },
}

impl DmlPlan {
    /// Render the operation as the JSON event payload that
    /// `Effect::UpdateProjection`'s replay path expects (see
    /// `apply_single_dml_event` in `kimberlite/src/kimberlite.rs`).
    /// Centralising the shape here keeps the runtime + planner in
    /// lockstep when the wire shape evolves.
    pub fn to_event_json(&self) -> Vec<JsonValue> {
        match &self.op {
            DmlOp::Insert { rows } => rows
                .iter()
                .map(|row| {
                    json!({
                        "type": "insert",
                        "table": self.table,
                        "data": json_object_from_value_map(row),
                    })
                })
                .collect(),
            DmlOp::Update {
                assignments,
                where_predicates,
            } => {
                let set_pairs: Vec<JsonValue> = assignments
                    .iter()
                    .map(|(col, v)| json!([col, value_to_json(v)]))
                    .collect();
                vec![json!({
                    "type": "update",
                    "table": self.table,
                    "set": set_pairs,
                    "where": predicates_to_json(where_predicates),
                })]
            }
            DmlOp::Delete { where_predicates } => vec![json!({
                "type": "delete",
                "table": self.table,
                "where": predicates_to_json(where_predicates),
            })],
        }
    }
}

// ============================================================================
// KernelHandle
// ============================================================================

/// Abstraction over the kernel command bus. Allows the planner +
/// downstream tooling to submit DML without naming
/// `kimberlite_kernel::Command` directly (which would force this
/// crate into a hard dependency on the kernel and re-introduce the
/// layering cycle the repo's CLAUDE.md flags).
///
/// The runtime implements this for its in-process
/// `kimberlite::TenantHandle`; tests can supply a recording mock to
/// assert planner output.
pub trait KernelHandle {
    /// Concrete error surfaced by the underlying kernel.
    type Error: std::error::Error;

    /// Submit a planned DML statement. Returns the number of rows
    /// affected; the runtime is responsible for returning RETURNING
    /// rows separately if the parsed statement requested them.
    fn submit_dml(&mut self, plan: &DmlPlan) -> std::result::Result<u64, Self::Error>;
}

// ============================================================================
// Planner functions
// ============================================================================

/// Plan an `INSERT` statement, binding `$N` placeholders against
/// `params`. Errors when the column count doesn't match a row's
/// value count or when an unknown placeholder is referenced.
pub fn plan_insert(
    tenant_id: TenantId,
    insert: &ParsedInsert,
    params: &[Value],
) -> Result<DmlPlan> {
    if insert.columns.is_empty() {
        return Err(QueryError::ParseError(
            "INSERT must specify columns explicitly for the planner — \
             schema-defaulted columns require a runtime metadata lookup"
                .into(),
        ));
    }
    let mut rows = Vec::with_capacity(insert.values.len());
    for row_values in &insert.values {
        if row_values.len() != insert.columns.len() {
            return Err(QueryError::TypeMismatch {
                expected: format!("{} values", insert.columns.len()),
                actual: format!("{} values provided", row_values.len()),
            });
        }
        let bound = bind_row(row_values, params)?;
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        for (col, val) in insert.columns.iter().zip(bound.into_iter()) {
            map.insert(col.clone(), val);
        }
        rows.push(map);
    }
    Ok(DmlPlan {
        tenant_id,
        table: insert.table.clone(),
        op: DmlOp::Insert { rows },
    })
}

/// Plan an `UPDATE` statement. The `assignments` are bound against
/// `params`; predicates are passed through unchanged so the runtime
/// can evaluate them against the projection store.
pub fn plan_update(
    tenant_id: TenantId,
    update: &ParsedUpdate,
    params: &[Value],
) -> Result<DmlPlan> {
    let mut assignments: BTreeMap<String, Value> = BTreeMap::new();
    for (col, val) in &update.assignments {
        assignments.insert(col.clone(), bind_value(val, params)?);
    }
    Ok(DmlPlan {
        tenant_id,
        table: update.table.clone(),
        op: DmlOp::Update {
            assignments,
            where_predicates: update.predicates.clone(),
        },
    })
}

/// Plan a `DELETE` statement. WHERE predicates pass through; the
/// runtime is responsible for evaluating them against rows.
pub fn plan_delete(tenant_id: TenantId, delete: &ParsedDelete) -> Result<DmlPlan> {
    Ok(DmlPlan {
        tenant_id,
        table: delete.table.clone(),
        op: DmlOp::Delete {
            where_predicates: delete.predicates.clone(),
        },
    })
}

// ============================================================================
// Helpers (kept private; the planner is the public surface)
// ============================================================================

fn bind_row(values: &[Value], params: &[Value]) -> Result<Vec<Value>> {
    values.iter().map(|v| bind_value(v, params)).collect()
}

fn bind_value(value: &Value, params: &[Value]) -> Result<Value> {
    if let Value::Placeholder(idx) = value {
        let i = *idx;
        if i == 0 || i > params.len() {
            return Err(QueryError::ParseError(format!(
                "placeholder ${i} out of range (have {} bound params)",
                params.len()
            )));
        }
        Ok(params[i - 1].clone())
    } else {
        Ok(value.clone())
    }
}

fn json_object_from_value_map(map: &BTreeMap<String, Value>) -> JsonValue {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        obj.insert(k.clone(), value_to_json(v));
    }
    JsonValue::Object(obj)
}

fn value_to_json(v: &Value) -> JsonValue {
    match v {
        Value::Null => JsonValue::Null,
        Value::Boolean(b) => JsonValue::Bool(*b),
        Value::TinyInt(i) => json!(*i),
        Value::SmallInt(i) => json!(*i),
        Value::Integer(i) => json!(*i),
        Value::BigInt(i) => json!(*i),
        Value::Real(f) => json!(*f),
        Value::Decimal(units, scale) => {
            // Render as `units` and `scale` separately so the
            // runtime can reconstruct the precise decimal without
            // round-tripping through f64. Matches what the existing
            // tenant.rs serializer does for DECIMAL columns.
            json!({"$decimal": {"units": units.to_string(), "scale": *scale}})
        }
        Value::Text(s) => json!(s),
        Value::Bytes(b) => {
            use base64::{Engine, engine::general_purpose};
            json!(general_purpose::STANDARD.encode(b.as_ref()))
        }
        Value::Date(d) => json!(d),
        Value::Time(t) => json!(t),
        Value::Timestamp(ts) => json!({"$ts": format!("{ts:?}")}),
        Value::Uuid(bytes) => {
            // Format as standard hex UUID.
            json!(uuid_bytes_to_string(bytes))
        }
        Value::Json(j) => j.clone(),
        Value::Placeholder(n) => json!({"$placeholder": n}),
    }
}

fn uuid_bytes_to_string(bytes: &[u8; 16]) -> String {
    // Standard 8-4-4-4-12 hex grouping. Avoid pulling the `uuid`
    // crate just for formatting.
    let mut out = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn predicates_to_json(preds: &[Predicate]) -> JsonValue {
    JsonValue::Array(preds.iter().map(predicate_to_json).collect())
}

fn predicate_to_json(p: &Predicate) -> JsonValue {
    match p {
        Predicate::Eq(col, pv) => json!({
            "op": "eq", "column": col.as_str(), "values": [pred_value_to_json(pv)],
        }),
        Predicate::Lt(col, pv) => json!({
            "op": "lt", "column": col.as_str(), "values": [pred_value_to_json(pv)],
        }),
        Predicate::Gt(col, pv) => json!({
            "op": "gt", "column": col.as_str(), "values": [pred_value_to_json(pv)],
        }),
        Predicate::Le(col, pv) => json!({
            "op": "le", "column": col.as_str(), "values": [pred_value_to_json(pv)],
        }),
        Predicate::Ge(col, pv) => json!({
            "op": "ge", "column": col.as_str(), "values": [pred_value_to_json(pv)],
        }),
        Predicate::In(col, pvs) => json!({
            "op": "in",
            "column": col.as_str(),
            "values": pvs.iter().map(pred_value_to_json).collect::<Vec<_>>(),
        }),
        Predicate::NotIn(col, pvs) => json!({
            "op": "not_in",
            "column": col.as_str(),
            "values": pvs.iter().map(pred_value_to_json).collect::<Vec<_>>(),
        }),
        Predicate::NotBetween(col, low, high) => json!({
            "op": "not_between",
            "column": col.as_str(),
            "low": pred_value_to_json(low),
            "high": pred_value_to_json(high),
        }),
        Predicate::ScalarCmp { op, .. } => json!({
            "op": "scalar_cmp",
            "cmp": match op {
                crate::parser::ScalarCmpOp::Eq => "eq",
                crate::parser::ScalarCmpOp::NotEq => "ne",
                crate::parser::ScalarCmpOp::Lt => "lt",
                crate::parser::ScalarCmpOp::Le => "le",
                crate::parser::ScalarCmpOp::Gt => "gt",
                crate::parser::ScalarCmpOp::Ge => "ge",
            },
        }),
        Predicate::IsNull(col) => json!({"op":"isnull","column":col.as_str()}),
        Predicate::IsNotNull(col) => json!({"op":"isnotnull","column":col.as_str()}),
        Predicate::Like(col, pat) => json!({
            "op": "like", "column": col.as_str(), "pattern": pat,
        }),
        Predicate::NotLike(col, pat) => json!({
            "op": "not_like", "column": col.as_str(), "pattern": pat,
        }),
        Predicate::ILike(col, pat) => json!({
            "op": "ilike", "column": col.as_str(), "pattern": pat,
        }),
        Predicate::NotILike(col, pat) => json!({
            "op": "not_ilike", "column": col.as_str(), "pattern": pat,
        }),
        Predicate::Or(left, right) => json!({
            "op": "or",
            "left": predicates_to_json(left),
            "right": predicates_to_json(right),
        }),
        Predicate::JsonExtractEq {
            column,
            path,
            as_text,
            value,
        } => json!({
            "op": if *as_text { "json_extract_text_eq" } else { "json_extract_eq" },
            "column": column.as_str(),
            "path": path,
            "value": pred_value_to_json(value),
        }),
        Predicate::JsonContains { column, value } => json!({
            "op": "json_contains",
            "column": column.as_str(),
            "value": pred_value_to_json(value),
        }),
        // Subquery predicates are pre-executed before reaching the DML planner;
        // if they reach here, the entry-point substitution didn't run.
        Predicate::InSubquery { column, .. } => json!({
            "op": "in_subquery_unresolved",
            "column": column.as_str(),
        }),
        Predicate::Exists { negated, .. } => json!({
            "op": if *negated { "not_exists_unresolved" } else { "exists_unresolved" },
        }),
        Predicate::Always(b) => json!({"op": if *b { "always_true" } else { "always_false" }}),
    }
}

fn pred_value_to_json(pv: &PredicateValue) -> JsonValue {
    match pv {
        PredicateValue::Int(i) => json!(i),
        PredicateValue::String(s) => json!(s),
        PredicateValue::Bool(b) => json!(b),
        PredicateValue::Null => JsonValue::Null,
        PredicateValue::Param(idx) => json!({"$placeholder": idx}),
        PredicateValue::Literal(v) => value_to_json(v),
        PredicateValue::ColumnRef(s) => json!({"$colref": s}),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedStatement;
    use crate::parser::parse_statement;
    use kimberlite_types::TenantId;

    fn parse_insert(sql: &str) -> ParsedInsert {
        match parse_statement(sql).expect("parse") {
            ParsedStatement::Insert(i) => i,
            other => panic!("expected INSERT, got {other:?}"),
        }
    }

    fn parse_update(sql: &str) -> ParsedUpdate {
        match parse_statement(sql).expect("parse") {
            ParsedStatement::Update(u) => u,
            other => panic!("expected UPDATE, got {other:?}"),
        }
    }

    fn parse_delete(sql: &str) -> ParsedDelete {
        match parse_statement(sql).expect("parse") {
            ParsedStatement::Delete(d) => d,
            other => panic!("expected DELETE, got {other:?}"),
        }
    }

    #[test]
    fn plan_insert_single_row_binds_placeholders() {
        let parsed = parse_insert("INSERT INTO patients (id, name) VALUES ($1, $2)");
        let plan = plan_insert(
            TenantId::new(7),
            &parsed,
            &[Value::BigInt(42), Value::Text("Alice".into())],
        )
        .expect("plan");
        let DmlOp::Insert { rows } = plan.op else {
            panic!("not an Insert");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("id"), Some(&Value::BigInt(42)));
        assert_eq!(rows[0].get("name"), Some(&Value::Text("Alice".into())));
    }

    #[test]
    fn plan_insert_multi_row_preserves_order() {
        let parsed =
            parse_insert("INSERT INTO patients (id, name) VALUES (1, 'a'), (2, 'b'), (3, 'c')");
        let plan = plan_insert(TenantId::new(1), &parsed, &[]).expect("plan");
        let DmlOp::Insert { rows } = plan.op else {
            panic!()
        };
        assert_eq!(rows.len(), 3);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.get("id"), Some(&Value::BigInt(i as i64 + 1)));
        }
    }

    #[test]
    fn plan_insert_rejects_arity_mismatch() {
        let parsed = parse_insert("INSERT INTO patients (id, name) VALUES (1)");
        let err = plan_insert(TenantId::new(1), &parsed, &[]).expect_err("must error");
        match err {
            QueryError::TypeMismatch { .. } => {}
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn plan_insert_event_json_matches_runtime_shape() {
        // The `apply_single_dml_event` path in
        // kimberlite/src/kimberlite.rs decodes events of the form
        // {"type":"insert","table":...,"data":{...}}. Asserting the
        // exact shape here is the contract that lets the runtime
        // adapter swap between the inline JSON construction in
        // tenant.rs and DmlPlan::to_event_json without behavioural
        // drift.
        let parsed = parse_insert("INSERT INTO patients (id, name) VALUES (1, 'alice')");
        let plan = plan_insert(TenantId::new(1), &parsed, &[]).expect("plan");
        let events = plan.to_event_json();
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev["type"], "insert");
        assert_eq!(ev["table"], "patients");
        assert_eq!(ev["data"]["id"], 1);
        assert_eq!(ev["data"]["name"], "alice");
    }

    #[test]
    fn plan_update_emits_set_pairs_in_array_form() {
        let parsed = parse_update("UPDATE patients SET name = 'bob' WHERE id = 1");
        let plan = plan_update(TenantId::new(1), &parsed, &[]).expect("plan");
        let events = plan.to_event_json();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "update");
        // SET pairs are 2-arrays so `apply_single_dml_event` can
        // index them positionally — that's the existing wire shape
        // (see set_obj[0] / set_obj[1] in apply_single_dml_event).
        let set = events[0]["set"].as_array().expect("set array");
        assert_eq!(set.len(), 1);
        let pair = set[0].as_array().expect("set entry is array");
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0], "name");
        assert_eq!(pair[1], "bob");
    }

    #[test]
    fn plan_delete_passes_predicates_through_unchanged() {
        let parsed = parse_delete("DELETE FROM patients WHERE id = 42");
        let plan = plan_delete(TenantId::new(1), &parsed).expect("plan");
        let DmlOp::Delete { where_predicates } = &plan.op else {
            panic!()
        };
        assert_eq!(where_predicates.len(), 1);
        let events = plan.to_event_json();
        assert_eq!(events[0]["type"], "delete");
        let preds = events[0]["where"].as_array().unwrap();
        assert_eq!(preds.len(), 1);
        assert_eq!(preds[0]["op"], "eq");
        assert_eq!(preds[0]["column"], "id");
    }

    /// Out-of-range placeholder produces a clear error rather than
    /// panicking. Critical for any caller binding params from
    /// untrusted input (the wire layer's `QueryParam` decoder).
    #[test]
    fn plan_insert_rejects_out_of_range_placeholder() {
        let parsed = parse_insert("INSERT INTO patients (id, name) VALUES ($1, $99)");
        let err = plan_insert(
            TenantId::new(1),
            &parsed,
            &[Value::BigInt(1), Value::Text("a".into())],
        )
        .expect_err("must reject");
        match err {
            QueryError::ParseError(msg) => assert!(
                msg.contains("$99") && msg.contains("out of range"),
                "expected $99 placeholder error, got: {msg}",
            ),
            other => panic!("expected ParseError, got {other:?}"),
        }
    }
}
