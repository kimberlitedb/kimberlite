//! EXPLAIN renderer for query plans.
//!
//! AUDIT-2026-04 S3.3 — compliance-vertical developers need to
//! see the access path a query will take before running it in
//! production. Before this addition, the only way to inspect a
//! plan was to add a `tracing::debug!` in the executor — not
//! usable from an SDK client or an ops CLI.
//!
//! # Output shape
//!
//! A plan is rendered as an indented tree, one node per line.
//! Every line starts with `"-> "` followed by the node kind
//! and its most salient attributes. Example:
//!
//! ```text
//! -> Aggregate [group=[dept_id], aggs=[COUNT(*), SUM(salary)]]
//!   -> TableScan [employees, filter, limit=100]
//! ```
//!
//! **Masked column names are rendered as-is** — the *strategy*
//! is what should be sensitive at this layer, and masks are
//! applied post-projection. A later enhancement can render
//! `ssn [REDACT]` once the plan layer learns the registry.

use crate::plan::QueryPlan;

/// Render `plan` as a multi-line EXPLAIN tree.
///
/// The output is deterministic — the same plan always produces
/// the same bytes. This enables golden-file regression tests
/// (`tests/explain_goldens/`) that catch silent plan changes.
pub fn explain_plan(plan: &QueryPlan) -> String {
    let mut out = String::new();
    write_node(&mut out, plan, 0);
    out
}

fn write_node(out: &mut String, plan: &QueryPlan, depth: usize) {
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push_str("-> ");
    match plan {
        QueryPlan::PointLookup {
            metadata,
            column_names,
            ..
        } => {
            out.push_str(&format!(
                "PointLookup [{}, cols={}]\n",
                metadata.table_name,
                column_names.len()
            ));
        }
        QueryPlan::RangeScan {
            metadata,
            filter,
            limit,
            order,
            column_names,
            order_by,
            ..
        } => {
            let attrs = vec![
                metadata.table_name.clone(),
                format!("cols={}", column_names.len()),
                format!("order={order:?}"),
                if filter.is_some() {
                    "filter=yes".into()
                } else {
                    "filter=no".into()
                },
                match limit {
                    Some(n) => format!("limit={n}"),
                    None => "limit=none".into(),
                },
                match order_by {
                    Some(_) => "sort=client".into(),
                    None => "sort=none".into(),
                },
            ];
            out.push_str(&format!("RangeScan [{}]\n", attrs.join(", ")));
        }
        QueryPlan::IndexScan {
            metadata,
            index_name,
            filter,
            limit,
            column_names,
            ..
        } => {
            out.push_str(&format!(
                "IndexScan [{}, index={}, cols={}, filter={}, limit={}]\n",
                metadata.table_name,
                index_name,
                column_names.len(),
                if filter.is_some() { "yes" } else { "no" },
                limit.map_or("none".into(), |n| n.to_string()),
            ));
        }
        QueryPlan::TableScan {
            metadata,
            filter,
            limit,
            order,
            column_names,
            ..
        } => {
            out.push_str(&format!(
                "TableScan [{}, cols={}, filter={}, limit={}, sort={}]\n",
                metadata.table_name,
                column_names.len(),
                if filter.is_some() { "yes" } else { "no" },
                limit.map_or("none".into(), |n| n.to_string()),
                if order.is_some() { "yes" } else { "no" },
            ));
        }
        QueryPlan::Aggregate {
            source,
            group_by_names,
            aggregates,
            having,
            ..
        } => {
            out.push_str(&format!(
                "Aggregate [group=[{}], aggs={}, having={}]\n",
                group_by_names
                    .iter()
                    .map(|c| c.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                aggregates.len(),
                having.len(),
            ));
            write_node(out, source, depth + 1);
        }
        QueryPlan::Join {
            join_type,
            left,
            right,
            on_conditions,
            column_names,
            ..
        } => {
            out.push_str(&format!(
                "Join [type={join_type:?}, on={}, cols={}]\n",
                on_conditions.len(),
                column_names.len(),
            ));
            write_node(out, left, depth + 1);
            write_node(out, right, depth + 1);
        }
        QueryPlan::Materialize {
            source,
            filter,
            case_columns,
            order,
            limit,
            ..
        } => {
            out.push_str(&format!(
                "Materialize [filter={}, case_cols={}, sort={}, limit={}]\n",
                if filter.is_some() { "yes" } else { "no" },
                case_columns.len(),
                if order.is_some() { "yes" } else { "no" },
                limit.map_or("none".into(), |n| n.to_string()),
            ));
            write_node(out, source, depth + 1);
        }
    }
}

/// Extract `EXPLAIN` prefix from a SQL string.
///
/// Kimberlite recognises `EXPLAIN <select>` (and the future
/// `EXPLAIN ANALYZE <select>` — separate follow-up; for now only
/// the plain form is handled).
///
/// Returns `(cleaned_sql, true)` if the prefix was present;
/// `(original, false)` otherwise.
pub fn extract_explain(sql: &str) -> (&str, bool) {
    let trimmed = sql.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    // `EXPLAIN` must be followed by whitespace or end-of-string
    // to avoid eating the prefix of `EXPLAIN_FOO` (not currently
    // valid SQL, but cheap defence).
    const KW: &str = "EXPLAIN";
    if upper.starts_with(KW) {
        let after = &trimmed[KW.len()..];
        if after.starts_with(|c: char| c.is_whitespace()) {
            return (after.trim_start(), true);
        }
    }
    (sql, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_explain_prefix() {
        assert_eq!(
            extract_explain("EXPLAIN SELECT * FROM t"),
            ("SELECT * FROM t", true),
        );
    }

    #[test]
    fn extract_explain_case_insensitive() {
        assert_eq!(
            extract_explain("explain SELECT 1"),
            ("SELECT 1", true),
        );
        assert_eq!(
            extract_explain("ExPlAiN SELECT 1"),
            ("SELECT 1", true),
        );
    }

    #[test]
    fn extract_explain_leading_whitespace() {
        assert_eq!(
            extract_explain("  \tEXPLAIN SELECT 1"),
            ("SELECT 1", true),
        );
    }

    #[test]
    fn extract_explain_ignores_prefix_inside_sql() {
        // "EXPLAINER" is not the EXPLAIN keyword.
        let sql = "SELECT * FROM explainer";
        assert_eq!(extract_explain(sql), (sql, false));
    }

    #[test]
    fn extract_explain_word_boundary_required() {
        let sql = "EXPLAIN_TABLE";
        assert_eq!(extract_explain(sql), (sql, false));
    }

    #[test]
    fn extract_explain_no_prefix() {
        assert_eq!(
            extract_explain("SELECT 1"),
            ("SELECT 1", false),
        );
    }
}
