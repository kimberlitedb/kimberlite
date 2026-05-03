//! GDPR Article 7 Consent Management
//!
//! This module implements consent tracking for personal data processing under GDPR.
//!
//! # GDPR Requirements
//!
//! **Article 7(1)**: Controller must demonstrate consent was given
//! **Article 7(3)**: Withdrawal of consent as easy as giving consent
//! **Article 7(4)**: Consent not valid if processing is condition of contract performance
//!
//! # Architecture
//!
//! ```text
//! ConsentRecord = {
//!     consent_id: Uuid,
//!     subject_id: String,         // Data subject identifier
//!     purpose: Purpose,            // Why data is processed
//!     granted_at: Timestamp,       // When consent was given
//!     withdrawn_at: Option<Timestamp>,
//!     granularity: ConsentScope,   // What data is covered
//! }
//! ```
//!
//! # Example
//!
//! ```
//! use kimberlite_compliance::consent::{ConsentTracker, ConsentRecord};
//! use kimberlite_compliance::purpose::Purpose;
//!
//! let mut tracker = ConsentTracker::new();
//!
//! // Grant consent
//! let consent_id = tracker.grant_consent(
//!     "user@example.com",
//!     Purpose::Marketing,
//! ).unwrap();
//!
//! // Check consent
//! assert!(tracker.check_consent("user@example.com", Purpose::Marketing));
//!
//! // Withdraw consent
//! tracker.withdraw_consent(consent_id).unwrap();
//! assert!(!tracker.check_consent("user@example.com", Purpose::Marketing));
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::purpose::Purpose;

#[derive(Debug, Error)]
pub enum ConsentError {
    #[error("Consent not found: {0}")]
    ConsentNotFound(Uuid),

    #[error("Consent already withdrawn")]
    AlreadyWithdrawn,

    #[error("Invalid subject identifier: {0}")]
    InvalidSubject(String),

    #[error("Consent expired")]
    Expired,
}

pub type Result<T> = std::result::Result<T, ConsentError>;

/// Scope of consent (what data is covered)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsentScope {
    /// All personal data
    AllData,
    /// Only contact information (email, phone)
    ContactInfo,
    /// Only usage analytics (anonymized)
    AnalyticsOnly,
    /// Only necessary for contract performance
    ContractualNecessity,
}

/// GDPR Article 6(1) lawful basis for processing personal data.
///
/// Threaded onto `ConsentRecord` from wire protocol v4 (v0.6.0) so
/// regulated callers (clinical ops, financial compliance) can record
/// the paragraph letter alongside a free-form justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GdprArticle {
    /// Article 6(1)(a) — the data subject has given consent.
    Consent,
    /// Article 6(1)(b) — necessary for performance of a contract.
    Contract,
    /// Article 6(1)(c) — compliance with a legal obligation.
    LegalObligation,
    /// Article 6(1)(d) — to protect vital interests.
    VitalInterests,
    /// Article 6(1)(e) — task carried out in the public interest.
    PublicTask,
    /// Article 6(1)(f) — legitimate interests pursued by the controller.
    LegitimateInterests,
}

/// GDPR Article 6(1) lawful basis + caller-supplied justification.
/// Added in wire protocol v4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentBasis {
    /// The GDPR Article 6(1)(a)–(f) lettered basis.
    pub article: GdprArticle,
    /// Free-form justification captured at grant time.
    pub justification: Option<String>,
}

/// A single consent record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    /// Unique consent identifier
    pub consent_id: Uuid,
    /// Data subject identifier (email, user ID, etc.)
    pub subject_id: String,
    /// Purpose of processing
    pub purpose: Purpose,
    /// When consent was granted
    pub granted_at: DateTime<Utc>,
    /// When consent was withdrawn (if applicable)
    pub withdrawn_at: Option<DateTime<Utc>>,
    /// Scope of consent
    pub scope: ConsentScope,
    /// Optional expiry date
    pub expires_at: Option<DateTime<Utc>>,
    /// Free-form notes (e.g., how consent was obtained)
    pub notes: Option<String>,
    /// GDPR Article 6(1) lawful basis + justification. Populated
    /// when the grant call supplied a basis; `None` on pre-v4
    /// records. Wire protocol v4 (v0.6.0).
    #[serde(default)]
    pub basis: Option<ConsentBasis>,
    /// Terms-of-service version the subject responded to (e.g.
    /// `"v3"` or `"2026-04-tos"`). `None` on pre-v0.6.2 records and
    /// when the caller did not supply a value. Added in v0.6.2 to
    /// support consent capture flows that need to pin which terms
    /// version a subject saw at grant time.
    #[serde(default)]
    pub terms_version: Option<String>,
    /// Whether the subject accepted (`true`, default) or explicitly
    /// declined (`false`) at grant time. A declined record is still
    /// a compliance event — the audit trail captures that the
    /// subject was asked and said no, against `terms_version`.
    /// Pre-v0.6.2 records deserialize as `true` because consent
    /// granting was acceptance-only. Added in v0.6.2.
    #[serde(default = "default_accepted")]
    pub accepted: bool,
}

