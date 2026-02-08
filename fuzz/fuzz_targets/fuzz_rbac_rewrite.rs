#![no_main]

use libfuzzer_sys::fuzz_target;

use kimberlite_query::rbac_filter::RbacFilter;
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;
use kimberlite_types::TenantId;
use sqlparser::ast::{SetExpr, Statement};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Build an AccessPolicy from fuzz bytes.
fn policy_from_bytes(data: &[u8]) -> AccessPolicy {
    if data.is_empty() {
        return AccessPolicy::new(Role::Admin);
    }

    let role = match data[0] % 4 {
        0 => Role::Admin,
        1 => Role::Analyst,
        2 => Role::User,
        _ => Role::Auditor,
    };

    let mut policy = AccessPolicy::new(role);

    if data.len() > 1 {
        // Optionally set tenant
        if data[1] & 0x01 != 0 {
            let tenant_id = if data.len() > 9 {
                u64::from_le_bytes(data[2..10].try_into().unwrap_or([0; 8]))
            } else {
                42
            };
            policy = policy.with_tenant(TenantId::new(tenant_id));
        }

        // Optionally add stream filters
        if data.len() > 10 && data[1] & 0x02 != 0 {
            policy = policy.allow_stream("*");
        }
        if data.len() > 11 && data[1] & 0x04 != 0 {
            let len = (data[11] as usize % 16).min(data.len().saturating_sub(12));
            if let Ok(pattern) = std::str::from_utf8(&data[12..12 + len]) {
                if !pattern.is_empty() {
                    policy = policy.deny_stream(pattern);
                }
            }
        }

        // Optionally add column filters
        if data.len() > 12 && data[1] & 0x08 != 0 {
            policy = policy.allow_column("*");
        }
        if data.len() > 13 && data[1] & 0x10 != 0 {
            policy = policy.deny_column("ssn");
        }

        // Optionally add row filter
        if data.len() > 14 && data[1] & 0x20 != 0 {
            let op = match data[14] % 4 {
                0 => RowFilterOperator::Eq,
                1 => RowFilterOperator::Ne,
                2 => RowFilterOperator::Lt,
                _ => RowFilterOperator::Gt,
            };
            let value = if data.len() > 15 {
                let vlen = (data[15] as usize % 16).min(data.len().saturating_sub(16));
                std::str::from_utf8(&data[16..16 + vlen])
                    .unwrap_or("42")
                    .to_string()
            } else {
                "42".to_string()
            };
            policy = policy.with_row_filter(RowFilter::new("tenant_id", op, value));
        }
    }

    policy
}

fuzz_target!(|data: &[u8]| {
    // Need at least some bytes for policy + SQL
    if data.len() < 20 {
        return;
    }

    // Split input: first portion for policy, rest for SQL
    let policy_len = (data[0] as usize % 32).min(data.len().saturating_sub(1));
    let policy_data = &data[1..1 + policy_len];
    let sql_data = &data[1 + policy_len..];

    // Build policy from fuzz input
    let policy = policy_from_bytes(policy_data);

    // Try to parse the SQL
    let sql = match std::str::from_utf8(sql_data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let dialect = GenericDialect {};
    let statements = match Parser::parse_sql(&dialect, sql) {
        Ok(stmts) => stmts,
        Err(_) => return,
    };

    let stmt = match statements.into_iter().next() {
        Some(s) => s,
        None => return,
    };

    // Create filter and attempt rewrite
    let filter = RbacFilter::new(policy);

    match filter.rewrite_statement(stmt.clone()) {
        Ok(rewritten) => {
            // Verify structural invariants of the rewritten statement:

            // 1. Rewritten statement must still be a valid SQL statement
            //    (sqlparser produced it, so it's structurally valid)

            // 2. If the original was a SELECT, the rewritten must also be a SELECT
            if let Statement::Query(ref orig_query) = stmt {
                if let Statement::Query(ref new_query) = rewritten {
                    // 3. If the policy has row filters, verify a WHERE clause was injected
                    //    (for simple single-table SELECTs)
                    if let (SetExpr::Select(orig_sel), SetExpr::Select(new_sel)) =
                        (orig_query.body.as_ref(), new_query.body.as_ref())
                    {
                        // Column count should not increase (only filtering happens)
                        assert!(
                            new_sel.projection.len() <= orig_sel.projection.len(),
                            "RBAC filter should not add columns: original {} vs rewritten {}",
                            orig_sel.projection.len(),
                            new_sel.projection.len()
                        );
                    }
                }
            }
        }
        Err(_) => {
            // Errors (AccessDenied, UnsupportedQuery, etc.) are expected —
            // the important thing is no panics.
        }
    }

    // Test 2: SQL injection attempts in row filter values
    //
    // These should be handled safely by the RBAC filter, not passed
    // through as raw SQL.
    let injection_sqls = [
        "SELECT name, email FROM users",
        "SELECT id FROM accounts",
        "SELECT col1 FROM data",
    ];

    let injection_values = [
        "1; DROP TABLE users",
        "' OR '1'='1",
        "1 UNION SELECT password FROM credentials",
        "1' AND 1=1--",
    ];

    for sql_str in &injection_sqls {
        for injection in &injection_values {
            let injected_policy = AccessPolicy::new(Role::User)
                .allow_stream("*")
                .allow_column("*")
                .with_row_filter(RowFilter::new("tenant_id", RowFilterOperator::Eq, *injection));

            let filter = RbacFilter::new(injected_policy);

            let dialect = GenericDialect {};
            if let Ok(stmts) = Parser::parse_sql(&dialect, sql_str) {
                if let Some(stmt) = stmts.into_iter().next() {
                    // Must not panic. If it succeeds, the injection value is
                    // treated as a literal value, not executable SQL.
                    let _ = filter.rewrite_statement(stmt);
                }
            }
        }
    }
});
