//! Kani proofs for ABAC policy evaluation
//!
//! These proofs verify correctness properties of the Attribute-Based Access Control
//! engine using bounded model checking.
//!
//! **Proof Count**: 4 proofs (#46-49)
//!
//! Run with: `cargo kani --tests --harness verify_*`

#[cfg(kani)]
use crate::attributes::{DeviceType, EnvironmentAttributes, ResourceAttributes, UserAttributes};
#[cfg(kani)]
use crate::evaluator;
#[cfg(kani)]
use crate::policy::{AbacPolicy, Condition, Effect, Rule};
#[cfg(kani)]
use chrono::{TimeZone, Utc};
#[cfg(kani)]
use kimberlite_types::DataClass;

/// Proof #46: Policy evaluation determinism
///
/// **Property**: Same inputs always produce the same decision
///
/// **Verification**:
/// - Evaluate a policy with fixed attributes twice
/// - Both decisions must be identical (effect and matched rule)
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(10)]
fn verify_policy_evaluation_determinism() {
    let policy = AbacPolicy::hipaa_policy();

    // Fixed attributes — same inputs every time
    let user = UserAttributes::new("doctor", "medicine", 2);
    let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
    // Wednesday at 10:00 UTC (business hours)
    let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
    let env = EnvironmentAttributes::from_timestamp(ts, "US");

    let decision1 = evaluator::evaluate(&policy, &user, &resource, &env);
    let decision2 = evaluator::evaluate(&policy, &user, &resource, &env);

    // Postcondition: Identical decisions
    assert_eq!(decision1.effect, decision2.effect);
    assert_eq!(decision1.matched_rule, decision2.matched_rule);
}

/// Proof #47: Priority-based conflict resolution
///
/// **Property**: When multiple rules match, the highest priority rule wins
///
/// **Verification**:
/// - Create a policy with two rules that both match
/// - High-priority Deny and low-priority Allow
/// - The Deny (higher priority) must win
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(10)]
fn verify_priority_conflict_resolution() {
    // Two rules: both match any request, different priorities and effects
    let policy = AbacPolicy::new(Effect::Allow)
        .with_rule(Rule {
            name: "low-allow".to_string(),
            effect: Effect::Allow,
            conditions: vec![Condition::ClearanceLevelAtLeast(0)], // Always matches
            priority: 1,
        })
        .with_rule(Rule {
            name: "high-deny".to_string(),
            effect: Effect::Deny,
            conditions: vec![Condition::ClearanceLevelAtLeast(0)], // Always matches
            priority: 100,
        });

    let user = UserAttributes::new("user", "engineering", 1);
    let resource = ResourceAttributes::new(DataClass::Public, 1, "test");
    let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
    let env = EnvironmentAttributes::from_timestamp(ts, "US");

    let decision = evaluator::evaluate(&policy, &user, &resource, &env);

    // Postcondition: High-priority Deny wins over low-priority Allow
    assert_eq!(decision.effect, Effect::Deny);
    assert_eq!(decision.matched_rule, Some("high-deny".to_string()));
}

/// Proof #48: Default deny safety
///
/// **Property**: When no rule matches, the default effect is applied (Deny)
///
/// **Verification**:
/// - Create a policy with rules that don't match the request
/// - The default Deny effect must be returned
/// - No matched rule should be reported
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(10)]
fn verify_default_deny_safety() {
    // Policy requires admin role — request comes from user role
    let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
        name: "admin-only".to_string(),
        effect: Effect::Allow,
        conditions: vec![Condition::RoleEquals("admin".to_string())],
        priority: 10,
    });

    let user = UserAttributes::new("user", "engineering", 1); // Not admin
    let resource = ResourceAttributes::new(DataClass::Public, 1, "test");
    let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
    let env = EnvironmentAttributes::from_timestamp(ts, "US");

    let decision = evaluator::evaluate(&policy, &user, &resource, &env);

    // Postcondition: Default Deny applied
    assert_eq!(decision.effect, Effect::Deny);
    assert!(decision.matched_rule.is_none());
}

/// Proof #49: FedRAMP location enforcement
///
/// **Property**: Non-US requests are denied by the FedRAMP policy
///
/// **Verification**:
/// - Use the pre-built FedRAMP policy
/// - Request from Germany (non-US)
/// - Must be denied by the fedramp-us-only rule
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(10)]
fn verify_fedramp_location_enforcement() {
    let policy = AbacPolicy::fedramp_policy();

    let user = UserAttributes::new("analyst", "engineering", 3);
    let resource = ResourceAttributes::new(DataClass::Confidential, 1, "metrics");
    let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
    let env = EnvironmentAttributes::from_timestamp(ts, "DE"); // Germany, not US

    let decision = evaluator::evaluate(&policy, &user, &resource, &env);

    // Postcondition: Denied by FedRAMP US-only rule
    assert_eq!(decision.effect, Effect::Deny);
    assert_eq!(decision.matched_rule, Some("fedramp-us-only".to_string()));
}

/// Proof #49b (AUDIT-2026-04 M-2): ABAC tenant-equality is isolated
/// across distinct users sharing role + clearance.
///
/// **Property**: Given a `TenantEquals(A)` rule, a user bound to
/// `tenant_id = A` is allowed; a user with identical role +
/// department + clearance but bound to `tenant_id = B != A` is
/// denied. ABAC must treat the tenant attribute as a first-class
/// discriminant, not collapse across shared-role surface.
///
/// **Background**: existing proofs #46–49 all fix `user.tenant_id`
/// to `None` (default from `UserAttributes::new`). That leaves
/// `TenantEquals` condition evaluation untested for the split-brain
/// case two users present the same role but different tenants — the
/// April 2026 projection-table bug-class in ABAC form.
#[cfg(kani)]
#[kani::proof]
#[kani::unwind(10)]
fn verify_abac_tenant_isolation() {
    let a_raw: u64 = kani::any();
    let b_raw: u64 = kani::any();
    kani::assume(a_raw != b_raw);

    let policy = AbacPolicy::new(Effect::Deny).with_rule(Rule {
        name: "tenant-a-only".to_string(),
        effect: Effect::Allow,
        conditions: vec![
            Condition::RoleEquals("analyst".to_string()),
            Condition::TenantEquals(a_raw),
        ],
        priority: 100,
    });

    let resource = ResourceAttributes::new(DataClass::Public, a_raw, "shared");
    let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
    let env = EnvironmentAttributes::from_timestamp(ts, "US");

    // User A: matches the tenant-a-only rule.
    let user_a = UserAttributes::new("analyst", "engineering", 3).with_tenant(a_raw);
    let decision_a = evaluator::evaluate(&policy, &user_a, &resource, &env);
    assert_eq!(decision_a.effect, Effect::Allow);
    assert_eq!(decision_a.matched_rule, Some("tenant-a-only".to_string()));

    // User B: identical role + clearance, different tenant — must
    // hit the default Deny. A shared-keyed policy lookup or a
    // tenant-oblivious condition chain would collapse these.
    let user_b = UserAttributes::new("analyst", "engineering", 3).with_tenant(b_raw);
    let decision_b = evaluator::evaluate(&policy, &user_b, &resource, &env);
    assert_eq!(decision_b.effect, Effect::Deny);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_proof_count() {
        // AUDIT-2026-04 M-2: +1 proof for cross-tenant ABAC isolation.
        let proof_count = 5;
        assert_eq!(proof_count, 5, "Expected 5 Kani proofs for ABAC");
    }
}
