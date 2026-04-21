#![no_main]

// Free-form RBAC/ABAC policy fuzzer.
//
// Replaces the previous enumerated-mutation design (8 hardcoded RBAC mutations,
// 6 ABAC mutations) with open-ended policy generation so libFuzzer can explore
// the full policy space. Invariants are checked at the AST level rather than
// by string-matching the rendered SQL.
//
// Core invariants:
//   - Columns explicitly denied by the policy must never appear in any
//     projected identifier of the rewritten statement.
//   - Row filter values that escape the `validate_sql_literal` whitelist must
//     never produce a successful rewrite (RBAC literal-validation bypass).
//   - ABAC: a non-healthcare role evaluated against a PHI resource must be
//     denied whenever the policy retains a priority≥200 deny rule for that
//     condition (the "secure baseline" rule).

use std::collections::HashSet;

use libfuzzer_sys::fuzz_target;

use kimberlite_abac::attributes::{EnvironmentAttributes, ResourceAttributes, UserAttributes};
use kimberlite_abac::policy::{AbacPolicy, Condition, Effect, Rule};
use kimberlite_query::rbac_filter::RbacFilter;
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;
use kimberlite_types::{DataClass, TenantId};
use sqlparser::ast::{Expr, SelectItem, SetExpr, Statement};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Mirrors `kimberlite_rbac::enforcement::validate_sql_literal` — any value
/// outside this whitelist must be rejected by `generate_where_clause` before
/// the RBAC filter can return a successful rewrite.
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

/// Build a free-form policy. Every knob on `AccessPolicy` is driven by fuzz
/// bytes so libFuzzer can explore novel combinations instead of picking from
/// a fixed menu of mutations.
fn generate_rbac_policy(data: &[u8]) -> (AccessPolicy, Vec<String>) {
    if data.is_empty() {
        return (AccessPolicy::new(Role::User), Vec::new());
    }

    let role = match data[0] % 4 {
        0 => Role::Admin,
        1 => Role::Analyst,
        2 => Role::User,
        _ => Role::Auditor,
    };
    let mut policy = AccessPolicy::new(role);
    let mut denied_columns: Vec<String> = Vec::new();

    let mut i = 1;
    while i < data.len() {
        let op = data[i] & 0x0F;
        i += 1;

        match op {
            0 => {
                // with_tenant
                if data.len() < i + 8 {
                    break;
                }
                let tid = u64::from_le_bytes(data[i..i + 8].try_into().expect("8 bytes"));
                policy = policy.with_tenant(TenantId::new(tid));
                i += 8;
            }
            1 | 2 | 3 | 4 => {
                // stream / column allow / deny with a fuzz-derived short pattern.
                if data.len() < i + 1 {
                    break;
                }
                let plen = (data[i] as usize % 12).min(data.len().saturating_sub(i + 1));
                let start = i + 1;
                let end = start + plen;
                let pat_bytes = &data[start..end];
                i = end;
                let pattern = match std::str::from_utf8(pat_bytes) {
                    Ok(s) if !s.is_empty() => s.to_string(),
                    _ => continue,
                };
                policy = match op {
                    1 => policy.allow_stream(pattern),
                    2 => policy.deny_stream(pattern),
                    3 => policy.allow_column(pattern),
                    _ => {
                        denied_columns.push(pattern.clone());
                        policy.deny_column(pattern)
                    }
                };
            }
            5 => {
                // with_row_filter — value drawn from fuzz bytes. Most values
                // will fail the literal whitelist and must cause rewrite to
                // return Err (checked in the fuzz target).
                if data.len() < i + 3 {
                    break;
                }
                let op_byte = data[i];
                let vlen = (data[i + 1] as usize % 20).min(data.len().saturating_sub(i + 2));
                let start = i + 2;
                let end = start + vlen;
                let value = std::str::from_utf8(&data[start..end])
                    .unwrap_or("42")
                    .to_string();
                let operator = match op_byte % 7 {
                    0 => RowFilterOperator::Eq,
                    1 => RowFilterOperator::Ne,
                    2 => RowFilterOperator::Lt,
                    3 => RowFilterOperator::Le,
                    4 => RowFilterOperator::Gt,
                    5 => RowFilterOperator::Ge,
                    _ => RowFilterOperator::Eq,
                };
                policy = policy.with_row_filter(RowFilter::new("tenant_id", operator, value));
                i = end;
            }
            _ => {
                // No-op for remaining values; leaves bytes for subsequent ops.
            }
        }
    }

    (policy, denied_columns)
}

/// Build a SQL string from fuzz bytes. Uses a small fixed schema so most
/// generated queries are parseable, giving libFuzzer a productive surface
/// instead of spending cycles on UTF-8 / parser rejection.
fn generate_query(data: &[u8]) -> String {
    if data.is_empty() {
        return "SELECT name FROM users".to_string();
    }
    let table = match data[0] % 4 {
        0 => "users",
        1 => "accounts",
        2 => "private_secrets",
        _ => "public_events",
    };
    let columns = match data.get(1).copied().unwrap_or(0) % 6 {
        0 => "name",
        1 => "name, email",
        2 => "name, ssn, email",
        3 => "id, password",
        4 => "tenant_id, name",
        _ => "ssn",
    };
    format!("SELECT {columns} FROM {table}")
}

