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

/// Content-based classification result with confidence scoring.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationResult {
    /// Inferred data class.
    pub data_class: DataClass,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Which patterns matched (for audit trail).
    pub matched_patterns: Vec<String>,
}

/// Regex-free pattern matching for sensitive data detection.
///
/// Uses lightweight heuristic scanning instead of full regex for performance.
/// Patterns are designed for high recall (prefer false positives over misses).
struct ContentScanner;

impl ContentScanner {
    /// Scans content for SSN patterns (XXX-XX-XXXX).
    fn scan_ssn(content: &str) -> bool {
        let bytes = content.as_bytes();
        // Look for XXX-XX-XXXX pattern (11 chars)
        if bytes.len() < 11 {
            return false;
        }
        for window in bytes.windows(11) {
            if window[0].is_ascii_digit()
                && window[1].is_ascii_digit()
                && window[2].is_ascii_digit()
                && window[3] == b'-'
                && window[4].is_ascii_digit()
                && window[5].is_ascii_digit()
                && window[6] == b'-'
                && window[7].is_ascii_digit()
                && window[8].is_ascii_digit()
                && window[9].is_ascii_digit()
                && window[10].is_ascii_digit()
            {
                return true;
            }
        }
        false
    }

    /// Scans content for credit card number patterns (16 consecutive digits or groups of 4).
    fn scan_credit_card(content: &str) -> bool {
        // Check for 16 consecutive digits
        let mut consecutive_digits = 0u32;
        for ch in content.chars() {
            if ch.is_ascii_digit() {
                consecutive_digits += 1;
                if consecutive_digits >= 13 {
                    return true; // Visa (13-16), MC (16), Amex (15)
                }
            } else if ch == ' ' || ch == '-' {
                // Allow separators between digit groups
            } else {
                consecutive_digits = 0;
            }
        }
        false
    }

    /// Scans content for email address patterns.
    fn scan_email(content: &str) -> bool {
        // Simple heuristic: word@word.word
        content.contains('@') && {
            for part in content.split_whitespace() {
                if let Some(at_pos) = part.find('@') {
                    let local = &part[..at_pos];
                    let domain = &part[at_pos + 1..];
                    if !local.is_empty() && domain.contains('.') && domain.len() > 2 {
                        return true;
                    }
                }
            }
            false
        }
    }

    /// Scans content for medical/health terminology.
    fn scan_medical_terms(content: &str) -> usize {
        let lower = content.to_lowercase();
        let terms = [
            "diagnosis", "icd-10", "icd-9", "procedure", "medication",
            "prescription", "dosage", "mg ", "ml ", "blood pressure",
            "heart rate", "bmi", "hemoglobin", "cholesterol", "glucose",
            "mrn", "medical record", "patient id", "dob", "date of birth",
            "insurance", "copay", "deductible", "hipaa",
        ];
        terms.iter().filter(|t| lower.contains(**t)).count()
    }

    /// Scans content for financial terminology.
    fn scan_financial_terms(content: &str) -> usize {
        let lower = content.to_lowercase();
        let terms = [
            "account number", "routing number", "iban", "swift", "bic",
            "balance", "transaction", "wire transfer", "ach",
            "tax id", "ein", "tin", "1099", "w-2", "w-9",
            "sox", "gaap", "ebitda", "revenue", "profit",
        ];
        terms.iter().filter(|t| lower.contains(**t)).count()
    }
}

/// Suggests data classification based on both stream name and content analysis.
///
/// Performs lightweight content scanning for sensitive data patterns:
/// - SSN patterns (XXX-XX-XXXX) -> PII
/// - Credit card numbers (13-16 digits) -> PCI
/// - Email addresses -> PII
/// - Medical terminology -> PHI
/// - Financial terminology -> Financial
///
/// Returns the higher classification between stream name inference and
/// content analysis (conservative approach).
pub fn suggest_classification(stream_name: &str, content_sample: &[u8]) -> DataClass {
    let name_class = infer_from_stream_name(stream_name);

    // If content is empty or not valid UTF-8, fall back to name-based
    let content = match std::str::from_utf8(content_sample) {
        Ok(s) => s,
        Err(_) => return name_class,
    };

    if content.is_empty() {
        return name_class;
    }

    let result = classify_content(stream_name, content);
    result.data_class
}

