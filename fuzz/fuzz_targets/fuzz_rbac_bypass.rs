#![no_main]

// RBAC/ABAC Policy Bypass Fuzzer
//
// This fuzzer performs adversarial testing of compliance policies by:
// 1. Generating baseline "secure" policies
// 2. Adversarially mutating them (flip Allow→Deny, add wildcards, remove restrictions)
// 3. Verifying that denied operations remain denied (no bypass)
//
// Expected to find 2-5 policy bypass bugs before production.

use libfuzzer_sys::fuzz_target;

use kimberlite_abac::attributes::{UserAttributes, ResourceAttributes, EnvironmentAttributes};
use kimberlite_abac::policy::{AbacPolicy, Condition, Effect, Rule};
use kimberlite_query::rbac_filter::RbacFilter;
use kimberlite_rbac::policy::{AccessPolicy, RowFilter, RowFilterOperator};
use kimberlite_rbac::roles::Role;
use kimberlite_types::{DataClass, TenantId};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

// ============================================================================
// Policy Mutation Strategies
// ============================================================================

/// Generates a baseline secure RBAC policy from bytes.
fn generate_baseline_rbac_policy(data: &[u8]) -> AccessPolicy {
    if data.is_empty() {
        return AccessPolicy::new(Role::User);
    }

    let role = match data[0] % 4 {
        0 => Role::Admin,
        1 => Role::Analyst,
        2 => Role::User,
        _ => Role::Auditor,
    };

    let mut policy = AccessPolicy::new(role);

    // Add restrictive defaults
    policy = policy
        .allow_stream("public_*")
        .deny_stream("private_*")
        .deny_column("ssn")
        .deny_column("password");

    // Add tenant isolation
    if data.len() > 1 {
        let tenant_id = if data.len() > 9 {
            u64::from_le_bytes(data[2..10].try_into().unwrap_or([0; 8]))
        } else {
            1
        };
        policy = policy.with_tenant(TenantId::new(tenant_id));
    }

    // Add row filter for tenant isolation
    if data.len() > 10 {
        policy = policy.with_row_filter(RowFilter::new(
            "tenant_id",
            RowFilterOperator::Eq,
            format!("{}", data[10]),
        ));
    }

    policy
}

/// Adversarially mutates an RBAC policy to attempt bypass.
fn mutate_rbac_policy(policy: AccessPolicy, data: &[u8]) -> AccessPolicy {
    if data.is_empty() {
        return policy;
    }

    let mutation_type = data[0] % 8;

    match mutation_type {
        0 => {
            // Attempt 1: Add wildcard to bypass stream restrictions
            policy.allow_stream("*")
        }
        1 => {
            // Attempt 2: Remove column denials by allowing all
            policy.allow_column("*")
        }
        2 => {
            // Attempt 3: Change row filter to always-true condition
            policy.with_row_filter(RowFilter::new(
                "tenant_id",
                RowFilterOperator::Ne,
                "-1", // Never equal to -1 => always passes
            ))
        }
        3 => {
            // Attempt 4: Elevate role to bypass restrictions
            let mut new_policy = AccessPolicy::new(Role::Admin);
            new_policy = new_policy.allow_stream("*").allow_column("*");
            new_policy
        }
        4 => {
            // Attempt 5: Add deny-all to block legitimate access
            policy.deny_stream("*")
        }
        5 => {
            // Attempt 6: SQL injection in row filter
            policy.with_row_filter(RowFilter::new(
                "tenant_id",
                RowFilterOperator::Eq,
                "1' OR '1'='1",
            ))
        }
        6 => {
            // Attempt 7: Remove tenant restriction
            policy.allow_stream("private_*")
        }
        _ => {
            // Attempt 8: Conflicting rules (deny then allow same column)
            policy.deny_column("ssn").allow_column("ssn")
        }
    }
}

