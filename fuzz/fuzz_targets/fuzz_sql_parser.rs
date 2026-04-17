#![no_main]

use libfuzzer_sys::fuzz_target;
use kimberlite_query::{ParsedStatement, parse_statement};

fuzz_target!(|data: &[u8]| {
    if let Ok(sql) = std::str::from_utf8(data) {
        match parse_statement(sql) {
            Ok(stmt) => validate_ast_structure(&stmt),
            Err(_) => {
                // Parse failure is expected for malformed SQL — the parser
                // must return Err, never panic.
            }
        }
    }
});

/// Validates AST structural invariants for correctness.
///
/// Invariants:
///   - SELECT: column list is non-empty when present (None means SELECT *)
///   - JOIN: ON conditions are non-empty
///   - CTE: name is non-empty and body is a valid SELECT
///   - CREATE TABLE: has at least one column, all names non-empty
///   - INSERT: if columns specified, count matches each row's value count
///   - UPDATE: has at least one assignment
fn validate_ast_structure(stmt: &ParsedStatement) {
    match stmt {
        ParsedStatement::Select(select) => validate_select_ast(select),
        ParsedStatement::Union(union) => {
            validate_select_ast(&union.left);
            validate_select_ast(&union.right);
        }
        ParsedStatement::CreateTable(create) => {
            assert!(
                !create.columns.is_empty(),
                "CREATE TABLE must have at least one column"
            );
            for col in &create.columns {
                assert!(!col.name.is_empty(), "Column name cannot be empty");
            }
        }
        ParsedStatement::Insert(insert) => {
            // `columns` is always present (Vec, not Option). If non-empty,
            // each row's value count must match.
            if !insert.columns.is_empty() {
                for row in &insert.values {
                    assert_eq!(
                        insert.columns.len(),
                        row.len(),
                        "INSERT column count must match each row's value count"
                    );
                }
            }
        }
        ParsedStatement::Update(update) => {
            assert!(
                !update.assignments.is_empty(),
                "UPDATE must have at least one assignment"
            );
        }
        // Other variants have simpler structure; coverage from round-tripping
        // is sufficient.
        ParsedStatement::DropTable(_)
        | ParsedStatement::AlterTable(_)
        | ParsedStatement::CreateIndex(_)
        | ParsedStatement::Delete(_)
        | ParsedStatement::CreateMask(_)
        | ParsedStatement::DropMask(_)
        | ParsedStatement::SetClassification(_)
        | ParsedStatement::ShowClassifications(_)
        | ParsedStatement::ShowTables
        | ParsedStatement::ShowColumns(_)
        | ParsedStatement::CreateRole(_)
        | ParsedStatement::Grant(_)
        | ParsedStatement::CreateUser(_) => {}
    }
}

fn validate_select_ast(select: &kimberlite_query::ParsedSelect) {
    assert!(
        !select.table.is_empty(),
        "SELECT must have non-empty table name"
    );

    if let Some(ref cols) = select.columns {
        assert!(!cols.is_empty(), "SELECT column list cannot be empty");
    }

    for join in &select.joins {
        assert!(
            !join.on_condition.is_empty(),
            "JOIN must have at least one ON condition"
        );
    }

    for cte in &select.ctes {
        assert!(!cte.name.is_empty(), "CTE name cannot be empty");
        validate_select_ast(&cte.query);
    }
}
