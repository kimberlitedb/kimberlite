//! Pre-parse input validation that rejects deeply-nested SQL to prevent
//! DoS attacks that exploit super-linear behavior in the upstream SQL
//! parser on expressions like `(NOT(NOT(NOT(...))))`.
//!
//! Nightly fuzz (`fuzz_rbac_rewrite`) found ~1.5KB inputs that stall
//! `sqlparser-rs` 0.54.0 for ~53 seconds — a cheap post-auth DoS vector.
//! This check rejects such inputs in microseconds before they reach the
//! parser. Injection safety is unaffected; this is purely availability.

use crate::QueryError;

/// Maximum parenthesis/expression nesting depth permitted before rejecting
/// SQL input. Chosen to comfortably accommodate legitimate queries while
/// rejecting pathological inputs that stall the parser.
pub const MAX_SQL_NESTING_DEPTH: usize = 50;

/// Maximum total `NOT` token count permitted in a single SQL input.
/// Deeply nested NOTs trigger worst-case parser behavior even when
/// parenthesis depth is modest.
pub const MAX_SQL_NOT_TOKENS: usize = 100;

/// Returns `Err(QueryError::SqlTooComplex ...)` if the input exceeds the
/// depth or NOT-token budgets, else `Ok(())`.
pub fn check_sql_depth(sql: &str) -> Result<(), QueryError> {
    let mut depth: usize = 0;
    let mut max_depth: usize = 0;
    for &b in sql.as_bytes() {
        match b {
            b'(' => {
                depth += 1;
                if depth > max_depth {
                    max_depth = depth;
                }
                if max_depth > MAX_SQL_NESTING_DEPTH {
                    return Err(QueryError::SqlTooComplex {
                        kind: "paren_depth",
                        value: max_depth,
                        limit: MAX_SQL_NESTING_DEPTH,
                    });
                }
            }
            b')' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    let not_count = count_not_tokens(sql);
    if not_count > MAX_SQL_NOT_TOKENS {
        return Err(QueryError::SqlTooComplex {
            kind: "not_tokens",
            value: not_count,
            limit: MAX_SQL_NOT_TOKENS,
        });
    }
    Ok(())
}

fn count_not_tokens(sql: &str) -> usize {
    let bytes = sql.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        let window = &bytes[i..i + 3];
        let is_not = window.eq_ignore_ascii_case(b"NOT");
        if is_not {
            let left_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let right_ok = i + 3 == bytes.len() || !bytes[i + 3].is_ascii_alphanumeric();
            if left_ok && right_ok {
                count += 1;
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_sql() {
        assert!(check_sql_depth("SELECT id FROM users WHERE tenant_id = 1").is_ok());
    }

    #[test]
    fn accepts_moderate_nesting() {
        let sql = format!(
            "SELECT * FROM t WHERE {}",
            "(".repeat(40) + "x" + &")".repeat(40)
        );
        assert!(check_sql_depth(&sql).is_ok());
    }

    #[test]
    fn accepts_moderate_not_tokens() {
        let sql = "SELECT * FROM t WHERE ".to_string() + &"NOT ".repeat(50) + "x";
        assert!(check_sql_depth(&sql).is_ok());
    }

    #[test]
    fn rejects_pathological_paren_nesting() {
        let sql = "(".repeat(100) + "x" + &")".repeat(100);
        assert!(check_sql_depth(&sql).is_err());
    }

    #[test]
    fn rejects_pathological_not_tokens() {
        let sql = "SELECT * FROM t WHERE ".to_string() + &"NOT ".repeat(150) + "x";
        assert!(check_sql_depth(&sql).is_err());
    }

    #[test]
    fn case_insensitive_not_matching() {
        let sql = "SELECT * FROM t WHERE ".to_string() + &"not ".repeat(150) + "x";
        assert!(check_sql_depth(&sql).is_err());
    }

    #[test]
    fn does_not_count_not_inside_identifier() {
        // `NOTIFY` / `CANNOTATE` etc. should not be counted as NOT tokens.
        let sql = "SELECT NOTIFY, CANNOTATE FROM t".to_string() + &" , NOTIFY".repeat(200);
        assert!(check_sql_depth(&sql).is_ok());
    }

    #[test]
    fn fuzz_regression_nested_not_pattern() {
        // Shape discovered by fuzz_rbac_rewrite on 2026-04-21.
        // Original took 53 seconds; depth check must reject in microseconds.
        let sql = "CALL\nQQ\n".to_string() + &"(NOT\n".repeat(60) + "(?)" + &")".repeat(60);
        let start = std::time::Instant::now();
        let result = check_sql_depth(&sql);
        let elapsed = start.elapsed();
        assert!(result.is_err());
        assert!(
            elapsed.as_millis() < 10,
            "depth check should reject in <10ms, took {elapsed:?}"
        );
    }

    #[test]
    fn error_reports_paren_depth_kind() {
        let sql = "(".repeat(100) + &")".repeat(100);
        let err = check_sql_depth(&sql).unwrap_err();
        match err {
            QueryError::SqlTooComplex { kind, .. } => assert_eq!(kind, "paren_depth"),
            other => panic!("expected SqlTooComplex, got {other:?}"),
        }
    }

    #[test]
    fn error_reports_not_tokens_kind() {
        let sql = "NOT ".repeat(150);
        let err = check_sql_depth(&sql).unwrap_err();
        match err {
            QueryError::SqlTooComplex { kind, .. } => assert_eq!(kind, "not_tokens"),
            other => panic!("expected SqlTooComplex, got {other:?}"),
        }
    }
}
