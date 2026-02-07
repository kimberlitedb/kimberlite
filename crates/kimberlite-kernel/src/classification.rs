//! Data classification inference from metadata and content.
//!
//! This module provides heuristics to automatically classify data based on
//! metadata hints, stream names, and content patterns.

use kimberlite_types::DataClass;

/// Infers data classification from stream metadata.
///
/// Uses heuristics based on stream name, metadata tags, and content hints.
///
/// # Examples
///
/// ```
/// use kimberlite_kernel::classification::infer_from_stream_name;
/// use kimberlite_types::DataClass;
///
/// assert_eq!(infer_from_stream_name("patient_records"), DataClass::PHI);
/// assert_eq!(infer_from_stream_name("credit_card_transactions"), DataClass::PCI);
/// assert_eq!(infer_from_stream_name("public_announcements"), DataClass::Public);
/// ```
pub fn infer_from_stream_name(stream_name: &str) -> DataClass {
    let lower = stream_name.to_lowercase();

    // Deidentified healthcare data (check first - most specific)
    if lower.contains("deidentified") || lower.contains("anonymized") || lower.contains("aggregate")
    {
        return DataClass::Deidentified;
    }

    // Healthcare (HIPAA) patterns
    if lower.contains("patient")
        || lower.contains("medical")
        || lower.contains("health")
        || lower.contains("diagnosis")
        || lower.contains("prescription")
        || lower.contains("lab_result")
        || lower.contains("clinical")
    {
        return DataClass::PHI;
    }

    // Payment Card Industry (PCI DSS) patterns
    if lower.contains("credit_card")
        || lower.contains("payment")
        || lower.contains("card_transaction")
        || lower.contains("cvv")
        || lower.contains("pan")
    // Primary Account Number
    {
        return DataClass::PCI;
    }

    // Financial (SOX) patterns
    if lower.contains("financial")
        || lower.contains("ledger")
        || lower.contains("accounting")
        || lower.contains("audit_trail")
        || lower.contains("balance_sheet")
        || lower.contains("revenue")
    {
        return DataClass::Financial;
    }

    // GDPR Sensitive (Article 9) patterns
    if lower.contains("biometric")
        || lower.contains("genetic")
        || lower.contains("racial")
        || lower.contains("ethnic")
        || lower.contains("political")
        || lower.contains("religious")
        || lower.contains("sexual_orientation")
        || lower.contains("trade_union")
    {
        return DataClass::Sensitive;
    }

    // GDPR PII patterns
    if lower.contains("user")
        || lower.contains("customer")
        || lower.contains("person")
        || lower.contains("profile")
        || lower.contains("contact")
        || lower.contains("email")
        || lower.contains("phone")
        || lower.contains("address")
    {
        return DataClass::PII;
    }

    // Confidential business data
    if lower.contains("internal")
        || lower.contains("confidential")
        || lower.contains("proprietary")
        || lower.contains("trade_secret")
        || lower.contains("strategy")
    {
        return DataClass::Confidential;
    }

    // Public data
    if lower.contains("public")
        || lower.contains("announcement")
        || lower.contains("press_release")
        || lower.contains("blog")
        || lower.contains("documentation")
    {
        return DataClass::Public;
    }

    // Default: Confidential (safe default - better to be restrictive)
    DataClass::Confidential
}

/// Validates that a user-provided classification is reasonable.
///
/// Returns `true` if the classification matches the inferred classification
/// or is more restrictive.
///
/// # Examples
///
/// ```
/// use kimberlite_kernel::classification::validate_user_classification;
/// use kimberlite_types::DataClass;
///
/// // User can classify as more restrictive
/// assert!(validate_user_classification("patient_records", DataClass::PHI));
///
/// // User cannot classify PHI as Public (too permissive)
/// assert!(!validate_user_classification("patient_records", DataClass::Public));
/// ```
pub fn validate_user_classification(stream_name: &str, user_classification: DataClass) -> bool {
    let inferred = infer_from_stream_name(stream_name);

    // Allow user to be more restrictive than inference
    // (e.g., classify Public as Confidential)
    //
    // Deny user being less restrictive than inference
    // (e.g., classify PHI as Public)
    //
    // Ordering (least to most restrictive):
    // Public < Deidentified < Confidential < PII < Financial < PCI < Sensitive < PHI

    let restrictiveness = |dc: DataClass| -> u8 {
        match dc {
            DataClass::Public => 0,
            DataClass::Deidentified => 1,
            DataClass::Confidential => 2,
            DataClass::PII => 3,
            DataClass::Financial => 4,
            DataClass::PCI => 5,
            DataClass::Sensitive => 6,
            DataClass::PHI => 7,
        }
    };

    restrictiveness(user_classification) >= restrictiveness(inferred)
}

