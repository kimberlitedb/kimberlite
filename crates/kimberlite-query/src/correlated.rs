//! Correlated subquery support (v0.6.0).
//!
//! See `docs/reference/sql/correlated-subqueries.md` for the full design.
//!
//! A correlated subquery is a subquery whose inner predicate tree
//! references at least one column from an enclosing (outer) scope.
//! Classic examples:
//!
//! ```sql
//! SELECT p.* FROM patient_current p
//! WHERE EXISTS (
//!   SELECT 1 FROM consent_current c
//!   WHERE c.subject_id = p.id
//! )
//! ```
//!
//! Here `p.id` inside the inner `WHERE` is an **outer reference**. This
//! module provides:
//!
//! 1. [`PlannerScope`] — a stack of visible tables used to classify a
//!    column reference as bound-in-inner vs. an outer reference.
//! 2. [`OuterRef`] — a resolved outer reference with the column name
//!    and scope depth.
//! 3. [`collect_outer_refs`] — walks a parsed `SELECT` and returns every
//!    outer reference it contains.
//! 4. [`substitute_outer_refs`] — returns a copy of a `ParsedSelect`
//!    with outer-reference `ColumnRef` values replaced by concrete
//!    literals drawn from an outer row.
//! 5. [`try_semi_join_rewrite`] — attempts to rewrite a correlated
//!    `EXISTS` / `NOT EXISTS` into a `Predicate::In` / `Predicate::NotIn`
//!    against the outer column, when the correlation is a single
//!    equijoin.

use crate::parser::{ParsedSelect, Predicate, PredicateValue};
use crate::schema::{ColumnName, Schema, TableDef};
use crate::value::Value;

/// Default cap on `outer_rows × inner_rows_per_iter` for correlated queries.
pub const DEFAULT_CORRELATED_CAP: u64 = 10_000_000;

/// A stack of table bindings visible at some point during planning.
///
/// The innermost scope sits at `scopes.last()`; enclosing scopes live
/// earlier in the vector. Lookup walks from innermost outward so
/// a correlated reference ends up carrying the depth at which it
/// resolved (0 = innermost, 1 = one level out, …). We cap depth at 2
/// today — a correlated subquery inside a correlated subquery bumps
/// depth to 1, which is the deepest anything the v0.6.0 executor
/// handles.
#[derive(Debug, Clone, Default)]
pub struct PlannerScope<'a> {
    /// Tables visible at each scope level, bound under their SQL alias
    /// (falling back to the table name itself when no alias was given).
    scopes: Vec<Vec<ScopeBinding<'a>>>,
}

/// A single `(alias, table_def)` binding inside a scope.
#[derive(Debug, Clone)]
struct ScopeBinding<'a> {
    alias: String,
    table: &'a TableDef,
}

impl<'a> PlannerScope<'a> {
    /// Empty scope stack — used as the top-level starting point.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Push a new innermost scope with the given table bindings, and
    /// return the new scope. Bindings are `(alias, table_def)`; when a
    /// FROM clause has no explicit alias, pass the table name as alias.
    #[must_use]
    pub fn push(&self, bindings: Vec<(String, &'a TableDef)>) -> Self {
        let mut scopes = self.scopes.clone();
        scopes.push(
            bindings
                .into_iter()
                .map(|(alias, table)| ScopeBinding { alias, table })
                .collect(),
        );
        Self { scopes }
    }

    /// Resolve a column reference.
    ///
    /// `qualifier` is the `alias` portion of `alias.column` (or `None`
    /// for bare `column`). Walks from the innermost scope outward.
    /// Returns `(scope_depth, table_def)` — depth 0 means resolved in
    /// the innermost scope (NOT an outer reference); depth ≥ 1 is an
    /// outer reference.
    ///
    /// Returns `None` if no scope owns the column.
    pub fn resolve(
        &self,
        qualifier: Option<&str>,
        column: &ColumnName,
    ) -> Option<(usize, &'a TableDef)> {
        // Innermost scope is at the end; walk from last to first.
        let n = self.scopes.len();
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            let depth = n - 1 - i;
            for binding in scope {
                // Qualified: match alias first, then check the table has the column.
                if let Some(q) = qualifier {
                    if binding.alias.eq_ignore_ascii_case(q)
                        && binding.table.find_column(column).is_some()
                    {
                        return Some((depth, binding.table));
                    }
                } else if binding.table.find_column(column).is_some() {
                    return Some((depth, binding.table));
                }
            }
        }
        None
    }

