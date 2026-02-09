#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use kimberlite_abac::{
    AbacPolicy, Condition, Decision, Effect, EnvironmentAttributes, ResourceAttributes, Rule,
    UserAttributes, evaluate,
};
use kimberlite_types::DataClass;
use chrono::Utc;

// ============================================================================
// Arbitrary Implementations (AUDIT-2026-03 M-2)
// ============================================================================

/// Fuzzer-friendly Effect with Arbitrary derivation.
#[derive(Debug, Clone, Copy, Arbitrary)]
enum FuzzEffect {
    Allow,
    Deny,
}

impl From<FuzzEffect> for Effect {
    fn from(f: FuzzEffect) -> Self {
        match f {
            FuzzEffect::Allow => Effect::Allow,
            FuzzEffect::Deny => Effect::Deny,
        }
    }
}

/// Fuzzer-friendly Condition with Arbitrary derivation.
///
/// Simplified for fuzzing - generates valid but varied conditions.
#[derive(Debug, Clone, Arbitrary)]
enum FuzzCondition {
    RoleEquals(String),
    ClearanceLevelAtLeast(u8),
    DepartmentEquals(String),
    TenantEquals(u64),
    DataClassAtMost(String),
    StreamNameMatches(String),
    BusinessHoursOnly,
    CountryIn(Vec<String>),
    CountryNotIn(Vec<String>),
    RetentionPeriodAtLeast(u32),
    DataCorrectionAllowed,
    IncidentReportingDeadline(u32),
    FieldLevelRestriction(Vec<String>),
    OperationalSequencing(Vec<String>),
    LegalHoldActive,
    // Logical combinators (simplified to avoid infinite recursion)
    And2(Box<FuzzCondition>, Box<FuzzCondition>),
    Or2(Box<FuzzCondition>, Box<FuzzCondition>),
    Not(Box<FuzzCondition>),
}

impl FuzzCondition {
    /// Converts fuzzer condition to real ABAC condition.
    ///
    /// **Depth limiting:** Logical combinators are limited to depth 3 to prevent stack overflow.
    fn to_abac_condition(&self, depth: u8) -> Condition {
        if depth > 3 {
            // Depth limit reached - return simple condition
            return Condition::BusinessHoursOnly;
        }

        match self {
            Self::RoleEquals(r) => Condition::RoleEquals(r.clone()),
            Self::ClearanceLevelAtLeast(l) => Condition::ClearanceLevelAtLeast(*l),
            Self::DepartmentEquals(d) => Condition::DepartmentEquals(d.clone()),
            Self::TenantEquals(t) => Condition::TenantEquals(*t),
            Self::DataClassAtMost(dc) => Condition::DataClassAtMost(dc.clone()),
            Self::StreamNameMatches(p) => Condition::StreamNameMatches(p.clone()),
            Self::BusinessHoursOnly => Condition::BusinessHoursOnly,
            Self::CountryIn(c) => Condition::CountryIn(c.clone()),
            Self::CountryNotIn(c) => Condition::CountryNotIn(c.clone()),
            Self::RetentionPeriodAtLeast(d) => Condition::RetentionPeriodAtLeast(*d),
            Self::DataCorrectionAllowed => Condition::DataCorrectionAllowed,
            Self::IncidentReportingDeadline(h) => Condition::IncidentReportingDeadline(*h),
            Self::FieldLevelRestriction(f) => Condition::FieldLevelRestriction(f.clone()),
            Self::OperationalSequencing(ops) => Condition::OperationalSequencing(ops.clone()),
            Self::LegalHoldActive => Condition::LegalHoldActive,
            Self::And2(a, b) => Condition::And(vec![
                a.to_abac_condition(depth + 1),
                b.to_abac_condition(depth + 1),
            ]),
            Self::Or2(a, b) => Condition::Or(vec![
                a.to_abac_condition(depth + 1),
                b.to_abac_condition(depth + 1),
            ]),
            Self::Not(c) => Condition::Not(Box::new(c.to_abac_condition(depth + 1))),
        }
    }
}

/// Fuzzer-friendly Rule.
#[derive(Debug, Clone, Arbitrary)]
struct FuzzRule {
    name: String,
    effect: FuzzEffect,
    conditions: Vec<FuzzCondition>,
    priority: u32,
}

impl FuzzRule {
    fn to_abac_rule(&self) -> Rule {
        Rule {
            name: self.name.clone(),
            effect: self.effect.into(),
            conditions: self.conditions.iter().map(|c| c.to_abac_condition(0)).collect(),
            priority: self.priority,
        }
    }
}

