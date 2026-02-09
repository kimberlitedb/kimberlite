#![no_main]

use libfuzzer_sys::fuzz_target;
use kimberlite_query::{ParsedStatement, parse_statement};

fuzz_target!(|data: &[u8]| {
    // Convert bytes to UTF-8 string (ignore invalid UTF-8)
    if let Ok(sql) = std::str::from_utf8(data) {
        // Try to parse the SQL - should never panic, only return Err
        match parse_statement(sql) {
            Ok(stmt) => {
                // AUDIT-2026-03 M-1: Validate AST structure
                validate_ast_structure(&stmt);
            }
            Err(_) => {
                // Parse failure is expected for malformed SQL
                // The parser should return Err, not panic
            }
        }
    }
});

/// Validates the AST structure for correctness invariants.
///
/// **Invariants checked:**
/// 1. SELECT: column list is non-empty when not SELECT *
/// 2. JOIN: ON conditions exist for each join
/// 3. INSERT: column count matches value count
/// 4. UPDATE: at least one SET clause
/// 5. CREATE TABLE: at least one column defined
/// 6. CTE: query is valid SELECT
///
/// **Security Context:** AUDIT-2026-03 M-1, CWE-707 (Improper Neutralization)
fn validate_ast_structure(stmt: &ParsedStatement) {
    match stmt {
        ParsedStatement::Select(select) => {
            validate_select_ast(select);
        }
        ParsedStatement::Union(union) => {
            validate_select_ast(&union.left);
            validate_select_ast(&union.right);
        }
        ParsedStatement::CreateTable(create) => {
            // Invariant: Must have at least one column
            assert!(
                !create.columns.is_empty(),
                "CREATE TABLE must have at least one column"
            );

            // Invariant: Column names are non-empty
            for col in &create.columns {
                assert!(!col.name.is_empty(), "Column name cannot be empty");
            }
        }
        ParsedStatement::Insert(insert) => {
            // Invariant: If columns specified, must match value count
            if let Some(ref cols) = insert.columns {
                assert_eq!(
                    cols.len(),
                    insert.values.len(),
                    "INSERT column count must match value count"
                );
            }
        }
        ParsedStatement::Update(update) => {
            // Invariant: Must have at least one SET clause
            assert!(!update.set_clauses.is_empty(), "UPDATE must have at least one SET clause");
        }
        ParsedStatement::DropTable(_)
        | ParsedStatement::AlterTable(_)
        | ParsedStatement::CreateIndex(_)
        | ParsedStatement::Delete(_) => {
            // These have simpler structures, validation is minimal
        }
    }
}

/// Validates SELECT statement structure.
fn validate_select_ast(select: &kimberlite_query::ParsedSelect) {
    // Invariant: Table name is non-empty
    assert!(!select.table.is_empty(), "SELECT must have non-empty table name");

    // Invariant: If columns specified (not SELECT *), list is non-empty
    if let Some(ref cols) = select.columns {
        assert!(!cols.is_empty(), "SELECT column list cannot be empty");
    }

    // Invariant: Each JOIN has ON conditions
    for join in &select.joins {
        assert!(
            !join.on_condition.is_empty(),
            "JOIN must have at least one ON condition"
        );
    }

    // Invariant: CTEs have valid SELECT queries
    for cte in &select.ctes {
        assert!(!cte.name.is_empty(), "CTE name cannot be empty");
        validate_select_ast(&cte.query);
    }

    // Invariant: GROUP BY columns are non-empty if specified
    if let Some(ref group_by) = select.group_by {
        assert!(!group_by.is_empty(), "GROUP BY column list cannot be empty");
    }

    // Invariant: ORDER BY columns are non-empty if specified
    if let Some(ref order_by) = select.order_by {
        assert!(!order_by.is_empty(), "ORDER BY column list cannot be empty");
    }
}
