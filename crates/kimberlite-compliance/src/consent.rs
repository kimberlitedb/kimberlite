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
        }
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
        let subject_id = subject_id.into();

        // Validate subject_id
        if subject_id.is_empty() {
            return Err(ConsentError::InvalidSubject(subject_id));
        }

        let record = ConsentRecord::new(subject_id.clone(), purpose, scope);
        let consent_id = record.consent_id;

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
                record.is_valid() && record.purpose == purpose && record.scope == scope
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
}
