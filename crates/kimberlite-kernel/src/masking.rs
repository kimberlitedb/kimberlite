//! Kernel-level masking policy catalogue.
//!
//! v0.6.0 Tier 2 #7 — column-level masking policy CRUD. The DDL layer
//! compiles a `CREATE MASKING POLICY ... AS <CASE expr>` into a
//! [`MaskingStrategyKind`] plus [`RoleGuard`], both of which are
//! serde-stable and fully independent of `kimberlite-rbac`'s concrete
//! [`FieldMask`](kimberlite_rbac::masking::FieldMask) type.
//!
//! ## Why kernel-local types?
//!
//! The kernel lives beneath every other crate (crypto, rbac, storage,
//! query…). Storing a `FieldMask` directly would force the kernel to
//! depend on `kimberlite-rbac`, inverting the layering. Instead, we
//! define kernel-local mirrors for the five masking strategies and a
//! `RoleGuard` describing which roles see the masked vs raw value, then
//! translate at the RBAC layer.
//!
//! ## Persistence through backup/restore
//!
//! Because masking policies and their attachments are durable kernel
//! state, they replay deterministically from the command log. Backup
//! (checkpoint of `State`) + restore (replay) round-trips them without
//! any separate bookkeeping.

use kimberlite_types::TenantId;
use serde::{Deserialize, Serialize};

use crate::command::TableId;

// ---------------------------------------------------------------------------
// Strategy kinds (mirror of kimberlite-rbac::masking::MaskingStrategy)
// ---------------------------------------------------------------------------

/// Redaction patterns that preserve a formatted-suffix of the cleartext.
///
/// Keep in lock-step with `kimberlite_rbac::masking::RedactPattern`; the
/// RBAC layer translates on read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RedactPatternKind {
    /// SSN (`***-**-6789`).
    Ssn,
    /// Phone (`***-***-4567`).
    Phone,
    /// Email (`j***@example.com`).
    Email,
    /// Credit card (`****-****-****-1234`).
    CreditCard,
    /// Custom fixed replacement (`Custom { replacement: "***" }`).
    Custom {
        /// The literal replacement string.
        replacement: String,
    },
}

/// Kernel-local mirror of the five masking strategies.
///
/// Translated to [`kimberlite_rbac::masking::MaskingStrategy`] by
/// `tenant.rs::apply_masking_policies_to_result`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaskingStrategyKind {
    /// Pattern-aware partial redaction.
    Redact(RedactPatternKind),
    /// SHA-256 hex digest.
    Hash,
    /// BLAKE3-derived deterministic token prefixed with `tok_`.
    Tokenize,
    /// Keep first `max_chars`, pad with `"..."`.
    Truncate {
        /// Number of leading characters to preserve.
        max_chars: usize,
    },
    /// Replace with the empty/NULL value.
    Null,
}

// ---------------------------------------------------------------------------
// Role guard — decomposed from the DDL CASE expression
// ---------------------------------------------------------------------------

/// Decomposed form of the DDL CASE expression's role predicate.
///
/// `CREATE MASKING POLICY p AS CASE WHEN session_role IN ('a','b') THEN @col ELSE '***' END`
/// compiles to:
///
/// ```text
/// RoleGuard {
///     exempt_roles: ["a", "b"],   // roles that see the raw value
///     default_masked: true,       // all other roles see the masked value
/// }
/// ```
///
/// The role strings are deliberately free-form here — the kernel does
/// not enumerate the four built-in [`kimberlite_rbac::Role`] values.
/// Translation happens at the RBAC boundary.
///
/// Invariant: roles are ASCII-lower-cased at parse time so the same
/// policy attached under two different case variants (`'Clinician'`
/// vs `'clinician'`) compares equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoleGuard {
    /// Roles exempt from masking — they see the raw column value.
    pub exempt_roles: Vec<String>,
    /// `true` when the ELSE branch yields the masked literal; `false`
    /// (currently not reachable from the DDL) would invert the guard.
    /// Kept as an explicit field for forward compatibility with future
    /// syntaxes like `CASE WHEN role = 'ops' THEN '***' ELSE @col END`.
    pub default_masked: bool,
}

impl RoleGuard {
    /// Returns `true` if the given session role should see the masked value.
    pub fn should_mask(&self, session_role: &str) -> bool {
        let lower = session_role.to_ascii_lowercase();
        let exempt = self.exempt_roles.iter().any(|r| r == &lower);
        // `default_masked = true` (the only currently-reachable form):
        // everyone NOT in `exempt_roles` gets masked.
        // `default_masked = false` would flip this for the inverted form.
        if self.default_masked {
            !exempt
        } else {
            exempt
        }
    }
}

// ---------------------------------------------------------------------------
// Policy record — one CREATE MASKING POLICY, pre-attachment
// ---------------------------------------------------------------------------

/// A named masking policy definition, tenant-scoped.
///
/// Carries the strategy + role guard but **not** the set of attached
/// `(table, column)` pairs — those live in [`MaskingAttachment`] so
/// many columns can share one policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskingPolicyRecord {
    /// Owning tenant.
    pub tenant_id: TenantId,
    /// Policy name (unique per tenant).
    pub name: String,
    /// Compiled strategy.
    pub strategy: MaskingStrategyKind,
    /// Role guard decomposed from the `CASE WHEN session_role ...` clause.
    pub role_guard: RoleGuard,
}

/// An attachment of a masking policy to a specific column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskingAttachment {
    /// Table being masked.
    pub table_id: TableId,
    /// Column name (case-sensitive; SQL identifiers normalised upstream).
    pub column_name: String,
    /// The policy being attached (looked up in the tenant's catalogue).
    pub policy_name: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_guard_default_masked_exempt_roles_pass_through() {
        let g = RoleGuard {
            exempt_roles: vec!["clinician".into(), "nurse".into()],
            default_masked: true,
        };
        assert!(!g.should_mask("clinician"));
        assert!(!g.should_mask("Clinician")); // case-insensitive
        assert!(!g.should_mask("NURSE"));
        assert!(g.should_mask("reception"));
        assert!(g.should_mask("auditor"));
    }

    #[test]
    fn role_guard_inverted_form_masks_listed_roles() {
        // Currently not reachable from DDL but the data model supports it.
        let g = RoleGuard {
            exempt_roles: vec!["ops".into()],
            default_masked: false,
        };
        assert!(g.should_mask("ops"));
        assert!(!g.should_mask("clinician"));
    }

    #[test]
    fn role_guard_empty_exempt_masks_everyone() {
        let g = RoleGuard {
            exempt_roles: vec![],
            default_masked: true,
        };
        assert!(g.should_mask("clinician"));
        assert!(g.should_mask("any"));
    }

    #[test]
    fn strategy_kinds_roundtrip_serde() {
        let variants = [
            MaskingStrategyKind::Redact(RedactPatternKind::Ssn),
            MaskingStrategyKind::Redact(RedactPatternKind::Phone),
            MaskingStrategyKind::Redact(RedactPatternKind::Email),
            MaskingStrategyKind::Redact(RedactPatternKind::CreditCard),
            MaskingStrategyKind::Redact(RedactPatternKind::Custom {
                replacement: "***".into(),
            }),
            MaskingStrategyKind::Hash,
            MaskingStrategyKind::Tokenize,
            MaskingStrategyKind::Truncate { max_chars: 4 },
            MaskingStrategyKind::Null,
        ];
        for v in variants {
            let s = serde_json::to_string(&v).expect("serialize");
            let back: MaskingStrategyKind = serde_json::from_str(&s).expect("deserialize");
            assert_eq!(v, back);
        }
    }
}
