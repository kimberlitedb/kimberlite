//! AUDIT-2026-04 S3.2 — SQL window functions.
//!
//! Supports `ROW_NUMBER()`, `RANK()`, `DENSE_RANK()`, `LAG()`,
//! `LEAD()`, `FIRST_VALUE()`, and `LAST_VALUE()` with `PARTITION BY`
//! and `ORDER BY`. No frame clauses (the default frame for ranking
//! functions is `RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW`,
//! which is what these implementations apply).
//!
//! # Execution model
//!
//! Window functions execute as a *post-pass* over the rows produced
//! by the underlying SELECT. This sidesteps the need for a
//! `Plan::Window` node and keeps the change additive — see
//! `apply_window_fns` in `lib.rs`.
//!
//! Pseudo-code:
//!
//! ```text
//! for fn in window_fns:
//!     sort rows by (partition_keys ++ order_keys)
//!     iterate rows once:
//!         on partition boundary: reset rank counters
//!         compute fn value, append to row
//! ```
//!
//! Determinism: the sort uses a stable comparator over typed values
//! (the `sort_rows` helper from executor.rs), so two equal rows
//! retain their original order — a property `LAG`/`LEAD` rely on.

use std::cmp::Ordering;

use crate::error::{QueryError, Result};
use crate::executor::{QueryResult, Row};
use crate::parser::ParsedWindowFn;
use crate::schema::ColumnName;
use crate::value::Value;

/// Window function operations supported by the engine.
///
/// `ROW_NUMBER` / `RANK` / `DENSE_RANK` are pure ranking functions
/// (no args). `LAG` / `LEAD` look at a sibling row offset
/// (default 1). `FIRST_VALUE` / `LAST_VALUE` return the column at
/// the partition boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowFunction {
    RowNumber,
    Rank,
    DenseRank,
    /// `LAG(column, offset = 1)` — value `offset` rows back, NULL
    /// if before partition start.
    Lag {
        column: ColumnName,
        offset: usize,
    },
    /// `LEAD(column, offset = 1)` — value `offset` rows forward,
    /// NULL if past partition end.
    Lead {
        column: ColumnName,
        offset: usize,
    },
    /// `FIRST_VALUE(column)` — value of `column` at the first row
    /// of the current partition (under the ORDER BY).
    FirstValue {
        column: ColumnName,
    },
    /// `LAST_VALUE(column)` — value of `column` at the last row
    /// of the current partition. Per ANSI default frame, "last
    /// row" here means the *current* row — so we treat
    /// `LAST_VALUE` with no explicit frame as "value of column on
    /// the current row". Postgres parity in `tests/`.
    LastValue {
        column: ColumnName,
    },
}

impl WindowFunction {
    /// Output column name when no alias is present.
    pub fn default_alias(&self) -> &'static str {
        match self {
            Self::RowNumber => "row_number",
            Self::Rank => "rank",
            Self::DenseRank => "dense_rank",
            Self::Lag { .. } => "lag",
            Self::Lead { .. } => "lead",
            Self::FirstValue { .. } => "first_value",
            Self::LastValue { .. } => "last_value",
        }
    }
}

/// Apply each window function in order to the rows produced by the
/// underlying SELECT. Returns a new [`QueryResult`] with the
/// window-function output columns appended in left-to-right order.
///
/// `result.rows` is consumed. The base columns are preserved at
/// their original positions; window output columns are appended in
/// the order the parser saw them.
pub fn apply_window_fns(base: QueryResult, window_fns: &[ParsedWindowFn]) -> Result<QueryResult> {
    if window_fns.is_empty() {
        return Ok(base);
    }

    // Resolve column indices for each window fn's
    // partition_by + order_by + arg references against base.columns.
    let columns_idx = build_column_index(&base.columns);

    let QueryResult { columns, rows } = base;
    let mut out_columns = columns.clone();

    // Each window fn produces one new column. Compute one fn at a
    // time; the algorithm needs the rows sorted by that fn's
    // (partition_by ++ order_by), so re-sort per fn. For
    // partition_by = [] and order_by = [] (whole-table frame) the
    // sort is a no-op.
    let mut work_rows = rows;
    let original_index_col = work_rows.len(); // sentinel marker (unused)
    let _ = original_index_col;

    // Stamp each row with its original index so we can restore
    // ordering at the end. The original column positions stay
    // unchanged; we only append window-fn output columns.
    let mut indexed: Vec<(usize, Row)> = work_rows.drain(..).enumerate().collect();

    for win in window_fns {
        let fn_col = compute_window_column(win, &mut indexed, &columns_idx)?;
        out_columns.push(ColumnName::new(
            win.alias
                .clone()
                .unwrap_or_else(|| win.function.default_alias().to_string()),
        ));
        for ((_, row), val) in indexed.iter_mut().zip(fn_col.into_iter()) {
            row.push(val);
        }
    }

    // Restore original input order so callers see rows in the
    // pre-window position (the SELECT's own ORDER BY, if any, ran
    // before this point).
    indexed.sort_by_key(|(idx, _)| *idx);
    let final_rows = indexed.into_iter().map(|(_, r)| r).collect();

    Ok(QueryResult {
        columns: out_columns,
        rows: final_rows,
    })
}