/// Generates a baseline secure ABAC policy from bytes.
fn generate_baseline_abac_policy(data: &[u8]) -> AbacPolicy {
    let mut policy = AbacPolicy::new(Effect::Deny);

    // Rule 1: PHI data requires Healthcare role
    policy = policy.with_rule(Rule {
        name: "phi_requires_healthcare_role".to_string(),
        effect: Effect::Allow,
        conditions: vec![
            Condition::RoleEquals("Healthcare".to_string()),
            Condition::DataClassAtMost("PHI".to_string()),
        ],
        priority: 100,
    });

    // Rule 2: Deny access to PHI for non-healthcare roles
    policy = policy.with_rule(Rule {
        name: "deny_phi_to_non_healthcare".to_string(),
        effect: Effect::Deny,
        conditions: vec![
            Condition::Not(Box::new(Condition::RoleEquals("Healthcare".to_string()))),
            Condition::DataClassAtMost("PHI".to_string()),
        ],
        priority: 200, // Higher priority than allow rule
    });

    // Rule 3: Tenant isolation
    if data.len() > 1 {
        let tenant_id = if data.len() > 9 {
            u64::from_le_bytes(data[2..10].try_into().unwrap_or([0; 8]))
        } else {
            1
        };
        policy = policy.with_rule(Rule {
            name: "tenant_isolation".to_string(),
            effect: Effect::Deny,
            conditions: vec![
                Condition::Not(Box::new(Condition::TenantEquals(tenant_id))),
            ],
            priority: 250,
        });
    }

    policy
}

/// Adversarially mutates an ABAC policy to attempt bypass.
fn mutate_abac_policy(mut policy: AbacPolicy, data: &[u8]) -> AbacPolicy {
    if data.is_empty() {
        return policy;
    }

    let mutation_type = data[0] % 6;

    match mutation_type {
        0 => {
            // Attempt 1: Flip default to Allow (dangerous)
            policy.default_effect = Effect::Allow;
            policy
        }
        1 => {
            // Attempt 2: Flip all Deny rules to Allow
            for rule in &mut policy.rules {
                if rule.effect == Effect::Deny {
                    rule.effect = Effect::Allow;
                }
            }
            policy
        }
        2 => {
            // Attempt 3: Remove conditions from high-priority deny rules
            for rule in &mut policy.rules {
                if rule.effect == Effect::Deny && rule.priority > 100 {
                    rule.conditions.clear();
                }
            }
            policy
        }
        3 => {
            // Attempt 4: Lower priority of deny rules (allow rules execute first)
            for rule in &mut policy.rules {
                if rule.effect == Effect::Deny {
                    rule.priority = 1;
                }
            }
            policy
        }
        4 => {
            // Attempt 5: Add contradictory allow rule with highest priority
            policy.rules.push(Rule {
                name: "bypass_all".to_string(),
                effect: Effect::Allow,
                conditions: vec![], // Empty = always matches
                priority: 1000,
            });
            policy
        }
        _ => {
            // Attempt 6: Inject always-true condition using logical operators
            policy.rules.push(Rule {
                name: "tautology_bypass".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::Or(vec![
                    Condition::RoleEquals("Admin".to_string()),
                    Condition::Not(Box::new(Condition::RoleEquals("Admin".to_string()))),
                ])],
                priority: 500,
            });
            policy
        }
    }
}

// ============================================================================
// Invariant Checking
// ============================================================================