    /// True if at least one scope is on the stack.
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// A column reference inside a subquery that resolves to an enclosing
/// scope.
///
/// `qualifier` preserves the original `alias.` prefix so substitution
/// can match the exact `PredicateValue::ColumnRef("alias.col")` form
/// produced by the parser.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OuterRef {
    /// Alias qualifier (e.g. `"p"` in `p.id`). Always present for
    /// outer references because a bare column that happened to match
    /// an outer table gets flagged only when the inner scope has no
    /// column of the same name — and in that ambiguous case we require
    /// the qualifier for safety.
    pub qualifier: String,
    /// The column name inside the outer table.
    pub column: ColumnName,
    /// Scope depth — 1 = one level out, 2 = two levels, etc.
    pub scope_depth: usize,
}

impl OuterRef {
    /// Serialises back to the `"qualifier.column"` form used in
    /// `PredicateValue::ColumnRef`.
    pub fn as_column_ref(&self) -> String {
        format!("{}.{}", self.qualifier, self.column)
    }
}

/// Walk a parsed subquery and collect every outer-reference column.
///
/// `outer_scope` holds the enclosing query's visible tables (stacked
/// with any further-outer scopes already pushed in). `inner_tables`
/// is the list of tables visible inside `subquery` itself (FROM + all
/// JOINs) — a column that resolves to one of these is NOT an outer
/// reference.
pub fn collect_outer_refs(
    subquery: &ParsedSelect,
    outer_scope: &PlannerScope<'_>,
    schema: &Schema,
) -> Vec<OuterRef> {
    // Build the inner scope: the subquery's FROM + JOIN tables.
    let mut inner_bindings: Vec<(String, &TableDef)> = Vec::new();
    if let Some(t) = schema.get_table(&subquery.table.clone().into()) {
        inner_bindings.push((subquery.table.clone(), t));
    }
    for join in &subquery.joins {
        if let Some(t) = schema.get_table(&join.table.clone().into()) {
            inner_bindings.push((join.table.clone(), t));
        }
    }
    let inner_scope = outer_scope.push(inner_bindings);

    let mut out = Vec::new();
    for pred in &subquery.predicates {
        collect_from_predicate(pred, &inner_scope, &mut out);
    }
    // ORDER BY / GROUP BY / HAVING column references don't carry
    // qualifiers in the parser today — they are bound against the inner
    // FROM by the planner. A future extension can revisit this if we
    // ever emit qualified ORDER BY columns.
    out
}

fn collect_from_predicate(
    pred: &Predicate,
    inner_scope: &PlannerScope<'_>,
    out: &mut Vec<OuterRef>,
) {
    match pred {
        Predicate::Eq(_col, val)
        | Predicate::Lt(_col, val)
        | Predicate::Le(_col, val)
        | Predicate::Gt(_col, val)
        | Predicate::Ge(_col, val) => {
            if let Some(r) = pv_as_outer_ref(val, inner_scope) {
                out.push(r);
            }
        }
        Predicate::In(_col, vals) | Predicate::NotIn(_col, vals) => {
            for v in vals {
                if let Some(r) = pv_as_outer_ref(v, inner_scope) {
                    out.push(r);
                }
            }
        }
        Predicate::NotBetween(_col, lo, hi) => {
            if let Some(r) = pv_as_outer_ref(lo, inner_scope) {
                out.push(r);
            }
            if let Some(r) = pv_as_outer_ref(hi, inner_scope) {
                out.push(r);
            }
        }
        Predicate::JsonExtractEq { value, .. } | Predicate::JsonContains { value, .. } => {
            if let Some(r) = pv_as_outer_ref(value, inner_scope) {
                out.push(r);
            }
        }
        Predicate::Or(left, right) => {
            for p in left {
                collect_from_predicate(p, inner_scope, out);
            }
            for p in right {
                collect_from_predicate(p, inner_scope, out);
            }
        }
        Predicate::InSubquery { subquery, .. } | Predicate::Exists { subquery, .. } => {
            // Nested subquery: recurse into it, using our inner_scope as
            // the OUTER scope for that further subquery. If it has outer
            // refs that resolve above our inner scope, bump their depth.
            for r in collect_outer_refs_nested(subquery, inner_scope) {
                out.push(r);
            }
        }
        Predicate::Always(_)
        | Predicate::Like(_, _)
        | Predicate::NotLike(_, _)
        | Predicate::ILike(_, _)
        | Predicate::NotILike(_, _)
        | Predicate::IsNull(_)
        | Predicate::IsNotNull(_)
        | Predicate::ScalarCmp { .. } => {
            // No outer refs possible through these shapes today.
        }
    }
}