fn default_accepted() -> bool {
    true
}

impl ConsentRecord {
    /// Create a new consent record
    pub fn new(subject_id: String, purpose: Purpose, scope: ConsentScope) -> Self {
        Self {
            consent_id: Uuid::new_v4(),
            subject_id,
            purpose,
            granted_at: Utc::now(),
            withdrawn_at: None,
            scope,
            expires_at: None,
            notes: None,
            basis: None,
            terms_version: None,
            accepted: true,
        }
    }

    /// Attach a GDPR Article 6(1) lawful basis + justification.
    /// Builder-style; callers typically chain this after `new`.
    pub fn with_basis(mut self, basis: ConsentBasis) -> Self {
        self.basis = Some(basis);
        self
    }

    /// Attach the terms-of-service version the subject responded to.
    /// Builder-style; chain after `new`.
    pub fn with_terms_version(mut self, terms_version: impl Into<String>) -> Self {
        self.terms_version = Some(terms_version.into());
        self
    }

    /// Record an explicit acceptance state. Default for `new` is
    /// `accepted = true`; callers capturing a decline pass `false`.
    pub fn with_accepted(mut self, accepted: bool) -> Self {
        self.accepted = accepted;
        self
    }

    /// Check if consent is currently valid
    pub fn is_valid(&self) -> bool {
        // Withdrawn consent is invalid
        if self.withdrawn_at.is_some() {
            return false;
        }

        // Check expiry
        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }

        true
    }

    /// Check if consent has been withdrawn
    pub fn is_withdrawn(&self) -> bool {
        self.withdrawn_at.is_some()
    }

    /// Check if consent has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Withdraw this consent
    pub fn withdraw(&mut self) -> Result<()> {
        if self.withdrawn_at.is_some() {
            return Err(ConsentError::AlreadyWithdrawn);
        }

        self.withdrawn_at = Some(Utc::now());
        Ok(())
    }

    /// Set expiry date
    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Add notes
    pub fn with_notes(mut self, notes: String) -> Self {
        self.notes = Some(notes);
        self
    }
}

/// Options bundle for [`ConsentTracker::grant_consent_with_options`].
///
/// Added in v0.6.2 to keep the grant call site stable as the
/// optional-fields surface grows. Existing pre-v0.6.2 entry points
/// (`grant_consent`, `grant_consent_with_scope`,
/// `grant_consent_with_basis`) delegate here with field defaults
/// matching their pre-v0.6.2 semantics — no source-level breakage.
///
/// The custom [`Default`] impl pins `accepted = true` so omitting
/// fields preserves the v0.6.1 acceptance-only behaviour.
#[derive(Debug, Clone)]
pub struct GrantOptions {
    /// Scope of the grant. Defaults to [`ConsentScope::AllData`].
    pub scope: ConsentScope,
    /// GDPR Article 6(1) lawful basis + justification. Defaults to `None`.
    pub basis: Option<ConsentBasis>,
    /// Terms-of-service version the subject responded to. Defaults to `None`.
    pub terms_version: Option<String>,
    /// Whether the subject accepted (`true`, default) or declined (`false`).
    pub accepted: bool,
}

impl Default for GrantOptions {
    fn default() -> Self {
        Self {
            scope: ConsentScope::AllData,
            basis: None,
            terms_version: None,
            accepted: true,
        }
    }
}

/// Consent tracker manages all consent records
#[derive(Debug, Default)]
pub struct ConsentTracker {
    /// All consent records indexed by `consent_id`
    consents: HashMap<Uuid, ConsentRecord>,
    /// Index: `subject_id` -> `Vec<consent_id>`
    subject_index: HashMap<String, Vec<Uuid>>,
}

impl ConsentTracker {
    /// Create a new empty consent tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant consent for a data subject
    pub fn grant_consent(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
    ) -> Result<Uuid> {
        self.grant_consent_with_scope(subject_id, purpose, ConsentScope::AllData)
    }