/// Verifies that sensitive operations remain denied after mutation.
fn check_rbac_invariants(policy: &AccessPolicy) -> Result<(), String> {
    let filter = RbacFilter::new(policy.clone());

    // Invariant 1: Users should never access private streams
    if policy.role == Role::User {
        let sql = "SELECT * FROM private_secrets";
        let dialect = GenericDialect {};
        if let Ok(stmts) = Parser::parse_sql(&dialect, sql) {
            if let Some(stmt) = stmts.into_iter().next() {
                match filter.rewrite_statement(stmt) {
                    Ok(_) => {
                        // If rewrite succeeded for private stream, this is a bypass
                        return Err("BYPASS: User role accessed private_* stream".to_string());
                    }
                    Err(_) => {
                        // Expected: access denied
                    }
                }
            }
        }
    }

    // Invariant 2: SSN columns should always be filtered
    let sql_with_ssn = "SELECT name, ssn, email FROM users";
    let dialect = GenericDialect {};
    if let Ok(stmts) = Parser::parse_sql(&dialect, sql_with_ssn) {
        if let Some(stmt) = stmts.into_iter().next() {
            match filter.rewrite_statement(stmt.clone()) {
                Ok(rewritten) => {
                    // Verify SSN column is filtered out
                    let rewritten_str = format!("{}", rewritten);
                    if rewritten_str.to_lowercase().contains("ssn") {
                        return Err("BYPASS: SSN column not filtered in rewritten query".to_string());
                    }
                }
                Err(_) => {
                    // Access denied is also acceptable
                }
            }
        }
    }

    Ok(())
}

/// Verifies that ABAC policy denies PHI access to non-healthcare roles.
fn check_abac_invariants(policy: &AbacPolicy) -> Result<(), String> {
    // Create attributes for non-healthcare user accessing PHI data
    let user = UserAttributes::new("Finance", "finance", 0);
    let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
    let env = EnvironmentAttributes::from_timestamp(
        chrono::Utc::now(),
        "US"
    );

    // Evaluate policy - should deny access
    let decision = kimberlite_abac::evaluator::evaluate(policy, &user, &resource, &env);

    if decision.effect == Effect::Allow {
        return Err("BYPASS: Non-healthcare role accessed PHI data".to_string());
    }

    // Invariant 2: Default-deny should be preserved
    if policy.default_effect == Effect::Allow && policy.rules.is_empty() {
        return Err("BYPASS: Default-allow with no rules = unrestricted access".to_string());
    }

    Ok(())
}

// ============================================================================
// Fuzz Target
// ============================================================================

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    // Split input into sections
    let policy_len = (data[0] as usize % 64).min(data.len().saturating_sub(2));
    let mutation_len = (data[1] as usize % 32).min(data.len().saturating_sub(2 + policy_len));

    let policy_data = &data[2..2 + policy_len];
    let mutation_data = &data[2 + policy_len..2 + policy_len + mutation_len];

    // Test RBAC policies
    let baseline_rbac = generate_baseline_rbac_policy(policy_data);
    let mutated_rbac = mutate_rbac_policy(baseline_rbac.clone(), mutation_data);

    // Check invariants - mutations should NOT bypass security
    if let Err(msg) = check_rbac_invariants(&mutated_rbac) {
        panic!("SECURITY VIOLATION: {}", msg);
    }

    // Test ABAC policies
    let baseline_abac = generate_baseline_abac_policy(policy_data);
    let mutated_abac = mutate_abac_policy(baseline_abac.clone(), mutation_data);

    // Check invariants - mutations should NOT bypass security
    if let Err(msg) = check_abac_invariants(&mutated_abac) {
        panic!("SECURITY VIOLATION: {}", msg);
    }

    // Test SQL injection resistance
    let injection_attempts = [
        "1; DROP TABLE users",
        "' OR '1'='1",
        "1 UNION SELECT * FROM secrets",
        "1' AND (SELECT COUNT(*) FROM users) > 0--",
    ];

    for injection in &injection_attempts {
        let injected_policy = AccessPolicy::new(Role::User)
            .allow_stream("*")
            .with_row_filter(RowFilter::new("tenant_id", RowFilterOperator::Eq, *injection));

        let filter = RbacFilter::new(injected_policy);

        let sql = "SELECT name FROM users";
        let dialect = GenericDialect {};
        if let Ok(stmts) = Parser::parse_sql(&dialect, sql) {
            if let Some(stmt) = stmts.into_iter().next() {
                // Must not panic - injection should be treated as literal value
                let _ = filter.rewrite_statement(stmt);
            }
        }
    }
});