/// Recursion helper for nested subqueries. Requires the schema via the
/// scope itself (table defs are already reachable).
fn collect_outer_refs_nested(
    subquery: &ParsedSelect,
    outer_scope: &PlannerScope<'_>,
) -> Vec<OuterRef> {
    // We can't easily synthesise inner bindings without the Schema
    // handle. For v0.6.0 we cap scope depth at 1 and treat any outer
    // ref found here as carrying the depth that the caller resolves.
    // When a nested subquery is reached, we simply re-run the walker
    // with whatever scope we have — outer_scope already contains all
    // enclosing tables, so any unresolved column there would have been
    // flagged above.
    let mut out = Vec::new();
    for pred in &subquery.predicates {
        collect_from_predicate(pred, outer_scope, &mut out);
    }
    out
}

/// If a `PredicateValue::ColumnRef` names an outer-scope column,
/// return the corresponding `OuterRef`. Otherwise return None.
fn pv_as_outer_ref(pv: &PredicateValue, inner_scope: &PlannerScope<'_>) -> Option<OuterRef> {
    let PredicateValue::ColumnRef(raw) = pv else {
        return None;
    };
    // Only qualified refs (alias.column) can be outer — bare column
    // refs on the RHS of a predicate are usually JOIN keys; we don't
    // treat them as correlated.
    let (qualifier, col_name) = match raw.split_once('.') {
        Some((q, c)) => (q.to_string(), ColumnName::new(c.to_string())),
        None => return None,
    };
    // Resolve in the inner scope.
    match inner_scope.resolve(Some(&qualifier), &col_name) {
        Some((depth, _)) if depth >= 1 => Some(OuterRef {
            qualifier,
            column: col_name,
            scope_depth: depth,
        }),
        Some(_) => None, // depth 0 → bound in inner scope, not correlated
        None => {
            // Qualifier didn't match any visible scope. Treat this as a
            // correlated reference against depth 1 — the outer row
            // binder will substitute it by alias match at runtime.
            Some(OuterRef {
                qualifier,
                column: col_name,
                scope_depth: 1,
            })
        }
    }
}

/// Return a copy of `subquery` with every outer-reference `ColumnRef`
/// replaced by a literal value drawn from `bindings`.
///
/// `bindings` is keyed by `"qualifier.column"` — the same shape the
/// parser produced. Unmatched outer refs (i.e. no binding) are left
/// as-is; the caller is expected to have supplied a complete set.
pub fn substitute_outer_refs<H: std::hash::BuildHasher>(
    subquery: &ParsedSelect,
    bindings: &std::collections::HashMap<String, Value, H>,
) -> ParsedSelect {
    let mut out = subquery.clone();
    out.predicates = out
        .predicates
        .into_iter()
        .map(|p| substitute_in_predicate(p, bindings))
        .collect();
    out
}

