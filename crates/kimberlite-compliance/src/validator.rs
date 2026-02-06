//! Consent and Purpose Validation
//!
//! This module provides high-level validators for checking consent before
//! processing personal data. Integrates with both consent tracking and
//! purpose limitation.
//!
//! # Architecture
//!
//! ```text
//! API Request → validate_query() → Check:
//!   1. Does user have valid consent for this purpose?
//!   2. Is purpose valid for the data classification?
//!   3. Is data minimization principle satisfied?
//! ```
//!
//! # Example
//!
//! ```
//! use kimberlite_compliance::validator::ConsentValidator;
//! use kimberlite_compliance::purpose::Purpose;
//! use kimberlite_compliance::classification::DataClass;
//!
//! let mut validator = ConsentValidator::new();
//!
//! // Grant consent
//! validator.grant_consent("user@example.com", Purpose::Analytics).unwrap();
//!
//! // Validate query
//! let result = validator.validate_query(
//!     "user@example.com",
//!     Purpose::Analytics,
//!     DataClass::PII,
//! );
//! assert!(result.is_ok());
//! ```

use crate::classification::DataClass;
use crate::consent::{ConsentScope, ConsentTracker};
use crate::purpose::{self, Purpose, PurposeError};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Consent required but not granted: {0}")]
    ConsentNotGranted(String),

    #[error("Purpose validation failed: {0}")]
    PurposeInvalid(#[from] PurposeError),

    #[error("Consent error: {0}")]
    ConsentError(#[from] crate::consent::ConsentError),
}

pub type Result<T> = std::result::Result<T, ValidationError>;

/// Validator for consent and purpose checks
pub struct ConsentValidator {
    /// Underlying consent tracker
    tracker: ConsentTracker,
}

impl ConsentValidator {
    /// Create a new validator
    pub fn new() -> Self {
        Self {
            tracker: ConsentTracker::new(),
        }
    }

    /// Grant consent for a subject
    pub fn grant_consent(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
    ) -> crate::consent::Result<Uuid> {
        self.tracker.grant_consent(subject_id, purpose)
    }

    /// Grant consent with specific scope
    pub fn grant_consent_with_scope(
        &mut self,
        subject_id: impl Into<String>,
        purpose: Purpose,
        scope: ConsentScope,
    ) -> crate::consent::Result<Uuid> {
        self.tracker
            .grant_consent_with_scope(subject_id, purpose, scope)
    }

    /// Withdraw consent
    pub fn withdraw_consent(&mut self, consent_id: Uuid) -> crate::consent::Result<()> {
        self.tracker.withdraw_consent(consent_id)
    }

    /// Validate a query before execution
    ///
    /// Checks:
    /// 1. Purpose is valid for data class
    /// 2. Data minimization principle satisfied
    /// 3. If purpose requires consent, check consent is granted
    pub fn validate_query(
        &self,
        subject_id: &str,
        purpose: Purpose,
        data_class: DataClass,
    ) -> Result<()> {
        // Step 1: Validate purpose for data class
        purpose::validate_purpose(data_class, purpose)?;

        // Step 2: Check if purpose requires consent
        if purpose.requires_consent() && !self.tracker.check_consent(subject_id, purpose) {
            return Err(ValidationError::ConsentNotGranted(format!(
                "Subject '{subject_id}' has not granted consent for purpose '{purpose}'"
            )));
        }

        // Step 3: Passed all checks
        Ok(())
    }

    /// Validate a write operation
    ///
    /// Checks the same conditions as `validate_query` but for write operations.
    pub fn validate_write(
        &self,
        subject_id: &str,
        purpose: Purpose,
        data_class: DataClass,
    ) -> Result<()> {
        self.validate_query(subject_id, purpose, data_class)
    }

    /// Check if consent exists without full validation
    pub fn has_consent(&self, subject_id: &str, purpose: Purpose) -> bool {
        self.tracker.check_consent(subject_id, purpose)
    }

    /// Get statistics
    pub fn stats(&self) -> ValidatorStats {
        ValidatorStats {
            total_consents: self.tracker.total_consents(),
            valid_consents: self.tracker.valid_consents(),
            withdrawn_consents: self.tracker.withdrawn_consents(),
        }
    }

    /// Get reference to underlying tracker
    pub fn tracker(&self) -> &ConsentTracker {
        &self.tracker
    }
}

impl Default for ConsentValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about consent validator
#[derive(Debug, Clone)]
pub struct ValidatorStats {
    pub total_consents: usize,
    pub valid_consents: usize,
    pub withdrawn_consents: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_query_with_consent() {
        let mut validator = ConsentValidator::new();
        validator
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        let result =
            validator.validate_query("user@example.com", Purpose::Marketing, DataClass::PII);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_query_without_required_consent() {
        let validator = ConsentValidator::new();

        // Marketing requires consent
        let result =
            validator.validate_query("user@example.com", Purpose::Marketing, DataClass::PII);
        assert!(matches!(result, Err(ValidationError::ConsentNotGranted(_))));
    }

    #[test]
    fn test_validate_query_no_consent_required() {
        let validator = ConsentValidator::new();

        // Contractual doesn't require consent
        let result =
            validator.validate_query("user@example.com", Purpose::Contractual, DataClass::PII);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_query_invalid_purpose() {
        let mut validator = ConsentValidator::new();
        validator
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        // Marketing not allowed for PHI
        let result =
            validator.validate_query("user@example.com", Purpose::Marketing, DataClass::PHI);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_write() {
        let mut validator = ConsentValidator::new();
        validator
            .grant_consent("user@example.com", Purpose::Analytics)
            .unwrap();

        let result =
            validator.validate_write("user@example.com", Purpose::Analytics, DataClass::PII);
        assert!(result.is_ok());
    }

    #[test]
    fn test_has_consent() {
        let mut validator = ConsentValidator::new();
        validator
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        assert!(validator.has_consent("user@example.com", Purpose::Marketing));
        assert!(!validator.has_consent("user@example.com", Purpose::Analytics));
    }

    #[test]
    fn test_consent_withdrawal() {
        let mut validator = ConsentValidator::new();
        let consent_id = validator
            .grant_consent("user@example.com", Purpose::Marketing)
            .unwrap();

        // Initially valid
        let result =
            validator.validate_query("user@example.com", Purpose::Marketing, DataClass::PII);
        assert!(result.is_ok());

        // Withdraw consent
        validator.withdraw_consent(consent_id).unwrap();

        // Now invalid
        let result =
            validator.validate_query("user@example.com", Purpose::Marketing, DataClass::PII);
        assert!(matches!(result, Err(ValidationError::ConsentNotGranted(_))));
    }

    #[test]
    fn test_validator_stats() {
        let mut validator = ConsentValidator::new();
        let id1 = validator
            .grant_consent("user1@example.com", Purpose::Marketing)
            .unwrap();
        validator
            .grant_consent("user2@example.com", Purpose::Analytics)
            .unwrap();

        let stats = validator.stats();
        assert_eq!(stats.total_consents, 2);
        assert_eq!(stats.valid_consents, 2);

        validator.withdraw_consent(id1).unwrap();
        let stats = validator.stats();
        assert_eq!(stats.valid_consents, 1);
        assert_eq!(stats.withdrawn_consents, 1);
    }
}
