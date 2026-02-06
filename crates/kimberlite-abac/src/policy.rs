//! ABAC policy definitions.
//!
//! Policies consist of ordered rules, each with conditions that must all match
//! for the rule to apply. Rules are evaluated by priority (highest first), and
//! the first matching rule determines the outcome.

use serde::{Deserialize, Serialize};

// ============================================================================
// Effect
// ============================================================================

/// The effect of a policy rule: allow or deny access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    /// Grant access.
    Allow,
    /// Deny access.
    Deny,
}

impl Default for Effect {
    /// Defaults to `Deny` (safe default: deny unless explicitly allowed).
    fn default() -> Self {
        Self::Deny
    }
}

// ============================================================================
// Condition
// ============================================================================

/// A condition that must be satisfied for a rule to match.
///
/// Conditions can be combined with logical operators (`And`, `Or`, `Not`)
/// to express arbitrarily complex access control policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    // -- User attribute conditions --
    /// User's role must equal the specified value.
    RoleEquals(String),
    /// User's clearance level must be >= the specified value.
    ClearanceLevelAtLeast(u8),
    /// User's department must equal the specified value.
    DepartmentEquals(String),
    /// User's tenant must equal the specified value.
    TenantEquals(u64),

    // -- Resource attribute conditions --
    /// Resource data class must be at or below the specified level.
    /// Uses sensitivity ordering: Public < Confidential < ... < PHI.
    DataClassAtMost(String),
    /// Resource stream name must match the specified glob pattern.
    /// Supports `*` (any characters) and `?` (single character) wildcards.
    StreamNameMatches(String),

    // -- Environment conditions --
    /// Access is only permitted during business hours (09:00-17:00 UTC, weekdays).
    BusinessHoursOnly,
    /// Request source country must be in the specified list.
    CountryIn(Vec<String>),
    /// Request source country must NOT be in the specified list.
    CountryNotIn(Vec<String>),

    // -- Logical combinators --
    /// All sub-conditions must be true.
    And(Vec<Condition>),
    /// At least one sub-condition must be true.
    Or(Vec<Condition>),
    /// The sub-condition must be false.
    Not(Box<Condition>),
}

// ============================================================================
// Rule
// ============================================================================

/// A single access control rule within a policy.
///
/// Rules are evaluated in priority order (highest first). The first rule
/// whose conditions all match determines the access decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Human-readable name for audit logging.
    pub name: String,
    /// The effect when this rule matches.
    pub effect: Effect,
    /// All conditions must be true for this rule to match.
    pub conditions: Vec<Condition>,
    /// Evaluation priority. Higher values are evaluated first.
    pub priority: u32,
}

// ============================================================================
// AbacPolicy
// ============================================================================

/// An Attribute-Based Access Control policy.
///
/// Contains a set of rules evaluated against request attributes.
/// When no rule matches, the `default_effect` is applied (defaults to `Deny`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbacPolicy {
    /// The rules in this policy (evaluated in priority order).
    pub rules: Vec<Rule>,
    /// Effect applied when no rule matches. Defaults to `Deny`.
    pub default_effect: Effect,
}

impl Default for AbacPolicy {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_effect: Effect::Deny,
        }
    }
}

impl AbacPolicy {
    /// Creates a new policy with the specified default effect.
    pub fn new(default_effect: Effect) -> Self {
        Self {
            rules: Vec::new(),
            default_effect,
        }
    }

    /// Adds a rule to the policy (builder pattern).
    pub fn with_rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Returns a HIPAA-compliant policy.
    ///
    /// Rules:
    /// 1. PHI data requires clearance >= 2 AND business hours
    /// 2. Non-PHI data is allowed with any clearance
    pub fn hipaa_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "hipaa-phi-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::BusinessHoursOnly,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "hipaa-non-phi-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a FedRAMP-compliant policy.
    ///
    /// Rules:
    /// 1. Deny access from outside the US
    /// 2. Allow US-origin requests
    pub fn fedramp_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "fedramp-us-only".to_string(),
                effect: Effect::Deny,
                conditions: vec![Condition::CountryNotIn(vec!["US".to_string()])],
                priority: 100,
            })
            .with_rule(Rule {
                name: "fedramp-allow-us".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(vec!["US".to_string()])],
                priority: 50,
            })
    }

    /// Returns a PCI DSS-compliant policy.
    ///
    /// Rules:
    /// 1. PCI data only from Server devices with clearance >= 2
    /// 2. Non-PCI data allowed with any clearance
    pub fn pci_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "pci-server-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::DepartmentEquals("Server".to_string()),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "pci-non-pci-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_effect_is_deny() {
        let policy = AbacPolicy::default();
        assert_eq!(policy.default_effect, Effect::Deny);
    }

    #[test]
    fn test_hipaa_policy_structure() {
        let policy = AbacPolicy::hipaa_policy();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert_eq!(policy.rules.len(), 2);

        // PHI rule has higher priority
        assert_eq!(policy.rules[0].name, "hipaa-phi-access");
        assert_eq!(policy.rules[0].priority, 10);
        assert_eq!(policy.rules[0].effect, Effect::Allow);

        // Non-PHI fallback has lower priority
        assert_eq!(policy.rules[1].name, "hipaa-non-phi-access");
        assert_eq!(policy.rules[1].priority, 5);
    }

    #[test]
    fn test_fedramp_policy_structure() {
        let policy = AbacPolicy::fedramp_policy();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert_eq!(policy.rules.len(), 2);

        // Deny rule has highest priority
        assert_eq!(policy.rules[0].name, "fedramp-us-only");
        assert_eq!(policy.rules[0].effect, Effect::Deny);
        assert!(policy.rules[0].priority > policy.rules[1].priority);
    }

    #[test]
    fn test_pci_policy_structure() {
        let policy = AbacPolicy::pci_policy();
        assert_eq!(policy.default_effect, Effect::Deny);
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[0].name, "pci-server-access");
        assert_eq!(policy.rules[0].effect, Effect::Allow);
    }

    #[test]
    fn test_with_rule_builder() {
        let policy = AbacPolicy::new(Effect::Allow)
            .with_rule(Rule {
                name: "rule-a".to_string(),
                effect: Effect::Deny,
                conditions: vec![Condition::BusinessHoursOnly],
                priority: 1,
            })
            .with_rule(Rule {
                name: "rule-b".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::ClearanceLevelAtLeast(1)],
                priority: 2,
            });

        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.default_effect, Effect::Allow);
    }

    #[test]
    fn test_condition_serialization_roundtrip() {
        let condition = Condition::And(vec![
            Condition::RoleEquals("admin".to_string()),
            Condition::Not(Box::new(Condition::BusinessHoursOnly)),
        ]);

        let json = serde_json::to_string(&condition).expect("serialize condition");
        let deserialized: Condition = serde_json::from_str(&json).expect("deserialize condition");
        assert_eq!(condition, deserialized);
    }

    #[test]
    fn test_policy_serialization_roundtrip() {
        let policy = AbacPolicy::hipaa_policy();
        let json = serde_json::to_string(&policy).expect("serialize policy");
        let deserialized: AbacPolicy = serde_json::from_str(&json).expect("deserialize policy");

        assert_eq!(deserialized.default_effect, policy.default_effect);
        assert_eq!(deserialized.rules.len(), policy.rules.len());
    }
}
