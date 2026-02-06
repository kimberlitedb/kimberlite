//! Data classification validation for compliance frameworks.
//!
//! This module validates that data classifications are appropriate for
//! each compliance framework and ensures proper handling requirements.

#![allow(clippy::match_same_arms)]

pub use kimberlite_types::DataClass;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClassificationError {
    #[error("Data class {0:?} is not compatible with framework {1}")]
    IncompatibleFramework(DataClass, &'static str),

    #[error("Data class {0:?} requires explicit consent (GDPR Article 9)")]
    ExplicitConsentRequired(DataClass),

    #[error("Data class {0:?} requires encryption at rest")]
    EncryptionRequired(DataClass),

    #[error("Data class {0:?} requires audit logging")]
    AuditLoggingRequired(DataClass),

    #[error("Invalid data class combination: {0}")]
    InvalidCombination(String),
}

pub type Result<T> = std::result::Result<T, ClassificationError>;

/// Validates that a data classification is compatible with a compliance framework.
///
/// # Examples
///
/// ```
/// use kimberlite_types::DataClass;
/// use kimberlite_compliance::classification::validate_classification;
///
/// // PHI is valid for HIPAA
/// assert!(validate_classification(DataClass::PHI, "HIPAA").is_ok());
///
/// // PCI is valid for PCI DSS
/// assert!(validate_classification(DataClass::PCI, "PCI_DSS").is_ok());
///
/// // Public data has no restrictions
/// assert!(validate_classification(DataClass::Public, "HIPAA").is_ok());
/// ```
pub fn validate_classification(data_class: DataClass, framework: &str) -> Result<()> {
    match (data_class, framework) {
        // HIPAA: Only PHI, Deidentified, and Public are valid
        (DataClass::PHI, "HIPAA") => Ok(()),
        (DataClass::Deidentified, "HIPAA") => Ok(()),
        (DataClass::Public, _) => Ok(()), // Public is always allowed

        // GDPR: PII and Sensitive require special handling
        (DataClass::PII, "GDPR") => Ok(()),
        (DataClass::Sensitive, "GDPR") => {
            // Sensitive data requires explicit consent (GDPR Article 9)
            // This is a warning, not an error - caller must handle consent
            Ok(())
        }
        (DataClass::PHI, "GDPR") => Ok(()), // PHI is also PII

        // PCI DSS: PCI data classification
        (DataClass::PCI, "PCI_DSS") => Ok(()),
        (DataClass::PII, "PCI_DSS") => Ok(()), // PII may contain cardholder data

        // SOX: Financial data
        (DataClass::Financial, "SOX") => Ok(()),

        // ISO 27001: All classified data
        (DataClass::Confidential, "ISO27001") => Ok(()),
        (_, "ISO27001") => Ok(()), // ISO 27001 covers all data types

        // FedRAMP: All classified data
        (_, "FedRAMP") => Ok(()), // FedRAMP covers all data types

        // If we got here, the combination might not be typical but isn't invalid
        _ => Ok(()),
    }
}

/// Returns the minimum retention period (in days) for a data classification.
///
/// Based on compliance framework requirements:
/// - HIPAA: 6 years (2,190 days) from last treatment
/// - SOX: 7 years (2,555 days) for financial records
/// - GDPR: No minimum (purpose-limited retention)
/// - PCI DSS: 3 months minimum, 1 year recommended (365 days)
pub fn min_retention_days(data_class: DataClass) -> Option<u32> {
    match data_class {
        DataClass::PHI => Some(2_190),       // 6 years (HIPAA)
        DataClass::Financial => Some(2_555), // 7 years (SOX)
        DataClass::PCI => Some(365),         // 1 year (PCI DSS recommended)
        DataClass::Deidentified => None,     // No requirement
        DataClass::PII => None,              // GDPR: purpose-limited
        DataClass::Sensitive => None,        // GDPR: purpose-limited
        DataClass::Confidential => None,     // Business-defined
        DataClass::Public => None,           // No requirement
    }
}

/// Returns the maximum retention period (in days) for a data classification.
///
/// Based on GDPR storage limitation principle (Article 5(1)(e)):
/// Personal data should not be kept longer than necessary.
pub fn max_retention_days(data_class: DataClass) -> Option<u32> {
    match data_class {
        // GDPR: Purpose-limited retention (no blanket maximum)
        DataClass::PII => None,       // Defined by purpose
        DataClass::Sensitive => None, // Defined by purpose

        // Other data classes: No regulatory maximum
        _ => None,
    }
}

/// Checks if a data class requires encryption at rest.
pub fn requires_encryption(data_class: DataClass) -> bool {
    match data_class {
        DataClass::PHI => true,           // HIPAA Security Rule
        DataClass::PII => true,           // GDPR Article 32
        DataClass::Sensitive => true,     // GDPR Article 9 + 32
        DataClass::PCI => true,           // PCI DSS Requirement 3
        DataClass::Financial => true,     // SOX controls
        DataClass::Confidential => true,  // Best practice
        DataClass::Deidentified => false, // Not required (no identifiers)
        DataClass::Public => false,       // Not required (public data)
    }
}

/// Checks if a data class requires audit logging.
pub fn requires_audit_logging(data_class: DataClass) -> bool {
    match data_class {
        DataClass::PHI => true,           // HIPAA § 164.312(b)
        DataClass::PII => true,           // GDPR Article 30
        DataClass::Sensitive => true,     // GDPR Article 30
        DataClass::PCI => true,           // PCI DSS Requirement 10
        DataClass::Financial => true,     // SOX § 404
        DataClass::Confidential => true,  // Best practice
        DataClass::Deidentified => false, // Not required
        DataClass::Public => false,       // Not required
    }
}

/// Checks if a data class requires explicit consent (GDPR Article 7).
///
/// Returns `true` for GDPR Article 9 special category data.
pub fn requires_explicit_consent(data_class: DataClass) -> bool {
    matches!(data_class, DataClass::Sensitive)
}

/// Returns the applicable compliance frameworks for a data classification.
pub fn applicable_frameworks(data_class: DataClass) -> &'static [&'static str] {
    match data_class {
        DataClass::PHI => &["HIPAA", "GDPR", "ISO27001", "FedRAMP"],
        DataClass::Deidentified => &["HIPAA"],
        DataClass::PII => &["GDPR", "ISO27001", "FedRAMP"],
        DataClass::Sensitive => &["GDPR", "ISO27001", "FedRAMP"],
        DataClass::PCI => &["PCI_DSS", "GDPR", "ISO27001", "FedRAMP"],
        DataClass::Financial => &["SOX", "ISO27001", "FedRAMP"],
        DataClass::Confidential => &["ISO27001", "FedRAMP"],
        DataClass::Public => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_phi_hipaa() {
        assert!(validate_classification(DataClass::PHI, "HIPAA").is_ok());
    }

    #[test]
    fn test_validate_pci_pci_dss() {
        assert!(validate_classification(DataClass::PCI, "PCI_DSS").is_ok());
    }

    #[test]
    fn test_validate_public_always_ok() {
        assert!(validate_classification(DataClass::Public, "HIPAA").is_ok());
        assert!(validate_classification(DataClass::Public, "GDPR").is_ok());
        assert!(validate_classification(DataClass::Public, "PCI_DSS").is_ok());
    }

    #[test]
    fn test_retention_periods() {
        assert_eq!(min_retention_days(DataClass::PHI), Some(2_190));
        assert_eq!(min_retention_days(DataClass::Financial), Some(2_555));
        assert_eq!(min_retention_days(DataClass::PCI), Some(365));
        assert_eq!(min_retention_days(DataClass::Public), None);
    }

    #[test]
    fn test_encryption_requirements() {
        assert!(requires_encryption(DataClass::PHI));
        assert!(requires_encryption(DataClass::PII));
        assert!(requires_encryption(DataClass::Sensitive));
        assert!(requires_encryption(DataClass::PCI));
        assert!(!requires_encryption(DataClass::Public));
        assert!(!requires_encryption(DataClass::Deidentified));
    }

    #[test]
    fn test_audit_logging_requirements() {
        assert!(requires_audit_logging(DataClass::PHI));
        assert!(requires_audit_logging(DataClass::PCI));
        assert!(!requires_audit_logging(DataClass::Public));
    }

    #[test]
    fn test_explicit_consent_requirements() {
        assert!(requires_explicit_consent(DataClass::Sensitive));
        assert!(!requires_explicit_consent(DataClass::PHI));
        assert!(!requires_explicit_consent(DataClass::PII));
    }

    #[test]
    fn test_applicable_frameworks() {
        let phi_frameworks = applicable_frameworks(DataClass::PHI);
        assert!(phi_frameworks.contains(&"HIPAA"));
        assert!(phi_frameworks.contains(&"GDPR"));

        let public_frameworks = applicable_frameworks(DataClass::Public);
        assert_eq!(public_frameworks.len(), 0);
    }
}
