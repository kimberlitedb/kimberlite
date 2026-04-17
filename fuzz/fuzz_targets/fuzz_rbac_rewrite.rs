#![no_main]

use libfuzzer_sys::fuzz_target;

use kimberlite_query::rbac_filter::RbacFilter;
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;
use kimberlite_types::TenantId;
use sqlparser::ast::{SetExpr, Statement};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Mirrors the whitelist used by `kimberlite_rbac::enforcement::validate_sql_literal`:
/// integer, boolean, NULL, or a simple single-quoted string with no embedded
/// quotes or backslashes. Anything else must be rejected by the RBAC layer
/// before reaching SQL construction.
fn is_valid_sql_literal(value: &str) -> bool {
    if value.parse::<i64>().is_ok() {
        return true;
    }
    if value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("null")
    {
        return true;
    }
    value.len() >= 2
        && value.starts_with('\'')
        && value.ends_with('\'')
        && !value[1..value.len() - 1].contains('\'')
        && !value[1..value.len() - 1].contains('\\')
}

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
        if data[1] & 0x01 != 0 {
            let tenant_id = if data.len() > 9 {
                u64::from_le_bytes(data[2..10].try_into().unwrap_or([0; 8]))
            } else {
                42
            };
            policy = policy.with_tenant(TenantId::new(tenant_id));
        }

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

        if data.len() > 12 && data[1] & 0x08 != 0 {
            policy = policy.allow_column("*");
        }
        if data.len() > 13 && data[1] & 0x10 != 0 {
            policy = policy.deny_column("ssn");
        }

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
    if data.len() < 20 {
        return;
    }

    let policy_len = (data[0] as usize % 32).min(data.len().saturating_sub(1));
    let policy_data = &data[1..1 + policy_len];
    let sql_data = &data[1 + policy_len..];

    let policy = policy_from_bytes(policy_data);

    // Snapshot row-filter values before moving policy into the filter, so we
    // can assert the injection invariant below.
    let row_filter_values: Vec<String> = policy
        .row_filters()
        .iter()
        .map(|f| f.value.clone())
        .collect();

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

    let filter = RbacFilter::new(policy);

    match filter.rewrite_statement(stmt.clone()) {
        Ok(rewritten) => {
            // Structural: SELECT stays a SELECT.
            if let Statement::Query(ref orig_query) = stmt {
                if let Statement::Query(ref new_query) = rewritten {
                    if let (SetExpr::Select(orig_sel), SetExpr::Select(new_sel)) =
                        (orig_query.body.as_ref(), new_query.body.as_ref())
                    {
                        // Column count must not increase — RBAC only filters.
                        assert!(
                            new_sel.projection.len() <= orig_sel.projection.len(),
                            "RBAC filter must not add columns: {} → {}",
                            orig_sel.projection.len(),
                            new_sel.projection.len()
                        );
                    }
                }
            }

            // Injection invariant: if rewrite succeeded and row filters were
            // present, every filter value must have passed the SQL-literal
            // whitelist. Any escape of that whitelist into a successful rewrite
            // is a critical injection bug.
            for value in &row_filter_values {
                assert!(
                    is_valid_sql_literal(value),
                    "row filter value {value:?} passed rewrite but is not a valid SQL literal \
                     — indicates RBAC bypass of validate_sql_literal whitelist"
                );
            }
        }
        Err(_) => {
            // Errors (AccessDenied, UnsupportedQuery, PolicyEvaluationFailed) are fine.
        }
    }

    // Test 2: explicit injection oracle.
    //
    // Hardcoded injection strings exercise the full rewrite pipeline with a
    // permissive (allow-all) policy but a non-literal row filter value.
    // Invariant: rewrite must return Err for every injection string, because
    // the RBAC layer's `validate_sql_literal` must reject all of them.
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
        "'; DELETE FROM users; --",
        "\\' OR 1=1 --",
    ];

    for sql_str in &injection_sqls {
        for injection in &injection_values {
            // Sanity check on our mirror of the whitelist.
            assert!(
                !is_valid_sql_literal(injection),
                "injection value {injection:?} must be rejected by the literal whitelist"
            );

            let injected_policy = AccessPolicy::new(Role::User)
                .allow_stream("*")
                .allow_column("*")
                .with_row_filter(RowFilter::new("tenant_id", RowFilterOperator::Eq, *injection));

            let filter = RbacFilter::new(injected_policy);

            let dialect = GenericDialect {};
            if let Ok(stmts) = Parser::parse_sql(&dialect, sql_str) {
                if let Some(stmt) = stmts.into_iter().next() {
                    let result = filter.rewrite_statement(stmt);
                    assert!(
                        result.is_err(),
                        "injection {injection:?} against {sql_str:?} produced a successful \
                         rewrite — RBAC literal validation was bypassed"
                    );
                }
            }
        }
    }
});