/// Performs detailed content classification with confidence scoring.
///
/// Returns a `ClassificationResult` with the detected data class,
/// confidence score, and list of matched patterns for audit purposes.
pub fn classify_content(stream_name: &str, content: &str) -> ClassificationResult {
    let name_class = infer_from_stream_name(stream_name);
    let mut matched = Vec::new();
    let mut content_class = DataClass::Public;

    // Check for SSN patterns -> PII
    if ContentScanner::scan_ssn(content) {
        matched.push("ssn_pattern".to_string());
        content_class = higher_class(content_class, DataClass::PII);
    }

    // Check for credit card patterns -> PCI
    if ContentScanner::scan_credit_card(content) {
        matched.push("credit_card_pattern".to_string());
        content_class = higher_class(content_class, DataClass::PCI);
    }

    // Check for email patterns -> PII
    if ContentScanner::scan_email(content) {
        matched.push("email_pattern".to_string());
        content_class = higher_class(content_class, DataClass::PII);
    }

    // Check for medical terms -> PHI (threshold: 2+ terms)
    let medical_count = ContentScanner::scan_medical_terms(content);
    if medical_count >= 2 {
        matched.push(format!("medical_terms({medical_count})"));
        content_class = higher_class(content_class, DataClass::PHI);
    }

    // Check for financial terms -> Financial (threshold: 2+ terms)
    let financial_count = ContentScanner::scan_financial_terms(content);
    if financial_count >= 2 {
        matched.push(format!("financial_terms({financial_count})"));
        content_class = higher_class(content_class, DataClass::Financial);
    }

    // Take the more restrictive of name-based and content-based
    let final_class = higher_class(name_class, content_class);

    // Calculate confidence based on number and strength of matches
    let confidence = if matched.is_empty() {
        // Only name-based, moderate confidence
        0.6
    } else if final_class == name_class && !matched.is_empty() {
        // Name and content agree, high confidence
        0.95
    } else {
        // Content-based only, good confidence
        0.7 + (matched.len() as f64 * 0.05).min(0.2)
    };

    ClassificationResult {
        data_class: final_class,
        confidence,
        matched_patterns: matched,
    }
}

/// Returns the more restrictive of two data classes.
fn higher_class(a: DataClass, b: DataClass) -> DataClass {
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
    if restrictiveness(b) > restrictiveness(a) { b } else { a }
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

    #[test]
    fn test_content_scan_ssn() {
        assert!(ContentScanner::scan_ssn("SSN: 123-45-6789"));
        assert!(!ContentScanner::scan_ssn("phone: 123-456-789"));
        assert!(!ContentScanner::scan_ssn("short"));
    }

    #[test]
    fn test_content_scan_credit_card() {
        assert!(ContentScanner::scan_credit_card("4111111111111111"));
        assert!(ContentScanner::scan_credit_card("4111 1111 1111 1111"));
        assert!(!ContentScanner::scan_credit_card("12345"));
    }

    #[test]
    fn test_content_scan_email() {
        assert!(ContentScanner::scan_email("contact user@example.com please"));
        assert!(!ContentScanner::scan_email("no email here"));
        assert!(!ContentScanner::scan_email("@invalid"));
    }

    #[test]
    fn test_content_scan_medical_terms() {
        let text = "Patient diagnosis: ICD-10 code J06.9, prescribed medication 500mg";
        assert!(ContentScanner::scan_medical_terms(text) >= 2);

        let text = "The weather today is sunny";
        assert_eq!(ContentScanner::scan_medical_terms(text), 0);
    }

    #[test]
    fn test_content_scan_financial_terms() {
        let text = "Wire transfer to account number 12345, routing number 67890";
        assert!(ContentScanner::scan_financial_terms(text) >= 2);

        let text = "The cat sat on the mat";
        assert_eq!(ContentScanner::scan_financial_terms(text), 0);
    }

    #[test]
    fn test_suggest_classification_with_content() {
        // Content with SSN should upgrade from Confidential to PII
        let class = suggest_classification(
            "unknown_data",
            b"Record: John Doe, SSN 123-45-6789",
        );
        assert_eq!(class, DataClass::PII);

        // Content with credit card should upgrade to PCI
        let class = suggest_classification(
            "unknown_data",
            b"Card: 4111111111111111",
        );
        assert_eq!(class, DataClass::PCI);

        // Empty content falls back to name-based
        let class = suggest_classification("patient_records", b"");
        assert_eq!(class, DataClass::PHI);
    }

    #[test]
    fn test_classify_content_confidence() {
        // Name + content agreement = high confidence
        let result = classify_content(
            "patient_records",
            "Patient diagnosis: ICD-10 J06.9, medication prescribed 500mg",
        );
        assert_eq!(result.data_class, DataClass::PHI);
        assert!(result.confidence > 0.9);
        assert!(!result.matched_patterns.is_empty());

        // Name only = moderate confidence
        let result = classify_content("patient_records", "some random text");
        assert_eq!(result.data_class, DataClass::PHI);
        assert!(result.confidence < 0.7);
    }

    #[test]
    fn test_classify_content_takes_higher() {
        // Content detects PCI even though name says Public
        let result = classify_content(
            "public_data",
            "Payment card: 4111111111111111",
        );
        assert_eq!(result.data_class, DataClass::PCI);
    }

    #[test]
    fn test_higher_class() {
        assert_eq!(higher_class(DataClass::Public, DataClass::PHI), DataClass::PHI);
        assert_eq!(higher_class(DataClass::PHI, DataClass::Public), DataClass::PHI);
        assert_eq!(higher_class(DataClass::PII, DataClass::PCI), DataClass::PCI);
    }
}