/// Suggests appropriate data classification based on content patterns.
///
/// This is a placeholder for future ML-based classification.
/// Currently returns the inference from stream name.
pub fn suggest_classification(stream_name: &str, _content_sample: &[u8]) -> DataClass {
    // TODO(v0.9.0): Implement ML-based content classification
    // For now, just use stream name inference
    infer_from_stream_name(stream_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_phi() {
        assert_eq!(infer_from_stream_name("patient_records"), DataClass::PHI);
        assert_eq!(infer_from_stream_name("medical_history"), DataClass::PHI);
        assert_eq!(infer_from_stream_name("lab_results"), DataClass::PHI);
    }

    #[test]
    fn test_infer_deidentified() {
        assert_eq!(
            infer_from_stream_name("deidentified_cohort_study"),
            DataClass::Deidentified
        );
        assert_eq!(
            infer_from_stream_name("anonymized_patient_data"),
            DataClass::Deidentified
        );
    }

    #[test]
    fn test_infer_pci() {
        assert_eq!(
            infer_from_stream_name("credit_card_transactions"),
            DataClass::PCI
        );
        assert_eq!(
            infer_from_stream_name("payment_gateway_logs"),
            DataClass::PCI
        );
    }

    #[test]
    fn test_infer_financial() {
        assert_eq!(
            infer_from_stream_name("general_ledger"),
            DataClass::Financial
        );
        assert_eq!(
            infer_from_stream_name("revenue_reports"),
            DataClass::Financial
        );
    }

    #[test]
    fn test_infer_sensitive() {
        assert_eq!(
            infer_from_stream_name("biometric_access_logs"),
            DataClass::Sensitive
        );
        assert_eq!(
            infer_from_stream_name("genetic_test_results"),
            DataClass::Sensitive
        );
    }

    #[test]
    fn test_infer_pii() {
        assert_eq!(infer_from_stream_name("user_profiles"), DataClass::PII);
        assert_eq!(infer_from_stream_name("customer_contacts"), DataClass::PII);
    }

    #[test]
    fn test_infer_confidential() {
        assert_eq!(
            infer_from_stream_name("internal_strategy_docs"),
            DataClass::Confidential
        );
        assert_eq!(
            infer_from_stream_name("proprietary_algorithms"),
            DataClass::Confidential
        );
    }

    #[test]
    fn test_infer_public() {
        assert_eq!(
            infer_from_stream_name("public_announcements"),
            DataClass::Public
        );
        assert_eq!(infer_from_stream_name("blog_posts"), DataClass::Public);
    }

    #[test]
    fn test_validate_user_classification_allowed() {
        // User can be more restrictive
        assert!(validate_user_classification(
            "blog_posts",
            DataClass::Confidential
        ));

        // User matches inference
        assert!(validate_user_classification(
            "patient_records",
            DataClass::PHI
        ));
    }

    #[test]
    fn test_validate_user_classification_denied() {
        // User cannot be less restrictive
        assert!(!validate_user_classification(
            "patient_records",
            DataClass::Public
        ));

        assert!(!validate_user_classification(
            "credit_card_transactions",
            DataClass::Confidential
        ));
    }

    #[test]
    fn test_default_classification() {
        // Unknown patterns should default to Confidential (safe default)
        assert_eq!(
            infer_from_stream_name("unknown_stream_xyz"),
            DataClass::Confidential
        );
    }
}