fn substitute_in_predicate<H: std::hash::BuildHasher>(
    pred: Predicate,
    bindings: &std::collections::HashMap<String, Value, H>,
) -> Predicate {
    match pred {
        Predicate::Eq(col, v) => Predicate::Eq(col, substitute_pv(v, bindings)),
        Predicate::Lt(col, v) => Predicate::Lt(col, substitute_pv(v, bindings)),
        Predicate::Le(col, v) => Predicate::Le(col, substitute_pv(v, bindings)),
        Predicate::Gt(col, v) => Predicate::Gt(col, substitute_pv(v, bindings)),
        Predicate::Ge(col, v) => Predicate::Ge(col, substitute_pv(v, bindings)),
        Predicate::In(col, vs) => Predicate::In(
            col,
            vs.into_iter().map(|v| substitute_pv(v, bindings)).collect(),
        ),
        Predicate::NotIn(col, vs) => Predicate::NotIn(
            col,
            vs.into_iter().map(|v| substitute_pv(v, bindings)).collect(),
        ),
        Predicate::NotBetween(col, lo, hi) => {
            Predicate::NotBetween(col, substitute_pv(lo, bindings), substitute_pv(hi, bindings))
        }
        Predicate::JsonExtractEq {
            column,
            path,
            as_text,
            value,
        } => Predicate::JsonExtractEq {
            column,
            path,
            as_text,
            value: substitute_pv(value, bindings),
        },
        Predicate::JsonContains { column, value } => Predicate::JsonContains {
            column,
            value: substitute_pv(value, bindings),
        },
        Predicate::Or(l, r) => Predicate::Or(
            l.into_iter()
                .map(|p| substitute_in_predicate(p, bindings))
                .collect(),
            r.into_iter()
                .map(|p| substitute_in_predicate(p, bindings))
                .collect(),
        ),
        Predicate::InSubquery {
            column,
            subquery,
            negated,
        } => Predicate::InSubquery {
            column,
            subquery: Box::new(substitute_outer_refs(&subquery, bindings)),
            negated,
        },
        Predicate::Exists { subquery, negated } => Predicate::Exists {
            subquery: Box::new(substitute_outer_refs(&subquery, bindings)),
            negated,
        },
        other => other,
    }
}

fn substitute_pv<H: std::hash::BuildHasher>(
    pv: PredicateValue,
    bindings: &std::collections::HashMap<String, Value, H>,
) -> PredicateValue {
    if let PredicateValue::ColumnRef(ref name) = pv {
        if let Some(v) = bindings.get(name) {
            return PredicateValue::Literal(v.clone());
        }
    }
    pv
}