    /// Grant consent with specific scope
    pub fn grant_consent_with_scope(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
        scope: ConsentScope,
    ) -> Result<Uuid> {
        self.grant_consent_with_basis(subject_id, purpose, scope, None)
    }

    /// Grant consent with scope + GDPR Article 6(1) lawful basis.
    ///
    /// Added in wire protocol v4 (v0.6.0). `basis = None` preserves
    /// pre-v4 grant semantics; `basis = Some(...)` captures the
    /// paragraph letter + justification on the resulting
    /// [`ConsentRecord`].
    ///
    /// As of v0.6.2 this delegates to
    /// [`Self::grant_consent_with_options`] with default `terms_version`
    /// (`None`) and `accepted` (`true`). Callers needing the new fields
    /// should switch to `grant_consent_with_options` directly.
    pub fn grant_consent_with_basis(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
        scope: ConsentScope,
        basis: Option<ConsentBasis>,
    ) -> Result<Uuid> {
        self.grant_consent_with_options(
            subject_id,
            purpose,
            GrantOptions {
                scope,
                basis,
                ..GrantOptions::default()
            },
        )
    }

    /// Grant consent with the full v0.6.2 options surface.
    ///
    /// `GrantOptions` carries scope, GDPR Article 6(1) basis, the
    /// terms-of-service version the subject responded to, and an
    /// explicit `accepted` flag (default `true`). All four fields
    /// flow into the resulting [`ConsentRecord`], which is the
    /// audit trail for both acceptances and declines.
    ///
    /// Pre-v0.6.2 entry points (`grant_consent`,
    /// `grant_consent_with_scope`, `grant_consent_with_basis`)
    /// delegate here with the v0.6.1-equivalent defaults — adding
    /// the new fields is fully backwards-compatible at the source
    /// level.
    pub fn grant_consent_with_options(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
        options: GrantOptions,
    ) -> Result<Uuid> {
        let subject_id = subject_id.into();

        // Validate subject_id
        if subject_id.is_empty() {
            return Err(ConsentError::InvalidSubject(subject_id));
        }

        let mut record = ConsentRecord::new(subject_id.clone(), purpose, options.scope);
        record.basis = options.basis;
        record.terms_version = options.terms_version;
        record.accepted = options.accepted;
        let consent_id = record.consent_id;

        // ALWAYS: granted_at timestamp must not be in the future.
        kimberlite_properties::always!(
            record.granted_at <= Utc::now(),
            "compliance.consent.granted_at_not_future",
            "consent granted_at must not be in the future"
        );

        // Insert consent record
        self.consents.insert(consent_id, record);

        // Update subject index
        self.subject_index
            .entry(subject_id)
            .or_default()
            .push(consent_id);

        Ok(consent_id)
    }

    /// Withdraw consent by `consent_id`
    pub fn withdraw_consent(&mut self, consent_id: Uuid) -> Result<()> {
        let record = self
            .consents
            .get_mut(&consent_id)
            .ok_or(ConsentError::ConsentNotFound(consent_id))?;

        record.withdraw()
    }

    /// Check if subject has valid consent for a purpose
    pub fn check_consent(&self, subject_id: &str, purpose: Purpose) -> bool {
        self.check_consent_with_scope(subject_id, purpose, ConsentScope::AllData)
    }

    /// Check consent with specific scope
    pub fn check_consent_with_scope(
        &self,
        subject_id: &str,
        purpose: Purpose,
        scope: ConsentScope,
    ) -> bool {
        // Get all consents for this subject
        let Some(consent_ids) = self.subject_index.get(subject_id) else {
            return false;
        };

        // Check if any consent matches purpose and scope
        consent_ids.iter().any(|id| {
            if let Some(record) = self.consents.get(id) {
                // `matches` is bound so the NEVER predicate below references the same
                // computed value without re-evaluating; `let_and_return` is a false
                // positive here because the binding is read twice.
                #[allow(clippy::let_and_return)]
                let matches =
                    record.is_valid() && record.purpose == purpose && record.scope == scope;

                // NEVER: a withdrawn consent must never validate as true.
                kimberlite_properties::never!(
                    matches && record.is_withdrawn(),
                    "compliance.consent.withdrawn_never_valid",
                    "withdrawn consent must never satisfy a validation check"
                );

                matches
            } else {
                false
            }
        })
    }