/// Fuzzer-friendly AbacPolicy.
#[derive(Debug, Clone, Arbitrary)]
struct FuzzPolicy {
    rules: Vec<FuzzRule>,
    default_effect: FuzzEffect,
}

impl FuzzPolicy {
    fn to_abac_policy(&self) -> AbacPolicy {
        let mut policy = AbacPolicy::new(self.default_effect.into());
        for rule in &self.rules {
            policy = policy.with_rule(rule.to_abac_rule());
        }
        policy
    }
}

/// Fuzzer-friendly DataClass.
#[derive(Debug, Clone, Copy, Arbitrary)]
enum FuzzDataClass {
    Public,
    Internal,
    Confidential,
    PCI,
    PHI,
}

impl From<FuzzDataClass> for DataClass {
    fn from(f: FuzzDataClass) -> Self {
        match f {
            FuzzDataClass::Public => DataClass::Public,
            FuzzDataClass::Internal => DataClass::Internal,
            FuzzDataClass::Confidential => DataClass::Confidential,
            FuzzDataClass::PCI => DataClass::PCI,
            FuzzDataClass::PHI => DataClass::PHI,
        }
    }
}

/// Fuzzer-friendly attributes.
#[derive(Debug, Clone, Arbitrary)]
struct FuzzAttributes {
    // User attributes
    user_role: String,
    user_department: String,
    user_clearance: u8,
    user_tenant: u64,

    // Resource attributes
    resource_data_class: FuzzDataClass,
    resource_sensitivity: u8,
    resource_stream: String,

    // Environment attributes (simplified - use current time)
    env_country: String,
}

fuzz_target!(|input: (FuzzPolicy, FuzzAttributes)| {
    let (fuzz_policy, fuzz_attrs) = input;

    // Convert fuzzer types to real ABAC types
    let policy = fuzz_policy.to_abac_policy();
    let user = UserAttributes::new(
        &fuzz_attrs.user_role,
        &fuzz_attrs.user_department,
        fuzz_attrs.user_clearance,
    )
    .with_tenant_id(fuzz_attrs.user_tenant);

    let resource = ResourceAttributes::new(
        fuzz_attrs.resource_data_class.into(),
        fuzz_attrs.resource_sensitivity,
        &fuzz_attrs.resource_stream,
    );

    let env = EnvironmentAttributes::from_timestamp(Utc::now(), &fuzz_attrs.env_country);

    // Evaluate policy - should never panic
    let decision = evaluate(&policy, &user, &resource, &env);

    // AUDIT-2026-03 M-2: Validate ABAC invariants
    validate_abac_invariants(&policy, &decision);
});

/// Validates ABAC evaluator invariants.
///
/// **Invariants checked:**
/// 1. Decision is always either Allow or Deny (never indeterminate)
/// 2. Deny-by-default: If no rule matches and default is Deny, decision is Deny
/// 3. Decision has a reason (for audit trail)
/// 4. If a rule matches, decision effect matches rule effect
///
/// **Security Context:** AUDIT-2026-03 M-2, GDPR Art 5(2), HIPAA §164.308(a)(4)
fn validate_abac_invariants(policy: &AbacPolicy, decision: &Decision) {
    // Invariant 1: Decision is deterministic (Allow or Deny, never indeterminate)
    assert!(
        matches!(decision.effect, Effect::Allow | Effect::Deny),
        "ABAC decision must be either Allow or Deny, got {:?}",
        decision.effect
    );

    // Invariant 2: Decision has a reason for audit trail
    assert!(
        !decision.reason.is_empty(),
        "ABAC decision must have a non-empty reason for audit logging"
    );

    // Invariant 3: If default effect is Deny and no rules match, decision is Deny
    if policy.rules.is_empty() && policy.default_effect == Effect::Deny {
        assert!(
            decision.effect == Effect::Deny,
            "Empty policy with default Deny must result in Deny decision"
        );
    }

    // Invariant 4: If a rule matched, decision effect matches rule effect
    if let Some(ref matched_rule) = decision.matched_rule {
        let rule = policy.rules.iter().find(|r| r.name == *matched_rule);
        if let Some(rule) = rule {
            assert_eq!(
                decision.effect, rule.effect,
                "Decision effect must match matched rule effect"
            );
        }
    }

    // Invariant 5: Matched rule name is valid (exists in policy or is "default")
    if let Some(ref matched_rule) = decision.matched_rule {
        let is_valid = matched_rule == "default"
            || policy.rules.iter().any(|r| r.name == *matched_rule);
        assert!(
            is_valid,
            "Matched rule '{}' must exist in policy or be 'default'",
            matched_rule
        );
    }
}