/// Resolve column name → row index for the base columns.
fn build_column_index(columns: &[ColumnName]) -> Vec<(String, usize)> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str().to_string(), i))
        .collect()
}

fn lookup_col(idx: &[(String, usize)], name: &str) -> Result<usize> {
    idx.iter()
        .find(|(n, _)| n == name)
        .map(|(_, i)| *i)
        .ok_or_else(|| {
            QueryError::ParseError(format!(
                "window function references unknown column '{name}'"
            ))
        })
}

/// Compute the window-function column for `win` over `indexed_rows`.
///
/// Mutates `indexed_rows` (re-sorts by partition + order) so the
/// caller can append the resulting Vec<Value> column-wise.
fn compute_window_column(
    win: &ParsedWindowFn,
    indexed_rows: &mut [(usize, Row)],
    columns_idx: &[(String, usize)],
) -> Result<Vec<Value>> {
    // Resolve indices once.
    let partition_idx: Vec<usize> = win
        .partition_by
        .iter()
        .map(|c| lookup_col(columns_idx, c.as_str()))
        .collect::<Result<_>>()?;
    let order_idx: Vec<(usize, bool)> = win
        .order_by
        .iter()
        .map(|c| Ok((lookup_col(columns_idx, c.column.as_str())?, c.ascending)))
        .collect::<Result<_>>()?;

    indexed_rows
        .sort_by(|(_, a), (_, b)| compare_partition_then_order(a, b, &partition_idx, &order_idx));

    let n = indexed_rows.len();
    let mut out = vec![Value::Null; n];

    let mut row_num: i64 = 0;
    let mut rank: i64 = 0;
    let mut dense_rank: i64 = 0;
    let mut last_partition_key: Option<Vec<Value>> = None;
    let mut last_order_key: Option<Vec<Value>> = None;

    for i in 0..n {
        let row = &indexed_rows[i].1;
        let part_key: Vec<Value> = partition_idx.iter().map(|&j| row[j].clone()).collect();
        let ord_key: Vec<Value> = order_idx.iter().map(|&(j, _)| row[j].clone()).collect();

        let new_partition = last_partition_key.as_ref() != Some(&part_key);
        if new_partition {
            row_num = 0;
            rank = 0;
            dense_rank = 0;
            last_partition_key = Some(part_key.clone());
            last_order_key = None;
        }

        row_num += 1;
        let order_changed = last_order_key.as_ref() != Some(&ord_key);
        if order_changed {
            rank = row_num;
            dense_rank += 1;
            last_order_key = Some(ord_key.clone());
        }

        out[i] = match &win.function {
            WindowFunction::RowNumber => Value::BigInt(row_num),
            WindowFunction::Rank => Value::BigInt(rank),
            WindowFunction::DenseRank => Value::BigInt(dense_rank),
            WindowFunction::Lag { column, offset } => lookup_offset(
                indexed_rows,
                columns_idx,
                &partition_idx,
                column,
                i,
                -(*offset as isize),
            )?,
            WindowFunction::Lead { column, offset } => lookup_offset(
                indexed_rows,
                columns_idx,
                &partition_idx,
                column,
                i,
                *offset as isize,
            )?,
            WindowFunction::FirstValue { column } => {
                first_in_partition(indexed_rows, columns_idx, column, i, &partition_idx)?
            }
            WindowFunction::LastValue { column } => {
                // ANSI default frame for LAST_VALUE without an
                // explicit frame is the current row — mirror that.
                let col_i = lookup_col(columns_idx, column.as_str())?;
                indexed_rows[i].1[col_i].clone()
            }
        };
    }
    Ok(out)
}