    /// Get all consent records for a subject
    pub fn get_consents_for_subject(&self, subject_id: &str) -> Vec<&ConsentRecord> {
        let Some(consent_ids) = self.subject_index.get(subject_id) else {
            return Vec::new();
        };

        consent_ids
            .iter()
            .filter_map(|id| self.consents.get(id))
            .collect()
    }

    /// Get all valid consents for a subject
    pub fn get_valid_consents(&self, subject_id: &str) -> Vec<&ConsentRecord> {
        self.get_consents_for_subject(subject_id)
            .into_iter()
            .filter(|record| record.is_valid())
            .collect()
    }

    /// Get consent record by ID
    pub fn get_consent(&self, consent_id: Uuid) -> Option<&ConsentRecord> {
        self.consents.get(&consent_id)
    }

    /// Count total consents
    pub fn total_consents(&self) -> usize {
        self.consents.len()
    }

    /// Count valid consents
    pub fn valid_consents(&self) -> usize {
        self.consents.values().filter(|r| r.is_valid()).count()
    }

    /// Count withdrawn consents
    pub fn withdrawn_consents(&self) -> usize {
        self.consents.values().filter(|r| r.is_withdrawn()).count()
    }

    /// Expire old consents (housekeeping)
    pub fn expire_old_consents(&mut self) -> usize {
        let now = Utc::now();
        let mut expired_count = 0;

        for record in self.consents.values_mut() {
            if let Some(expires_at) = record.expires_at {
                if now > expires_at && record.withdrawn_at.is_none() {
                    record.withdrawn_at = Some(now);
                    expired_count += 1;
                }
            }
        }

        expired_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consent_grant() {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        assert!(tracker.check_consent("user@example.com", Purpose::Marketing));

        let record = tracker.get_consent(consent_id).unwrap();
        assert_eq!(record.subject_id, "user@example.com");
        assert_eq!(record.purpose, Purpose::Marketing);
        assert!(record.is_valid());
    }

    #[test]
    fn test_consent_withdrawal() {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent("user@example.com", Purpose::Analytics)
            .unwrap();

        assert!(tracker.check_consent("user@example.com", Purpose::Analytics));

        tracker.withdraw_consent(consent_id).unwrap();
        assert!(!tracker.check_consent("user@example.com", Purpose::Analytics));

        let record = tracker.get_consent(consent_id).unwrap();
        assert!(record.is_withdrawn());
        assert!(!record.is_valid());
    }

    #[test]
    fn test_double_withdrawal() {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        tracker.withdraw_consent(consent_id).unwrap();
        let result = tracker.withdraw_consent(consent_id);
        assert!(matches!(result, Err(ConsentError::AlreadyWithdrawn)));
    }

    #[test]
    fn test_purpose_separation() {
        let mut tracker = ConsentTracker::new();
        tracker
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        assert!(tracker.check_consent("user@example.com", Purpose::Marketing));
        assert!(!tracker.check_consent("user@example.com", Purpose::Analytics));
    }

    #[test]
    fn test_multiple_consents_same_subject() {
        let mut tracker = ConsentTracker::new();
        tracker
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();
        tracker
            .grant_consent("user@example.com", Purpose::Analytics)
            .unwrap();

        let consents = tracker.get_consents_for_subject("user@example.com");
        assert_eq!(consents.len(), 2);
    }

    #[test]
    fn test_consent_expiry() {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        // Set expiry to 1 second ago
        let record = tracker.consents.get_mut(&consent_id).unwrap();
        record.expires_at = Some(Utc::now() - chrono::Duration::seconds(1));

        assert!(!tracker.check_consent("user@example.com", Purpose::Marketing));
        let record = tracker.get_consent(consent_id).unwrap();
        assert!(record.is_expired());
    }

    #[test]
    fn test_consent_scope() {
        let mut tracker = ConsentTracker::new();
        tracker
            .grant_consent_with_scope(
                "user@example.com",
                Purpose::Marketing,
                ConsentScope::ContactInfo,
            )
            .unwrap();

        // ContactInfo scope granted
        assert!(tracker.check_consent_with_scope(
            "user@example.com",
            Purpose::Marketing,
            ConsentScope::ContactInfo
        ));

        // AllData scope NOT granted
        assert!(!tracker.check_consent_with_scope(
            "user@example.com",
            Purpose::Marketing,
            ConsentScope::AllData
        ));
    }

    #[test]
    fn test_invalid_subject() {
        let mut tracker = ConsentTracker::new();
        let result = tracker.grant_consent("", Purpose::Marketing);
        assert!(matches!(result, Err(ConsentError::InvalidSubject(_))));
    }

    #[test]
    fn test_consent_not_found() {
        let mut tracker = ConsentTracker::new();
        let fake_id = Uuid::new_v4();
        let result = tracker.withdraw_consent(fake_id);
        assert!(matches!(result, Err(ConsentError::ConsentNotFound(_))));
    }

    #[test]
    fn test_tracker_statistics() {
        let mut tracker = ConsentTracker::new();
        let id1 = tracker
            .grant_consent("user1@example.com", Purpose::Marketing)
            .unwrap();
        tracker
            .grant_consent("user2@example.com", Purpose::Analytics)
            .unwrap();

        assert_eq!(tracker.total_consents(), 2);
        assert_eq!(tracker.valid_consents(), 2);
        assert_eq!(tracker.withdrawn_consents(), 0);

        tracker.withdraw_consent(id1).unwrap();

        assert_eq!(tracker.total_consents(), 2);
        assert_eq!(tracker.valid_consents(), 1);
        assert_eq!(tracker.withdrawn_consents(), 1);
    }

    // ========================================================================
    // v0.6.2 — terms_version + accepted
    // ========================================================================

    #[test]
    fn grant_options_default_preserves_v061_semantics() {
        // Sanity: omitting all v0.6.2 options matches the pre-v0.6.2
        // grant shape (AllData scope, no basis, no terms version,
        // accepted = true).
        let opts = GrantOptions::default();
        assert!(matches!(opts.scope, ConsentScope::AllData));
        assert!(opts.basis.is_none());
        assert!(opts.terms_version.is_none());
        assert!(opts.accepted);
    }

    #[test]
    fn grant_consent_with_options_threads_terms_version_and_accepted() {
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent_with_options(
                "alice@example.com",
                Purpose::Marketing,
                GrantOptions {
                    terms_version: Some("2026-04-tos".into()),
                    accepted: true,
                    ..GrantOptions::default()
                },
            )
            .unwrap();

        let record = tracker.get_consent(consent_id).unwrap();
        assert_eq!(record.terms_version.as_deref(), Some("2026-04-tos"));
        assert!(record.accepted);
    }

    #[test]
    fn grant_consent_with_options_records_explicit_decline() {
        // The whole point: capturing that the subject was asked,
        // saw terms version v3, and said no.
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent_with_options(
                "bob@example.com",
                Purpose::Analytics,
                GrantOptions {
                    terms_version: Some("v3".into()),
                    accepted: false,
                    ..GrantOptions::default()
                },
            )
            .unwrap();

        let record = tracker.get_consent(consent_id).unwrap();
        assert!(!record.accepted);
        assert_eq!(record.terms_version.as_deref(), Some("v3"));
    }

    #[test]
    fn pre_v062_grant_with_basis_path_defaults_new_fields() {
        // The legacy `grant_consent_with_basis` entry point still
        // works and must produce a record with the v0.6.2 fields at
        // their pre-v0.6.2 defaults (None / true).
        let mut tracker = ConsentTracker::new();
        let consent_id = tracker
            .grant_consent_with_basis(
                "carol@example.com",
                Purpose::Research,
                ConsentScope::AllData,
                None,
            )
            .unwrap();

        let record = tracker.get_consent(consent_id).unwrap();
        assert!(record.terms_version.is_none());
        assert!(record.accepted);
    }

    #[test]
    fn record_serde_default_for_accepted_is_true_on_pre_v062_payloads() {
        // Simulate decoding a v0.6.1-shaped record (which has no
        // `accepted` field) into the v0.6.2 struct via JSON. JSON's
        // `#[serde(default)]` semantics fire for missing fields,
        // unlike postcard — so this test pins the JSON-side behaviour
        // independently. (Wire-format compat is gated at the frame
        // validator; see `kimberlite-wire::tests::v3_v4_compat`.)
        let v061_json = serde_json::json!({
            "consent_id": "00000000-0000-0000-0000-000000000003",
            "subject_id": "frank@example.com",
            "purpose": "Marketing",
            "granted_at": "2026-04-01T00:00:00Z",
            "withdrawn_at": null,
            "scope": "AllData",
            "expires_at": null,
            "notes": null,
        });
        let rec: ConsentRecord = serde_json::from_value(v061_json).unwrap();
        assert!(rec.terms_version.is_none());
        // The serde default (`true`) makes pre-v0.6.2 records
        // implicitly acceptances — preserves v0.6.1 semantics.
        assert!(rec.accepted);
    }
}
