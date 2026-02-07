//! ABAC policy evaluation engine.
//!
//! Evaluates access requests against a policy by checking rules in priority order.
//! The first matching rule wins. If no rule matches, the policy's default effect applies.

use crate::attributes::{EnvironmentAttributes, ResourceAttributes, UserAttributes};
use crate::policy::{AbacPolicy, Condition, Effect, Rule};
use kimberlite_types::DataClass;

// ============================================================================
// Decision
// ============================================================================

/// The result of evaluating an access request against a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Whether access is allowed or denied.
    pub effect: Effect,
    /// The name of the rule that matched, or `None` if the default was applied.
    pub matched_rule: Option<String>,
    /// Human-readable explanation of why this decision was made.
    pub reason: String,
}

// ============================================================================
// Public API
// ============================================================================

/// Evaluates an access request against a policy.
///
/// Rules are sorted by priority (highest first). The first rule whose
/// conditions all match determines the outcome. If no rule matches,
/// the policy's `default_effect` is returned.
///
/// # Postcondition
///
/// Always returns a `Decision` -- never panics on valid input.
pub fn evaluate(
    policy: &AbacPolicy,
    user: &UserAttributes,
    resource: &ResourceAttributes,
    env: &EnvironmentAttributes,
) -> Decision {
    // Sort rules by priority (highest first)
    let mut rules: Vec<&Rule> = policy.rules.iter().collect();
    rules.sort_by(|a, b| b.priority.cmp(&a.priority));

    for rule in &rules {
        let all_conditions_match = rule
            .conditions
            .iter()
            .all(|cond| evaluate_condition(cond, user, resource, env));

        if all_conditions_match {
            return Decision {
                effect: rule.effect,
                matched_rule: Some(rule.name.clone()),
                reason: format!("Matched rule '{}' (priority {})", rule.name, rule.priority),
            };
        }
    }

    // No rule matched -- apply default
    Decision {
        effect: policy.default_effect,
        matched_rule: None,
        reason: format!(
            "No rule matched; applying default effect: {:?}",
            policy.default_effect
        ),
    }
}

// ============================================================================
// Condition Evaluation
// ============================================================================

