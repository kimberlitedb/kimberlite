#![no_main]

use arbitrary::Arbitrary;
use chrono::Utc;
use kimberlite_abac::policy::Effect;
use kimberlite_abac::{
    AbacPolicy, Condition, Decision, EnvironmentAttributes, ResourceAttributes, Rule,
    UserAttributes, evaluate,
};
use kimberlite_types::DataClass;
use libfuzzer_sys::fuzz_target;

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
/// Simplified to avoid unbounded recursion.
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
    And2(Box<FuzzCondition>, Box<FuzzCondition>),
    Or2(Box<FuzzCondition>, Box<FuzzCondition>),
    Not(Box<FuzzCondition>),
}

impl FuzzCondition {
    /// Depth-limited conversion to avoid stack overflow from deep And/Or/Not chains.
    fn to_abac_condition(&self, depth: u8) -> Condition {
        if depth > 3 {
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
            conditions: self
                .conditions
                .iter()
                .map(|c| c.to_abac_condition(0))
                .collect(),
            priority: self.priority,
        }
    }
}

#[derive(Debug, Clone, Arbitrary)]
struct FuzzPolicy {
    rules: Vec<FuzzRule>,
    default_effect: FuzzEffect,
}

impl FuzzPolicy {
    fn to_abac_policy(&self) -> AbacPolicy {
        let mut policy = AbacPolicy::new(self.default_effect.into());
        for rule in &self.rules {
            // Drop duplicates: the new "one rule per name" policy semantics
            // reject duplicate-named rules. Fuzz input can produce duplicates,
            // so skip and continue rather than abort.
            let abac_rule = rule.to_abac_rule();
            if policy.rules.iter().any(|r| r.name == abac_rule.name) {
                continue;
            }
            policy = policy.with_rule(abac_rule).expect("duplicate pre-checked");
        }
        policy
    }
}

/// Fuzzer-friendly DataClass mapped onto the actual `DataClass` variants
/// exposed by `kimberlite-types`.
#[derive(Debug, Clone, Copy, Arbitrary)]
enum FuzzDataClass {
    Public,
    Confidential,
    PII,
    Sensitive,
    PCI,
    Financial,
    PHI,
    Deidentified,
}

impl From<FuzzDataClass> for DataClass {
    fn from(f: FuzzDataClass) -> Self {
        match f {
            FuzzDataClass::Public => DataClass::Public,
            FuzzDataClass::Confidential => DataClass::Confidential,
            FuzzDataClass::PII => DataClass::PII,
            FuzzDataClass::Sensitive => DataClass::Sensitive,
            FuzzDataClass::PCI => DataClass::PCI,
            FuzzDataClass::Financial => DataClass::Financial,
            FuzzDataClass::PHI => DataClass::PHI,
            FuzzDataClass::Deidentified => DataClass::Deidentified,
        }
    }
}

#[derive(Debug, Clone, Arbitrary)]
struct FuzzAttributes {
    user_role: String,
    user_department: String,
    user_clearance: u8,
    user_tenant: u64,

    resource_data_class: FuzzDataClass,
    resource_owner_tenant: u64,
    resource_stream: String,

    env_country: String,
}

fuzz_target!(|input: (FuzzPolicy, FuzzAttributes)| {
    let (fuzz_policy, fuzz_attrs) = input;

    let policy = fuzz_policy.to_abac_policy();
    let user = UserAttributes::new(
        &fuzz_attrs.user_role,
        &fuzz_attrs.user_department,
        fuzz_attrs.user_clearance,
    )
    .with_tenant(fuzz_attrs.user_tenant);

    let resource = ResourceAttributes::new(
        fuzz_attrs.resource_data_class.into(),
        fuzz_attrs.resource_owner_tenant,
        &fuzz_attrs.resource_stream,
    );

    let env = EnvironmentAttributes::from_timestamp(Utc::now(), &fuzz_attrs.env_country);

    // Evaluation must never panic.
    let decision = evaluate(&policy, &user, &resource, &env);

    validate_abac_invariants(&policy, &decision);
});

/// Invariants the ABAC evaluator must preserve:
///   1. Decision effect is Allow or Deny — never indeterminate.
///   2. Decision carries a non-empty reason string for audit.
///   3. Empty policy + default-Deny ⇒ Deny.
///   4. If a named rule matched, the decision's effect matches that rule's effect.
///   5. The matched rule name is either a rule in the policy or "default".
fn validate_abac_invariants(policy: &AbacPolicy, decision: &Decision) {
    assert!(
        matches!(decision.effect, Effect::Allow | Effect::Deny),
        "ABAC decision must be either Allow or Deny, got {:?}",
        decision.effect
    );

    assert!(
        !decision.reason.is_empty(),
        "ABAC decision must have a non-empty reason for audit logging"
    );

    if policy.rules.is_empty() && policy.default_effect == Effect::Deny {
        assert_eq!(
            decision.effect,
            Effect::Deny,
            "empty policy with default Deny must produce a Deny decision"
        );
    }

    if let Some(ref matched_rule) = decision.matched_rule {
        if let Some(rule) = policy.rules.iter().find(|r| r.name == *matched_rule) {
            assert_eq!(
                decision.effect, rule.effect,
                "decision effect must match matched rule's effect"
            );
        }

        let is_valid =
            matched_rule == "default" || policy.rules.iter().any(|r| r.name == *matched_rule);
        assert!(
            is_valid,
            "matched rule {matched_rule:?} must exist in policy or equal \"default\""
        );
    }
}
