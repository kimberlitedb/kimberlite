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

    // -- Compliance-specific conditions --
    /// Data retention period must be at least the specified number of days.
    /// Used for SOX (7yr/2555d), HIPAA (6yr/2190d), PCI DSS (1yr/365d).
    RetentionPeriodAtLeast(u32),
    /// Data correction/amendment is allowed (CCPA, Australian APPs).
    DataCorrectionAllowed,
    /// Incident must be reported within the specified number of hours.
    /// NIS2 (24h), NDB (30 days = 720h), breach module (72h).
    IncidentReportingDeadline(u32),
    /// Access restricted to specific fields only (HITECH minimum necessary).
    FieldLevelRestriction(Vec<String>),
    /// Operations must follow a specific sequence (21 CFR Part 11 review-then-approve).
    OperationalSequencing(Vec<String>),
    /// A legal hold is active, preventing deletion (Legal Compliance).
    LegalHoldActive,

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

    /// Returns a HITECH-compliant policy (extends HIPAA).
    ///
    /// Rules:
    /// 1. PHI requires clearance >= 2, business hours, and field-level restriction on patient_* streams
    /// 2. Non-PHI data allowed with any clearance
    pub fn hitech_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "hitech-phi-minimum-necessary".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::BusinessHoursOnly,
                    Condition::FieldLevelRestriction(vec![
                        "patient_id".to_string(),
                        "patient_name".to_string(),
                        "patient_dob".to_string(),
                    ]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "hitech-non-phi-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a 21 CFR Part 11-compliant policy (electronic records).
    ///
    /// Rules:
    /// 1. Electronic records require clearance >= 3 and operational sequencing
    /// 2. Read-only access with clearance >= 2
    pub fn cfr21_part11_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "cfr21-electronic-records".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(3),
                    Condition::OperationalSequencing(vec![
                        "Authorship".to_string(),
                        "Review".to_string(),
                        "Approval".to_string(),
                    ]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "cfr21-read-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::ClearanceLevelAtLeast(2)],
                priority: 5,
            })
    }

    /// Returns a SOX-compliant policy (Sarbanes-Oxley).
    ///
    /// Rules:
    /// 1. Financial data requires clearance >= 2 and 7-year retention
    /// 2. Non-financial data allowed with any clearance
    pub fn sox_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "sox-financial-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::RetentionPeriodAtLeast(2555),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "sox-non-financial-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a GLBA-compliant policy (Gramm-Leach-Bliley Act).
    ///
    /// Rules:
    /// 1. Financial data requires clearance >= 2 and US-only access
    /// 2. Non-financial data allowed with any clearance
    pub fn glba_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "glba-financial-us-only".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::CountryIn(vec!["US".to_string()]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "glba-non-financial-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a CCPA-compliant policy (California Consumer Privacy Act).
    ///
    /// Rules:
    /// 1. PII requires clearance >= 1 and data correction must be allowed
    /// 2. Non-PII data allowed with any clearance
    pub fn ccpa_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "ccpa-pii-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(1),
                    Condition::DataCorrectionAllowed,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "ccpa-non-pii-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a FERPA-compliant policy (student data protection).
    ///
    /// Rules:
    /// 1. Student PII requires clearance >= 2 and business hours
    /// 2. Non-PII data allowed with any clearance
    pub fn ferpa_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "ferpa-student-data".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::BusinessHoursOnly,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "ferpa-non-pii-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Confidential".to_string())],
                priority: 5,
            })
    }

    /// Returns a NIST 800-53-compliant policy.
    ///
    /// Rules:
    /// 1. Sensitive data requires clearance >= 2 and US-only access
    /// 2. Public data allowed with any clearance
    pub fn nist_800_53_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "nist-sensitive-us-only".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::CountryIn(vec!["US".to_string()]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "nist-public-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Public".to_string())],
                priority: 5,
            })
    }

    /// Returns a CMMC-compliant policy (Cybersecurity Maturity Model Certification).
    ///
    /// Rules:
    /// 1. Controlled data requires clearance >= 2 and US-only access
    /// 2. Public data allowed with any clearance
    pub fn cmmc_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "cmmc-controlled-us-only".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::CountryIn(vec!["US".to_string()]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "cmmc-public-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Public".to_string())],
                priority: 5,
            })
    }

    /// Returns a legal compliance policy.
    ///
    /// Rules:
    /// 1. Legal hold active blocks all deletion
    /// 2. Confidential+ data requires clearance >= 2
    pub fn legal_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "legal-hold-block-deletion".to_string(),
                effect: Effect::Deny,
                conditions: vec![Condition::LegalHoldActive],
                priority: 100,
            })
            .with_rule(Rule {
                name: "legal-confidential-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::ClearanceLevelAtLeast(2)],
                priority: 10,
            })
    }

    /// Returns a NIS2-compliant policy (EU Network and Information Security).
    ///
    /// Rules:
    /// 1. EU-only access with 24-hour incident reporting deadline
    /// 2. Deny non-EU access
    pub fn nis2_policy() -> Self {
        let eu_countries = vec![
            "DE".to_string(), "FR".to_string(), "NL".to_string(), "IT".to_string(),
            "ES".to_string(), "BE".to_string(), "AT".to_string(), "PT".to_string(),
            "IE".to_string(), "FI".to_string(), "SE".to_string(), "DK".to_string(),
            "PL".to_string(), "CZ".to_string(), "RO".to_string(), "BG".to_string(),
            "HR".to_string(), "SK".to_string(), "SI".to_string(), "LT".to_string(),
            "LV".to_string(), "EE".to_string(), "CY".to_string(), "MT".to_string(),
            "LU".to_string(), "HU".to_string(), "GR".to_string(),
        ];
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "nis2-eu-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::CountryIn(eu_countries),
                    Condition::IncidentReportingDeadline(24),
                ])],
                priority: 10,
            })
    }

    /// Returns a DORA-compliant policy (Digital Operational Resilience Act).
    ///
    /// Rules:
    /// 1. Financial data requires clearance >= 2 and EU-only access
    /// 2. Non-financial data allowed within the EU
    pub fn dora_policy() -> Self {
        let eu_countries = vec![
            "DE".to_string(), "FR".to_string(), "NL".to_string(), "IT".to_string(),
            "ES".to_string(), "BE".to_string(), "AT".to_string(), "PT".to_string(),
            "IE".to_string(), "FI".to_string(), "SE".to_string(), "DK".to_string(),
            "PL".to_string(), "CZ".to_string(), "RO".to_string(), "BG".to_string(),
            "HR".to_string(), "SK".to_string(), "SI".to_string(), "LT".to_string(),
            "LV".to_string(), "EE".to_string(), "CY".to_string(), "MT".to_string(),
            "LU".to_string(), "HU".to_string(), "GR".to_string(),
        ];
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "dora-financial-eu-only".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::CountryIn(eu_countries.clone()),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "dora-non-financial-eu".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(eu_countries)],
                priority: 5,
            })
    }

    /// Returns an eIDAS-compliant policy (EU electronic identification).
    ///
    /// Rules:
    /// 1. Qualified signatures require EU-only and operational sequencing
    /// 2. Basic access within the EU
    pub fn eidas_policy() -> Self {
        let eu_countries = vec![
            "DE".to_string(), "FR".to_string(), "NL".to_string(), "IT".to_string(),
            "ES".to_string(), "BE".to_string(), "AT".to_string(), "PT".to_string(),
            "IE".to_string(), "FI".to_string(), "SE".to_string(), "DK".to_string(),
            "PL".to_string(), "CZ".to_string(), "RO".to_string(), "BG".to_string(),
            "HR".to_string(), "SK".to_string(), "SI".to_string(), "LT".to_string(),
            "LV".to_string(), "EE".to_string(), "CY".to_string(), "MT".to_string(),
            "LU".to_string(), "HU".to_string(), "GR".to_string(),
        ];
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "eidas-qualified-signatures".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::CountryIn(eu_countries.clone()),
                    Condition::OperationalSequencing(vec![
                        "Identification".to_string(),
                        "Authentication".to_string(),
                        "Signing".to_string(),
                    ]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "eidas-basic-eu-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(eu_countries)],
                priority: 5,
            })
    }

    /// Returns a GDPR-compliant policy.
    ///
    /// Rules:
    /// 1. PII requires clearance >= 1, EU-only, and data correction allowed
    /// 2. Non-PII data allowed within the EU
    pub fn gdpr_policy() -> Self {
        let eu_countries = vec![
            "DE".to_string(), "FR".to_string(), "NL".to_string(), "IT".to_string(),
            "ES".to_string(), "BE".to_string(), "AT".to_string(), "PT".to_string(),
            "IE".to_string(), "FI".to_string(), "SE".to_string(), "DK".to_string(),
            "PL".to_string(), "CZ".to_string(), "RO".to_string(), "BG".to_string(),
            "HR".to_string(), "SK".to_string(), "SI".to_string(), "LT".to_string(),
            "LV".to_string(), "EE".to_string(), "CY".to_string(), "MT".to_string(),
            "LU".to_string(), "HU".to_string(), "GR".to_string(),
        ];
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "gdpr-pii-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(1),
                    Condition::CountryIn(eu_countries.clone()),
                    Condition::DataCorrectionAllowed,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "gdpr-non-pii-eu".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(eu_countries)],
                priority: 5,
            })
    }

    /// Returns an ISO 27001-compliant policy.
    ///
    /// Rules:
    /// 1. Confidential+ data requires clearance >= 2 and business hours
    /// 2. Public data allowed with any clearance
    pub fn iso27001_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "iso27001-confidential-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::BusinessHoursOnly,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "iso27001-public-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::DataClassAtMost("Public".to_string())],
                priority: 5,
            })
    }

    /// Returns an Australian Privacy Act-compliant policy.
    ///
    /// Rules:
    /// 1. PII requires clearance >= 1, AU-only, and data correction allowed
    /// 2. Non-PII data allowed within AU
    pub fn aus_privacy_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "aus-privacy-pii-access".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(1),
                    Condition::CountryIn(vec!["AU".to_string()]),
                    Condition::DataCorrectionAllowed,
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "aus-privacy-non-pii".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(vec!["AU".to_string()])],
                priority: 5,
            })
    }

    /// Returns an APRA CPS 234-compliant policy (Australian prudential regulation).
    ///
    /// Rules:
    /// 1. Financial data requires clearance >= 2 and AU-only access
    /// 2. Non-financial data allowed within AU
    pub fn apra_cps234_policy() -> Self {
        Self::new(Effect::Deny)
            .with_rule(Rule {
                name: "apra-financial-au-only".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::And(vec![
                    Condition::ClearanceLevelAtLeast(2),
                    Condition::CountryIn(vec!["AU".to_string()]),
                ])],
                priority: 10,
            })
            .with_rule(Rule {
                name: "apra-non-financial-au".to_string(),
                effect: Effect::Allow,
                conditions: vec![Condition::CountryIn(vec!["AU".to_string()])],
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