/// Collect every column identifier that appears as a projected item in a
/// rewritten SELECT statement. Walks the AST rather than grep'ing rendered
/// SQL, so table names / aliases containing substrings of column names are
/// not confused with actual column projections.
fn projected_identifiers(stmt: &Statement) -> HashSet<String> {
    let mut out = HashSet::new();
    let Statement::Query(query) = stmt else {
        return out;
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return out;
    };
    for item in &select.projection {
        match item {
            SelectItem::UnnamedExpr(Expr::Identifier(ident))
            | SelectItem::ExprWithAlias {
                expr: Expr::Identifier(ident),
                ..
            } => {
                out.insert(ident.value.to_lowercase());
            }
            SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts))
            | SelectItem::ExprWithAlias {
                expr: Expr::CompoundIdentifier(parts),
                ..
            } => {
                if let Some(last) = parts.last() {
                    out.insert(last.value.to_lowercase());
                }
            }
            _ => {}
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    let policy_len = (data[0] as usize % 64).min(data.len().saturating_sub(2));
    let query_len = data.len().saturating_sub(2 + policy_len);

    let policy_data = &data[2..2 + policy_len];
    let query_data = &data[2 + policy_len..2 + policy_len + query_len];

    // ── RBAC: free-form policy, free-form query ─────────────────────────────
    let (policy, denied_columns) = generate_rbac_policy(policy_data);
    let row_filter_values: Vec<String> = policy
        .row_filters()
        .iter()
        .map(|f| f.value.clone())
        .collect();

    let sql = generate_query(query_data);
    let dialect = GenericDialect {};
    if let Ok(stmts) = Parser::parse_sql(&dialect, &sql) {
        if let Some(stmt) = stmts.into_iter().next() {
            let filter = RbacFilter::new(policy);
            if let Ok(rewritten) = filter.rewrite_statement(stmt) {
                // Invariant: denied columns must not appear in the projection.
                let projected = projected_identifiers(&rewritten.statement);
                for denied in &denied_columns {
                    let key = denied.to_lowercase();
                    assert!(
                        !projected.contains(&key),
                        "RBAC BYPASS: denied column {denied:?} appears in rewritten projection \
                         {projected:?}"
                    );
                }

                // Invariant: every row filter value in the policy that reached
                // a successful rewrite must be a valid SQL literal (the layer
                // below rejects anything else).
                for value in &row_filter_values {
                    assert!(
                        is_valid_sql_literal(value),
                        "RBAC BYPASS: row filter value {value:?} escaped validate_sql_literal \
                         whitelist and produced a successful rewrite"
                    );
                }
            }
        }
    }

    // ── ABAC: baseline secure policy, evaluate non-healthcare → PHI ─────────
    let baseline_abac = baseline_abac_policy(policy_data);
    let user = UserAttributes::new("Finance", "finance", 0);
    let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
    let env = EnvironmentAttributes::from_timestamp(chrono::Utc::now(), "US");
    let decision = kimberlite_abac::evaluator::evaluate(&baseline_abac, &user, &resource, &env);
    assert_eq!(
        decision.effect,
        Effect::Deny,
        "ABAC BYPASS: non-healthcare role gained access to PHI with baseline secure policy"
    );

    // Guardrail: default-Allow with zero rules is always a bypass.
    if baseline_abac.default_effect == Effect::Allow && baseline_abac.rules.is_empty() {
        panic!("ABAC BYPASS: default-Allow with no rules = unrestricted access");
    }
});

fn baseline_abac_policy(data: &[u8]) -> AbacPolicy {
    let mut policy = AbacPolicy::new(Effect::Deny);
    policy = policy
        .with_rule(Rule {
            name: "phi_requires_healthcare_role".to_string(),
            effect: Effect::Allow,
            conditions: vec![
                Condition::RoleEquals("Healthcare".to_string()),
                Condition::DataClassAtMost("PHI".to_string()),
            ],
            priority: 100,
        })
        .expect("baseline ABAC rule names are unique");
    policy = policy
        .with_rule(Rule {
            name: "deny_phi_to_non_healthcare".to_string(),
            effect: Effect::Deny,
            conditions: vec![
                Condition::Not(Box::new(Condition::RoleEquals("Healthcare".to_string()))),
                Condition::DataClassAtMost("PHI".to_string()),
            ],
            priority: 200,
        })
        .expect("baseline ABAC rule names are unique");
    if data.len() > 9 {
        let tid = u64::from_le_bytes(data[2..10].try_into().unwrap_or([0; 8]));
        policy = policy
            .with_rule(Rule {
                name: "tenant_isolation".to_string(),
                effect: Effect::Deny,
                conditions: vec![Condition::Not(Box::new(Condition::TenantEquals(tid)))],
                priority: 250,
            })
            .expect("baseline ABAC rule names are unique");
    }
    policy
}