/// Attempt to rewrite a correlated `EXISTS` / `NOT EXISTS` into a
/// semi-join (`IN`) / anti-join (`NOT IN`) against the outer column.
///
/// Returns `Some((outer_column, rewritten_predicate))` on success.
/// The caller pre-executes the rewritten subquery just like the
/// uncorrelated path. Returns `None` when the shape doesn't qualify
/// for decorrelation — caller falls back to the correlated loop.
///
/// Conditions checked (see `docs/reference/sql/correlated-subqueries.md`):
///
/// 1. Exactly one correlated equality `inner.col = outer.col` among
///    the inner predicates; may be in any position.
/// 2. Inner subquery is a simple SELECT: no `GROUP BY`, aggregates,
///    `LIMIT`, `OFFSET`, `ORDER BY`, `DISTINCT`, CTEs, JOINs, or
///    `HAVING`.
/// 3. No other outer references besides the single equijoin pair.
pub fn try_semi_join_rewrite(
    subquery: &ParsedSelect,
    negated: bool,
    outer_refs: &[OuterRef],
) -> Option<(ColumnName, ParsedSelect)> {
    // Condition 2 — inner shape must be trivial enough to hoist.
    if !subquery.group_by.is_empty()
        || !subquery.aggregates.is_empty()
        || subquery.limit.is_some()
        || subquery.offset.is_some()
        || !subquery.order_by.is_empty()
        || subquery.distinct
        || !subquery.ctes.is_empty()
        || !subquery.joins.is_empty()
        || !subquery.having.is_empty()
    {
        return None;
    }

    // Condition 1 & 3 — find exactly one Eq(col, ColumnRef(outer)) or
    // inverse form. We inspect only the top-level predicate list to
    // keep the heuristic safe; nested OR or other shapes fall through
    // to the loop.
    let mut eq_idx: Option<usize> = None;
    let mut inner_col: Option<ColumnName> = None;
    let mut outer_col_ref: Option<String> = None;

    for (i, p) in subquery.predicates.iter().enumerate() {
        if let Predicate::Eq(col, PredicateValue::ColumnRef(raw)) = p {
            // Is this an outer-ref? It must match one of outer_refs.
            if outer_refs.iter().any(|r| &r.as_column_ref() == raw) {
                if eq_idx.is_some() {
                    // More than one correlated equijoin — bail.
                    return None;
                }
                eq_idx = Some(i);
                inner_col = Some(col.clone());
                outer_col_ref = Some(raw.clone());
            }
        }
    }

    // No correlated equijoin at all: we actually shouldn't have reached
    // this function, but be defensive.
    let eq_idx = eq_idx?;
    let inner_col = inner_col?;
    let outer_col_ref = outer_col_ref?;

    // Ensure the one correlation is the only outer reference — if
    // there are others, we can't decorrelate safely.
    if outer_refs
        .iter()
        .filter(|r| r.as_column_ref() == outer_col_ref)
        .count()
        != outer_refs.len()
    {
        return None;
    }

    // Extract the outer column name (strip qualifier).
    let outer_col_name = outer_col_ref.rsplit_once('.').map_or_else(
        || ColumnName::new(outer_col_ref.clone()),
        |(_, c)| ColumnName::new(c.to_string()),
    );

    // Build the rewritten inner subquery: project only `inner_col` and
    // strip the correlated equijoin from the predicate list. The other
    // predicates stay in place.
    let mut rewritten = subquery.clone();
    rewritten.predicates.remove(eq_idx);
    rewritten.columns = Some(vec![inner_col.clone()]);
    rewritten.column_aliases = None;

    // Marker: we turn the EXISTS into the caller's `Predicate::InSubquery`
    // with the outer column, which the regular uncorrelated path then
    // pre-executes. Negation carries through as NOT IN.
    let _ = negated;
    Some((outer_col_name, rewritten))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ColumnDef, DataType, SchemaBuilder, TableDef};
    use kimberlite_store::TableId;

    fn mini_schema() -> (Schema, TableDef, TableDef) {
        let schema = SchemaBuilder::new()
            .table(
                "patient",
                TableId::new(1),
                vec![
                    ColumnDef::new("id", DataType::BigInt).not_null(),
                    ColumnDef::new("name", DataType::Text),
                ],
                vec!["id".into()],
            )
            .table(
                "consent",
                TableId::new(2),
                vec![
                    ColumnDef::new("id", DataType::BigInt).not_null(),
                    ColumnDef::new("subject_id", DataType::BigInt).not_null(),
                    ColumnDef::new("purpose", DataType::Text),
                ],
                vec!["id".into()],
            )
            .build();
        let patient = schema.get_table(&"patient".into()).unwrap().clone();
        let consent = schema.get_table(&"consent".into()).unwrap().clone();
        (schema, patient, consent)
    }

    #[test]
    fn scope_resolve_innermost_first() {
        let (_schema, patient, consent) = mini_schema();
        let outer = PlannerScope::empty().push(vec![("p".into(), &patient)]);
        let inner = outer.push(vec![("c".into(), &consent)]);

        // `c.subject_id` resolves in inner, depth 0
        let res = inner.resolve(Some("c"), &"subject_id".into()).unwrap();
        assert_eq!(res.0, 0);

        // `p.id` resolves in outer, depth 1
        let res = inner.resolve(Some("p"), &"id".into()).unwrap();
        assert_eq!(res.0, 1);

        // `p.nonexistent` doesn't resolve
        assert!(inner.resolve(Some("p"), &"nonexistent".into()).is_none());
    }

    #[test]
    fn outer_ref_round_trip() {
        let r = OuterRef {
            qualifier: "p".into(),
            column: "id".into(),
            scope_depth: 1,
        };
        assert_eq!(r.as_column_ref(), "p.id");
    }
}