fn compare_partition_then_order(
    a: &Row,
    b: &Row,
    partition_idx: &[usize],
    order_idx: &[(usize, bool)],
) -> Ordering {
    for &j in partition_idx {
        match cmp_values(&a[j], &b[j]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    for &(j, asc) in order_idx {
        let ord = cmp_values(&a[j], &b[j]);
        match ord {
            Ordering::Equal => {}
            other => return if asc { other } else { other.reverse() },
        }
    }
    Ordering::Equal
}

/// Best-effort total order over `Value`. NULLs sort first, mirroring
/// PostgreSQL's `NULLS FIRST` ascending default.
///
/// Arms look identical (`x.cmp(y)`) across integer-family variants but
/// each is intentionally its own arm — the binding type differs
/// (`i64` / `i32` / `i16` / `i8` / timestamp-ns), and merging them
/// would tangle semantic equality. Silence the lint here.
#[allow(clippy::match_same_arms)]
fn cmp_values(a: &Value, b: &Value) -> Ordering {
    use Value::{BigInt, Boolean, Date, Integer, Null, Real, SmallInt, Text, Time, TinyInt};
    match (a, b) {
        (Null, Null) => Ordering::Equal,
        (Null, _) => Ordering::Less,
        (_, Null) => Ordering::Greater,
        (BigInt(x), BigInt(y)) => x.cmp(y),
        (Integer(x), Integer(y)) => x.cmp(y),
        (SmallInt(x), SmallInt(y)) => x.cmp(y),
        (TinyInt(x), TinyInt(y)) => x.cmp(y),
        (Real(x), Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Text(x), Text(y)) => x.cmp(y),
        (Boolean(x), Boolean(y)) => x.cmp(y),
        (Date(x), Date(y)) => x.cmp(y),
        (Time(x), Time(y)) => x.cmp(y),
        // Cross-type or unhandled: fall back to debug-string compare so
        // sort is total. Real-world window queries don't hit this since
        // schema enforces typed columns; the fallback exists for safety.
        (lhs, rhs) => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
    }
}

fn lookup_offset(
    indexed: &[(usize, Row)],
    columns_idx: &[(String, usize)],
    partition_idx: &[usize],
    column: &ColumnName,
    i: usize,
    delta: isize,
) -> Result<Value> {
    let col_i = lookup_col(columns_idx, column.as_str())?;
    let target_pos = i as isize + delta;
    if target_pos < 0 || (target_pos as usize) >= indexed.len() {
        return Ok(Value::Null);
    }
    let target = target_pos as usize;
    // Partition-boundary check: LAG/LEAD must NOT cross partition
    // boundaries — a row at the start of partition B should report
    // NULL even though indexed[i-1] holds the last row of A.
    if !same_partition(&indexed[i].1, &indexed[target].1, partition_idx) {
        return Ok(Value::Null);
    }
    Ok(indexed[target].1[col_i].clone())
}

fn same_partition(a: &Row, b: &Row, partition_idx: &[usize]) -> bool {
    partition_idx.iter().all(|&j| a[j] == b[j])
}

fn first_in_partition(
    indexed: &[(usize, Row)],
    columns_idx: &[(String, usize)],
    column: &ColumnName,
    i: usize,
    partition_idx: &[usize],
) -> Result<Value> {
    let col_i = lookup_col(columns_idx, column.as_str())?;
    let current_part: Vec<Value> = partition_idx
        .iter()
        .map(|&j| indexed[i].1[j].clone())
        .collect();
    // Walk back to the partition start.
    let mut start = i;
    while start > 0 {
        let prev_part: Vec<Value> = partition_idx
            .iter()
            .map(|&j| indexed[start - 1].1[j].clone())
            .collect();
        if prev_part != current_part {
            break;
        }
        start -= 1;
    }
    Ok(indexed[start].1[col_i].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::OrderByClause;
    use crate::schema::ColumnName;

    fn cols(names: &[&str]) -> Vec<ColumnName> {
        names.iter().map(|n| ColumnName::new(*n)).collect()
    }

    fn row(vals: Vec<Value>) -> Row {
        vals
    }

    fn order_asc(name: &str) -> OrderByClause {
        OrderByClause {
            column: ColumnName::new(name),
            ascending: true,
        }
    }

    #[test]
    fn row_number_no_partition_no_order_assigns_1_to_n_in_input_order() {
        let qr = QueryResult {
            columns: cols(&["id"]),
            rows: vec![row(vec![Value::BigInt(10)]), row(vec![Value::BigInt(20)])],
        };
        let win = ParsedWindowFn {
            function: WindowFunction::RowNumber,
            partition_by: vec![],
            order_by: vec![],
            alias: None,
        };
        let out = apply_window_fns(qr, &[win]).expect("apply");
        assert_eq!(out.columns.len(), 2);
        assert_eq!(out.rows[0][1], Value::BigInt(1));
        assert_eq!(out.rows[1][1], Value::BigInt(2));
    }

    #[test]
    fn row_number_resets_per_partition() {
        let qr = QueryResult {
            columns: cols(&["dept", "salary"]),
            rows: vec![
                row(vec![Value::Text("A".into()), Value::BigInt(100)]),
                row(vec![Value::Text("B".into()), Value::BigInt(200)]),
                row(vec![Value::Text("A".into()), Value::BigInt(150)]),
                row(vec![Value::Text("B".into()), Value::BigInt(250)]),
            ],
        };
        let win = ParsedWindowFn {
            function: WindowFunction::RowNumber,
            partition_by: vec![ColumnName::new("dept")],
            order_by: vec![order_asc("salary")],
            alias: Some("rn".into()),
        };
        let out = apply_window_fns(qr, &[win]).expect("apply");
        // Rows preserved in input order; locate by (dept, salary).
        let map: std::collections::HashMap<(String, i64), i64> = out
            .rows
            .iter()
            .map(|r| {
                let dept = match &r[0] {
                    Value::Text(s) => s.clone(),
                    _ => panic!(),
                };
                let salary = match &r[1] {
                    Value::BigInt(i) => *i,
                    _ => panic!(),
                };
                let rn = match &r[2] {
                    Value::BigInt(i) => *i,
                    _ => panic!(),
                };
                ((dept, salary), rn)
            })
            .collect();
        // A's lowest salary (100) → rn=1; A's next (150) → rn=2.
        assert_eq!(map.get(&("A".into(), 100)), Some(&1));
        assert_eq!(map.get(&("A".into(), 150)), Some(&2));
        assert_eq!(map.get(&("B".into(), 200)), Some(&1));
        assert_eq!(map.get(&("B".into(), 250)), Some(&2));
    }

    #[test]
    fn rank_and_dense_rank_distinguish_ties() {
        // Three rows with salaries 100, 100, 200 — RANK = 1, 1, 3;
        // DENSE_RANK = 1, 1, 2. PostgreSQL parity.
        let qr = QueryResult {
            columns: cols(&["salary"]),
            rows: vec![
                row(vec![Value::BigInt(100)]),
                row(vec![Value::BigInt(100)]),
                row(vec![Value::BigInt(200)]),
            ],
        };
        let win_rank = ParsedWindowFn {
            function: WindowFunction::Rank,
            partition_by: vec![],
            order_by: vec![order_asc("salary")],
            alias: Some("r".into()),
        };
        let win_dense = ParsedWindowFn {
            function: WindowFunction::DenseRank,
            partition_by: vec![],
            order_by: vec![order_asc("salary")],
            alias: Some("dr".into()),
        };
        let out = apply_window_fns(qr, &[win_rank, win_dense]).expect("apply");
        // After post-pass the rows are restored to input order.
        // Salary 100 (twice) → r=1, dr=1; salary 200 → r=3, dr=2.
        for r in &out.rows {
            let salary = match &r[0] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            let rank = match &r[1] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            let dense = match &r[2] {
                Value::BigInt(i) => *i,
                _ => panic!(),
            };
            if salary == 100 {
                assert_eq!(rank, 1, "rank ties");
                assert_eq!(dense, 1, "dense_rank ties");
            } else {
                assert_eq!(rank, 3, "rank skips after ties");
                assert_eq!(dense, 2, "dense_rank does not skip");
            }
        }
    }

    #[test]
    fn first_value_returns_partition_start_value() {
        let qr = QueryResult {
            columns: cols(&["dept", "salary"]),
            rows: vec![
                row(vec![Value::Text("A".into()), Value::BigInt(300)]),
                row(vec![Value::Text("A".into()), Value::BigInt(100)]),
                row(vec![Value::Text("A".into()), Value::BigInt(200)]),
            ],
        };
        let win = ParsedWindowFn {
            function: WindowFunction::FirstValue {
                column: ColumnName::new("salary"),
            },
            partition_by: vec![ColumnName::new("dept")],
            order_by: vec![order_asc("salary")],
            alias: Some("first".into()),
        };
        let out = apply_window_fns(qr, &[win]).expect("apply");
        // All three rows must report the partition's lowest salary.
        for r in &out.rows {
            assert_eq!(r[2], Value::BigInt(100));
        }
    }

    #[test]
    fn lag_returns_null_at_partition_start() {
        let qr = QueryResult {
            columns: cols(&["id"]),
            rows: vec![
                row(vec![Value::BigInt(10)]),
                row(vec![Value::BigInt(20)]),
                row(vec![Value::BigInt(30)]),
            ],
        };
        let win = ParsedWindowFn {
            function: WindowFunction::Lag {
                column: ColumnName::new("id"),
                offset: 1,
            },
            partition_by: vec![],
            order_by: vec![order_asc("id")],
            alias: Some("prev".into()),
        };
        let out = apply_window_fns(qr, &[win]).expect("apply");
        // After sorting by id asc and reapplying input order, the
        // row with id=10 (first by id) gets NULL, id=20 gets 10,
        // id=30 gets 20.
        let map: std::collections::HashMap<i64, Value> = out
            .rows
            .iter()
            .map(|r| {
                let id = match &r[0] {
                    Value::BigInt(i) => *i,
                    _ => panic!(),
                };
                (id, r[1].clone())
            })
            .collect();
        assert_eq!(map[&10], Value::Null, "first row lag is NULL");
        assert_eq!(map[&20], Value::BigInt(10));
        assert_eq!(map[&30], Value::BigInt(20));
    }
}