/// Recursively evaluates a single condition against the request attributes.
fn evaluate_condition(
    condition: &Condition,
    user: &UserAttributes,
    resource: &ResourceAttributes,
    env: &EnvironmentAttributes,
) -> bool {
    match condition {
        // -- User attribute conditions --
        Condition::RoleEquals(role) => user.role == *role,
        Condition::ClearanceLevelAtLeast(level) => user.clearance_level >= *level,
        Condition::DepartmentEquals(dept) => user.department == *dept,
        Condition::TenantEquals(tid) => user.tenant_id == Some(*tid),

        // -- Resource attribute conditions --
        Condition::DataClassAtMost(max_class) => {
            let resource_level = data_class_level(resource.data_class);
            let max_level = data_class_level_from_name(max_class);
            resource_level <= max_level
        }
        Condition::StreamNameMatches(pattern) => glob_matches(pattern, &resource.stream_name),

        // -- Environment conditions --
        Condition::BusinessHoursOnly => env.is_business_hours,
        Condition::CountryIn(countries) => countries.contains(&env.source_country),
        Condition::CountryNotIn(countries) => !countries.contains(&env.source_country),

        // -- Compliance-specific conditions --
        Condition::RetentionPeriodAtLeast(min_days) => {
            resource.retention_days.is_some_and(|d| d >= *min_days)
        }
        Condition::DataCorrectionAllowed => resource.correction_allowed,
        Condition::IncidentReportingDeadline(_hours) => {
            // Deadline is metadata for policy documentation; evaluation checks
            // that incident reporting infrastructure exists (always true for Kimberlite).
            true
        }
        Condition::FieldLevelRestriction(allowed_fields) => resource
            .requested_fields
            .as_ref()
            .is_none_or(|fields| fields.iter().all(|f| allowed_fields.contains(f))),
        Condition::OperationalSequencing(_steps) => {
            // Sequencing is enforced by the signature binding module;
            // ABAC checks that the policy is declared.
            true
        }
        Condition::LegalHoldActive => resource.legal_hold_active,

        // -- Logical combinators --
        Condition::And(sub) => sub
            .iter()
            .all(|c| evaluate_condition(c, user, resource, env)),
        Condition::Or(sub) => sub
            .iter()
            .any(|c| evaluate_condition(c, user, resource, env)),
        Condition::Not(sub) => !evaluate_condition(sub, user, resource, env),
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Returns a numeric sensitivity level for a `DataClass`.
///
/// Higher values indicate more sensitive data. This ordering is used for
/// `DataClassAtMost` comparisons.
///
/// Levels:
/// - 0: Public
/// - 1: Deidentified
/// - 2: Confidential
/// - 3: Financial
/// - 4: PII
/// - 5: PCI
/// - 6: Sensitive
/// - 7: PHI
fn data_class_level(dc: DataClass) -> u8 {
    match dc {
        DataClass::Public => 0,
        DataClass::Deidentified => 1,
        DataClass::Confidential => 2,
        DataClass::Financial => 3,
        DataClass::PII => 4,
        DataClass::PCI => 5,
        DataClass::Sensitive => 6,
        DataClass::PHI => 7,
    }
}

/// Parses a data class name string into its numeric sensitivity level.
///
/// Unrecognized names map to level 0 (most restrictive in `AtMost` comparisons).
fn data_class_level_from_name(name: &str) -> u8 {
    match name {
        "Deidentified" => 1,
        "Confidential" => 2,
        "Financial" => 3,
        "PII" => 4,
        "PCI" => 5,
        "Sensitive" => 6,
        "PHI" => 7,
        // "Public" and unrecognized names both map to 0 (least sensitive).
        _ => 0,
    }
}

/// Simple glob pattern matching supporting `*` and `?` wildcards.
///
/// - `*` matches zero or more characters
/// - `?` matches exactly one character
///
/// This is intentionally simple. Production systems needing full glob
/// semantics should use a dedicated glob crate.
fn glob_matches(pattern: &str, value: &str) -> bool {
    glob_match_recursive(pattern.as_bytes(), value.as_bytes())
}

/// Recursive glob matcher operating on byte slices.
///
/// Uses bounded recursion proportional to pattern length (max depth = `pattern.len()`).
fn glob_match_recursive(pattern: &[u8], value: &[u8]) -> bool {
    match (pattern.first(), value.first()) {
        // Both exhausted -- match
        (None, None) => true,
        // `*` at end of pattern matches everything
        (Some(b'*'), _) if pattern.len() == 1 => true,
        // `*` matches zero or more characters
        (Some(b'*'), _) => {
            // Try: `*` matches zero characters (skip `*`, keep value)
            // Or:  `*` matches one character (keep `*`, advance value)
            glob_match_recursive(&pattern[1..], value)
                || (!value.is_empty() && glob_match_recursive(pattern, &value[1..]))
        }
        // `?` matches exactly one character
        (Some(b'?'), Some(_)) => glob_match_recursive(&pattern[1..], &value[1..]),
        // Literal character match
        (Some(p), Some(v)) if p == v => glob_match_recursive(&pattern[1..], &value[1..]),
        // All remaining cases: pattern exhausted with value remaining, `?` with
        // no value character, or literal mismatch
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Rule;
    use chrono::TimeZone;
    use chrono::Utc;

    /// Helper: create a user with sensible defaults.
    fn test_user(role: &str, clearance: u8) -> UserAttributes {
        UserAttributes::new(role, "engineering", clearance)
    }

    /// Helper: create a resource with sensible defaults.
    fn test_resource(data_class: DataClass) -> ResourceAttributes {
        ResourceAttributes::new(data_class, 1, "test_stream")
    }

    /// Helper: create an environment during business hours.
    fn business_hours_env() -> EnvironmentAttributes {
        // Wednesday at 10:00 UTC
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
        EnvironmentAttributes::from_timestamp(ts, "US")
    }

    /// Helper: create an environment outside business hours.
    fn after_hours_env() -> EnvironmentAttributes {
        // Wednesday at 22:00 UTC
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 22, 0, 0).unwrap();
        EnvironmentAttributes::from_timestamp(ts, "US")
    }

    #[test]
    fn test_allow_business_hours() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "allow-biz-hours".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::BusinessHoursOnly],
            priority: 10,
        });

        let decision = evaluate(
            &policy,
            &test_user("user", 1),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );

        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(decision.matched_rule.as_deref(), Some("allow-biz-hours"));
    }

    #[test]
    fn test_deny_outside_business_hours() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "allow-biz-hours".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::BusinessHoursOnly],
            priority: 10,
        });

        let decision = evaluate(
            &policy,
            &test_user("user", 1),
            &test_resource(DataClass::Public),
            &after_hours_env(),
        );

        assert_eq!(decision.effect, Effect::Deny);
        assert!(
            decision.matched_rule.is_none(),
            "should fall through to default"
        );
    }

    #[test]
    fn test_deny_non_us_fedramp() {
        let policy = AbacPolicy::fedramp_policy();

        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "DE");

        let decision = evaluate(
            &policy,
            &test_user("analyst", 2),
            &test_resource(DataClass::Confidential),
            &env,
        );

        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("fedramp-us-only"),
            "should match the US-only deny rule"
        );
    }

    #[test]
    fn test_allow_us_fedramp() {
        let policy = AbacPolicy::fedramp_policy();

        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");

        let decision = evaluate(
            &policy,
            &test_user("analyst", 2),
            &test_resource(DataClass::Confidential),
            &env,
        );

        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(decision.matched_rule.as_deref(), Some("fedramp-allow-us"));
    }

    #[test]
    fn test_priority_ordering() {
        // Two rules: low-priority Allow and high-priority Deny.
        // High-priority should win.
        let policy = AbacPolicy::new(Effect::Allow)
            .with_rule(Rule {
                name: "low-allow".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::ClearanceLevelAtLeast(0)],
                priority: 1,
            })
            .with_rule(Rule {
                name: "high-deny".to_string(),
                effect: Effect::Deny,
                conditions: vec![Condition::ClearanceLevelAtLeast(0)],
                priority: 100,
            });

        let decision = evaluate(
            &policy,
            &test_user("user", 1),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );

        assert_eq!(decision.effect, Effect::Deny);
        assert_eq!(decision.matched_rule.as_deref(), Some("high-deny"));
    }

    #[test]
    fn test_default_effect() {
        // Empty policy with Allow default
        let policy = AbacPolicy::new(Effect::Allow);
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );

        assert_eq!(decision.effect, Effect::Allow);
        assert!(decision.matched_rule.is_none());
    }

    #[test]
    fn test_and_condition() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "and-rule".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::And(vec![
                Condition::RoleEquals("admin".to_string()),
                Condition::ClearanceLevelAtLeast(2),
            ])],
            priority: 10,
        });

        // Both conditions met
        let decision = evaluate(
            &policy,
            &test_user("admin", 3),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Only role matches
        let decision = evaluate(
            &policy,
            &test_user("admin", 1),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);

        // Only clearance matches
        let decision = evaluate(
            &policy,
            &test_user("user", 3),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_or_condition() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "or-rule".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::Or(vec![
                Condition::RoleEquals("admin".to_string()),
                Condition::RoleEquals("analyst".to_string()),
            ])],
            priority: 10,
        });

        // First alternative matches
        let decision = evaluate(
            &policy,
            &test_user("admin", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Second alternative matches
        let decision = evaluate(
            &policy,
            &test_user("analyst", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Neither matches
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_not_condition() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "not-admin".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::Not(Box::new(Condition::RoleEquals(
                "admin".to_string(),
            )))],
            priority: 10,
        });

        // Not admin => allowed
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Admin => denied (Not inverts it)
        let decision = evaluate(
            &policy,
            &test_user("admin", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_clearance_level() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "clearance-check".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::ClearanceLevelAtLeast(2)],
            priority: 10,
        });

        // Clearance 3 >= 2 => allowed
        let decision = evaluate(
            &policy,
            &test_user("user", 3),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Clearance 2 >= 2 => allowed
        let decision = evaluate(
            &policy,
            &test_user("user", 2),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Clearance 1 < 2 => denied
        let decision = evaluate(
            &policy,
            &test_user("user", 1),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_data_class_at_most() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "max-confidential".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
            priority: 10,
        });

        // Public (0) <= Confidential (2) => allowed
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Confidential (2) <= Confidential (2) => allowed
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Confidential),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // PII (4) > Confidential (2) => denied
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::PII),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);

        // PHI (7) > Confidential (2) => denied
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::PHI),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_stream_name_glob() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "patient-streams".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::StreamNameMatches("patient_*".to_string())],
            priority: 10,
        });

        // Matches
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &ResourceAttributes::new(DataClass::Public, 1, "patient_records"),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Matches
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &ResourceAttributes::new(DataClass::Public, 1, "patient_"),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Does not match
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &ResourceAttributes::new(DataClass::Public, 1, "metrics"),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_tenant_equals() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "tenant-42".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::TenantEquals(42)],
            priority: 10,
        });

        // Matching tenant
        let decision = evaluate(
            &policy,
            &test_user("user", 0).with_tenant(42),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // Different tenant
        let decision = evaluate(
            &policy,
            &test_user("user", 0).with_tenant(99),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);

        // No tenant
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_glob_matches_exact() {
        assert!(glob_matches("hello", "hello"));
        assert!(!glob_matches("hello", "world"));
    }

    #[test]
    fn test_glob_matches_star() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("foo*", "foobar"));
        assert!(glob_matches("foo*", "foo"));
        assert!(!glob_matches("foo*", "bar"));
        assert!(glob_matches("*bar", "foobar"));
        assert!(glob_matches("f*r", "foobar"));
    }

    #[test]
    fn test_glob_matches_question() {
        assert!(glob_matches("fo?", "foo"));
        assert!(!glob_matches("fo?", "fo"));
        assert!(!glob_matches("fo?", "fooo"));
        assert!(glob_matches("?oo", "foo"));
    }

    #[test]
    fn test_data_class_level_ordering() {
        assert!(data_class_level(DataClass::Public) < data_class_level(DataClass::Confidential));
        assert!(data_class_level(DataClass::Confidential) < data_class_level(DataClass::PII));
        assert!(data_class_level(DataClass::PII) < data_class_level(DataClass::PHI));
        assert!(data_class_level(DataClass::PCI) < data_class_level(DataClass::PHI));
    }

    #[test]
    fn test_hipaa_policy_phi_business_hours() {
        let policy = AbacPolicy::hipaa_policy();

        // PHI + clearance 2 + business hours => allowed
        let decision = evaluate(
            &policy,
            &test_user("doctor", 2),
            &test_resource(DataClass::PHI),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);

        // PHI + clearance 2 + after hours => denied
        let decision = evaluate(
            &policy,
            &test_user("doctor", 2),
            &test_resource(DataClass::PHI),
            &after_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);

        // PHI + clearance 1 + business hours => denied (low clearance)
        let decision = evaluate(
            &policy,
            &test_user("nurse", 1),
            &test_resource(DataClass::PHI),
            &business_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Deny);
    }

    #[test]
    fn test_hipaa_policy_non_phi_data() {
        let policy = AbacPolicy::hipaa_policy();

        // Confidential data + any clearance => allowed by non-PHI rule
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Confidential),
            &after_hours_env(),
        );
        assert_eq!(decision.effect, Effect::Allow);
        assert_eq!(
            decision.matched_rule.as_deref(),
            Some("hipaa-non-phi-access")
        );
    }

    #[test]
    fn test_country_in_condition() {
        let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
            name: "eu-only".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::CountryIn(vec![
                "DE".to_string(),
                "FR".to_string(),
                "NL".to_string(),
            ])],
            priority: 10,
        });

        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();

        // DE => allowed
        let env = EnvironmentAttributes::from_timestamp(ts, "DE");
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &env,
        );
        assert_eq!(decision.effect, Effect::Allow);

        // US => denied
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        let decision = evaluate(
            &policy,
            &test_user("user", 0),
            &test_resource(DataClass::Public),
            &env,
        );
        assert_eq!(decision.effect, Effect::Deny);
    }
}
