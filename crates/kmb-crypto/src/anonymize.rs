//! Anonymization and data masking primitives for secure data sharing.
//!
//! This module provides utilities for transforming sensitive data before
//! sharing with third parties (analytics providers, LLMs, auditors).
//!
//! # Transformation Hierarchy
//!
//! From least to most protective:
//!
//! 1. **Generalization**: Reduce precision (age → age range, ZIP → region)
//! 2. **Pseudonymization**: Replace with consistent token (reversible optional)
//! 3. **Redaction**: Remove/mask entirely
//!
//! # Example
//!
//! ```
//! use kmb_crypto::anonymize::{
//!     redact, mask, truncate_date, generalize_age, generalize_zip,
//!     MaskStyle, DatePrecision,
//! };
//!
//! // Full redaction - completely remove the value
//! let redacted: Option<&str> = redact();
//! assert!(redacted.is_none());
//!
//! // Masking - replace with placeholder
//! let masked = mask("John Doe", MaskStyle::Fixed("***"));
//! assert_eq!(masked, "***");
//!
//! // Age generalization (5-year buckets)
//! let age_range = generalize_age(47, 5);
//! assert_eq!(age_range, "45-49");
//!
//! // Date truncation (year-month only)
//! let truncated = truncate_date(2024, 3, 15, DatePrecision::Month);
//! assert_eq!(truncated, "2024-03");
//!
//! // ZIP code generalization (first 3 digits)
//! let partial_zip = generalize_zip("90210", 3);
//! assert_eq!(partial_zip, "902**");
//! ```
//!
//! # Security Considerations
//!
//! - **Generalization** preserves some utility but may allow re-identification
//!   if combined with other data (quasi-identifiers)
//! - **Pseudonymization** is reversible only with the key; still subject to
//!   frequency analysis
//! - **Redaction** is safest but eliminates utility; use when field adds no
//!   analytical value

// ============================================================================
// Redaction
// ============================================================================

/// Returns `None` to represent a fully redacted value.
///
/// Use this when a field should be completely removed from shared data.
/// The returned `Option` can be used directly with serialization that
/// skips `None` values.
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::redact;
///
/// let ssn: Option<&str> = redact();
/// assert!(ssn.is_none());
/// ```
#[inline]
pub fn redact<T>() -> Option<T> {
    None
}

// ============================================================================
// Masking
// ============================================================================

/// Style for masking sensitive values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskStyle<'a> {
    /// Replace with a fixed placeholder (e.g., "***" or "\[REDACTED\]").
    Fixed(&'a str),
    /// Replace each character with a mask character (e.g., 'X').
    PerCharacter(char),
    /// Preserve first N characters, mask the rest.
    PreservePrefix(usize, char),
    /// Preserve last N characters, mask the rest.
    PreserveSuffix(usize, char),
}

/// Masks a string value according to the specified style.
///
/// # Arguments
///
/// * `value` - The sensitive value to mask
/// * `style` - How to mask the value
///
/// # Returns
///
/// A new string with the value masked.
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::{mask, MaskStyle};
///
/// // Fixed replacement
/// assert_eq!(mask("secret", MaskStyle::Fixed("[REDACTED]")), "[REDACTED]");
///
/// // Per-character masking
/// assert_eq!(mask("secret", MaskStyle::PerCharacter('*')), "******");
///
/// // Preserve prefix (e.g., for phone numbers)
/// assert_eq!(mask("555-123-4567", MaskStyle::PreservePrefix(4, '*')), "555-********");
///
/// // Preserve suffix (e.g., for credit cards)
/// assert_eq!(mask("4111111111111111", MaskStyle::PreserveSuffix(4, '*')), "************1111");
/// ```
pub fn mask(value: &str, style: MaskStyle<'_>) -> String {
    match style {
        MaskStyle::Fixed(placeholder) => placeholder.to_string(),
        MaskStyle::PerCharacter(mask_char) => mask_char.to_string().repeat(value.chars().count()),
        MaskStyle::PreservePrefix(n, mask_char) => {
            let chars: Vec<char> = value.chars().collect();
            let preserved: String = chars.iter().take(n).collect();
            let masked: String = mask_char.to_string().repeat(chars.len().saturating_sub(n));
            format!("{preserved}{masked}")
        }
        MaskStyle::PreserveSuffix(n, mask_char) => {
            let chars: Vec<char> = value.chars().collect();
            let total = chars.len();
            let masked: String = mask_char.to_string().repeat(total.saturating_sub(n));
            let preserved: String = chars.iter().skip(total.saturating_sub(n)).collect();
            format!("{masked}{preserved}")
        }
    }
}

// ============================================================================
// Date Generalization
// ============================================================================

/// Precision level for date truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePrecision {
    /// Keep only the year (e.g., "2024")
    Year,
    /// Keep year and month (e.g., "2024-03")
    Month,
    /// Keep year, month, and day (full date)
    Day,
    /// Keep year and quarter (e.g., "2024-Q1")
    Quarter,
}

/// Truncates a date to the specified precision.
///
/// This is useful for sharing temporal data while reducing identifiability.
/// For example, a birth date might be truncated to just the year for
/// age-based analysis.
///
/// # Arguments
///
/// * `year` - The year
/// * `month` - The month (1-12)
/// * `day` - The day (1-31)
/// * `precision` - The desired precision level
///
/// # Returns
///
/// A string representation of the truncated date.
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::{truncate_date, DatePrecision};
///
/// assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Year), "2024");
/// assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Month), "2024-03");
/// assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Day), "2024-03-15");
/// assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Quarter), "2024-Q1");
/// ```
pub fn truncate_date(year: u16, month: u8, day: u8, precision: DatePrecision) -> String {
    // Precondition: valid date components
    debug_assert!((1..=12).contains(&month), "month must be 1-12");
    debug_assert!((1..=31).contains(&day), "day must be 1-31");

    match precision {
        DatePrecision::Year => format!("{year}"),
        DatePrecision::Month => format!("{year}-{month:02}"),
        DatePrecision::Day => format!("{year}-{month:02}-{day:02}"),
        DatePrecision::Quarter => {
            let quarter = (month - 1) / 3 + 1;
            format!("{year}-Q{quarter}")
        }
    }
}

// ============================================================================
// Age Generalization
// ============================================================================

/// Generalizes an age into a range (bucket).
///
/// This is a common HIPAA Safe Harbor technique. The standard recommends
/// 5-year buckets for ages under 90, and grouping all ages 90+ together.
///
/// # Arguments
///
/// * `age` - The age in years
/// * `bucket_size` - The size of each age bucket (e.g., 5 for 5-year ranges)
///
/// # Returns
///
/// A string representing the age range (e.g., "45-49").
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::generalize_age;
///
/// // Standard 5-year buckets
/// assert_eq!(generalize_age(23, 5), "20-24");
/// assert_eq!(generalize_age(45, 5), "45-49");
/// assert_eq!(generalize_age(50, 5), "50-54");
///
/// // HIPAA Safe Harbor: 90+ grouped
/// assert_eq!(generalize_age(95, 5), "90+");
///
/// // 10-year buckets
/// assert_eq!(generalize_age(45, 10), "40-49");
/// ```
/// HIPAA Safe Harbor: ages 90+ are treated as a single category.
const HIPAA_SAFE_HARBOR_THRESHOLD: u8 = 90;

pub fn generalize_age(age: u8, bucket_size: u8) -> String {
    // Precondition: bucket size is positive
    debug_assert!(bucket_size > 0, "bucket_size must be positive");

    if age >= HIPAA_SAFE_HARBOR_THRESHOLD {
        return format!("{HIPAA_SAFE_HARBOR_THRESHOLD}+");
    }

    let bucket_start = (age / bucket_size) * bucket_size;
    let bucket_end = bucket_start + bucket_size - 1;

    format!("{bucket_start}-{bucket_end}")
}

// ============================================================================
// Geographic Generalization
// ============================================================================

/// Generalizes a ZIP code by preserving only the first N digits.
///
/// This reduces geographic precision while maintaining regional information.
/// HIPAA Safe Harbor allows first 3 digits if the population is > 20,000.
///
/// # Arguments
///
/// * `zip` - The full ZIP code (string to handle leading zeros)
/// * `preserve_digits` - Number of digits to preserve (1-5)
///
/// # Returns
///
/// A partially masked ZIP code with asterisks for hidden digits.
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::generalize_zip;
///
/// // Preserve first 3 digits (HIPAA Safe Harbor)
/// assert_eq!(generalize_zip("90210", 3), "902**");
/// assert_eq!(generalize_zip("02134", 3), "021**");
///
/// // Preserve first digit only (very coarse)
/// assert_eq!(generalize_zip("90210", 1), "9****");
///
/// // Full ZIP (no generalization)
/// assert_eq!(generalize_zip("90210", 5), "90210");
/// ```
pub fn generalize_zip(zip: &str, preserve_digits: usize) -> String {
    // Precondition: reasonable preservation
    debug_assert!(
        (1..=5).contains(&preserve_digits),
        "preserve_digits must be 1-5"
    );

    let zip_chars: Vec<char> = zip.chars().take(5).collect();
    let preserved: String = zip_chars.iter().take(preserve_digits).collect();
    let masked: String = "*".repeat(5_usize.saturating_sub(preserve_digits));

    format!("{preserved}{masked}")
}

/// Geographic hierarchy levels for generalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoLevel {
    /// Full address (no generalization)
    Full,
    /// ZIP code only (no street)
    ZipCode,
    /// ZIP-3 prefix (first 3 digits)
    Zip3,
    /// City only
    City,
    /// State/Province only
    State,
    /// Country only
    Country,
    /// Region (e.g., "Northeast US", "Western Europe")
    Region,
}

// ============================================================================
// Numeric Generalization
// ============================================================================

/// Generalizes a numeric value into a range.
///
/// Useful for values like salary, weight, or any continuous measure
/// that should be shared with reduced precision.
///
/// # Arguments
///
/// * `value` - The numeric value
/// * `bucket_size` - The size of each bucket
///
/// # Returns
///
/// A string representing the range (e.g., "50000-59999").
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::generalize_numeric;
///
/// // Salary in $10k buckets
/// assert_eq!(generalize_numeric(75000, 10000), "70000-79999");
///
/// // Weight in 10kg buckets
/// assert_eq!(generalize_numeric(82, 10), "80-89");
/// ```
pub fn generalize_numeric(value: u64, bucket_size: u64) -> String {
    // Precondition: bucket size is positive
    debug_assert!(bucket_size > 0, "bucket_size must be positive");

    let bucket_start = (value / bucket_size) * bucket_size;
    let bucket_end = bucket_start + bucket_size - 1;

    format!("{bucket_start}-{bucket_end}")
}

// ============================================================================
// K-Anonymity Support
// ============================================================================

/// Result of a k-anonymity check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KAnonymityResult {
    /// The k value achieved (minimum group size)
    pub k: usize,
    /// Whether the dataset satisfies the target k
    pub satisfies_target: bool,
    /// Number of distinct equivalence classes
    pub equivalence_classes: usize,
    /// Size of the smallest equivalence class
    pub smallest_class_size: usize,
}

/// Checks if a set of quasi-identifier combinations achieves k-anonymity.
///
/// K-anonymity means every combination of quasi-identifiers appears at least
/// k times in the dataset. This prevents re-identification attacks.
///
/// # Arguments
///
/// * `quasi_identifiers` - Iterator over quasi-identifier tuples (as strings)
/// * `target_k` - The minimum k value required
///
/// # Returns
///
/// A [`KAnonymityResult`] with details about the achieved k-anonymity.
///
/// # Example
///
/// ```
/// use kmb_crypto::anonymize::check_k_anonymity;
///
/// // Dataset with generalized age and ZIP
/// let records = vec![
///     "20-29,902**",
///     "20-29,902**",
///     "30-39,902**",
///     "30-39,902**",
///     "30-39,902**",
/// ];
///
/// let result = check_k_anonymity(records.iter().map(|s| s.to_string()), 2);
/// assert!(result.satisfies_target); // Minimum group size is 2
/// assert_eq!(result.k, 2);
/// ```
pub fn check_k_anonymity(
    quasi_identifiers: impl Iterator<Item = String>,
    target_k: usize,
) -> KAnonymityResult {
    use std::collections::HashMap;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for qi in quasi_identifiers {
        *counts.entry(qi).or_insert(0) += 1;
    }

    let smallest = counts.values().min().copied().unwrap_or(0);
    let k = smallest;

    KAnonymityResult {
        k,
        satisfies_target: k >= target_k,
        equivalence_classes: counts.len(),
        smallest_class_size: smallest,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Redaction Tests
    // ========================================================================

    #[test]
    fn redact_returns_none() {
        let value: Option<&str> = redact();
        assert!(value.is_none());

        let numeric: Option<u64> = redact();
        assert!(numeric.is_none());
    }

    // ========================================================================
    // Masking Tests
    // ========================================================================

    #[test]
    fn mask_fixed() {
        assert_eq!(mask("secret", MaskStyle::Fixed("[REDACTED]")), "[REDACTED]");
        assert_eq!(mask("anything", MaskStyle::Fixed("***")), "***");
    }

    #[test]
    fn mask_per_character() {
        assert_eq!(mask("secret", MaskStyle::PerCharacter('*')), "******");
        assert_eq!(mask("ab", MaskStyle::PerCharacter('X')), "XX");
        assert_eq!(mask("", MaskStyle::PerCharacter('*')), "");
    }

    #[test]
    fn mask_preserve_prefix() {
        assert_eq!(
            mask("555-123-4567", MaskStyle::PreservePrefix(4, '*')),
            "555-********"
        );
        assert_eq!(mask("short", MaskStyle::PreservePrefix(10, '*')), "short");
        assert_eq!(mask("ab", MaskStyle::PreservePrefix(1, '*')), "a*");
    }

    #[test]
    fn mask_preserve_suffix() {
        assert_eq!(
            mask("4111111111111111", MaskStyle::PreserveSuffix(4, '*')),
            "************1111"
        );
        assert_eq!(mask("short", MaskStyle::PreserveSuffix(10, '*')), "short");
        assert_eq!(mask("ab", MaskStyle::PreserveSuffix(1, '*')), "*b");
    }

    #[test]
    fn mask_unicode() {
        // Should handle unicode characters correctly
        assert_eq!(mask("日本語", MaskStyle::PerCharacter('*')), "***");
        assert_eq!(mask("日本語", MaskStyle::PreservePrefix(1, '*')), "日**");
    }

    // ========================================================================
    // Date Generalization Tests
    // ========================================================================

    #[test]
    fn truncate_date_year() {
        assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Year), "2024");
        assert_eq!(truncate_date(1999, 12, 31, DatePrecision::Year), "1999");
    }

    #[test]
    fn truncate_date_month() {
        assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Month), "2024-03");
        assert_eq!(truncate_date(2024, 11, 1, DatePrecision::Month), "2024-11");
    }

    #[test]
    fn truncate_date_day() {
        assert_eq!(truncate_date(2024, 3, 15, DatePrecision::Day), "2024-03-15");
        assert_eq!(truncate_date(2024, 1, 5, DatePrecision::Day), "2024-01-05");
    }

    #[test]
    fn truncate_date_quarter() {
        assert_eq!(
            truncate_date(2024, 1, 15, DatePrecision::Quarter),
            "2024-Q1"
        );
        assert_eq!(
            truncate_date(2024, 3, 31, DatePrecision::Quarter),
            "2024-Q1"
        );
        assert_eq!(truncate_date(2024, 4, 1, DatePrecision::Quarter), "2024-Q2");
        assert_eq!(
            truncate_date(2024, 6, 30, DatePrecision::Quarter),
            "2024-Q2"
        );
        assert_eq!(truncate_date(2024, 7, 1, DatePrecision::Quarter), "2024-Q3");
        assert_eq!(
            truncate_date(2024, 9, 30, DatePrecision::Quarter),
            "2024-Q3"
        );
        assert_eq!(
            truncate_date(2024, 10, 1, DatePrecision::Quarter),
            "2024-Q4"
        );
        assert_eq!(
            truncate_date(2024, 12, 31, DatePrecision::Quarter),
            "2024-Q4"
        );
    }

    // ========================================================================
    // Age Generalization Tests
    // ========================================================================

    #[test]
    fn generalize_age_5_year_buckets() {
        assert_eq!(generalize_age(0, 5), "0-4");
        assert_eq!(generalize_age(4, 5), "0-4");
        assert_eq!(generalize_age(5, 5), "5-9");
        assert_eq!(generalize_age(23, 5), "20-24");
        assert_eq!(generalize_age(45, 5), "45-49");
        assert_eq!(generalize_age(89, 5), "85-89");
    }

    #[test]
    fn generalize_age_hipaa_safe_harbor() {
        // HIPAA requires ages 90+ to be grouped
        assert_eq!(generalize_age(90, 5), "90+");
        assert_eq!(generalize_age(95, 5), "90+");
        assert_eq!(generalize_age(100, 5), "90+");
        assert_eq!(generalize_age(255, 5), "90+"); // Edge case: max u8
    }

    #[test]
    fn generalize_age_10_year_buckets() {
        assert_eq!(generalize_age(23, 10), "20-29");
        assert_eq!(generalize_age(45, 10), "40-49");
        assert_eq!(generalize_age(50, 10), "50-59");
    }

    // ========================================================================
    // ZIP Code Generalization Tests
    // ========================================================================

    #[test]
    fn generalize_zip_3_digits() {
        assert_eq!(generalize_zip("90210", 3), "902**");
        assert_eq!(generalize_zip("02134", 3), "021**");
        assert_eq!(generalize_zip("12345", 3), "123**");
    }

    #[test]
    fn generalize_zip_1_digit() {
        assert_eq!(generalize_zip("90210", 1), "9****");
        assert_eq!(generalize_zip("02134", 1), "0****");
    }

    #[test]
    fn generalize_zip_full() {
        assert_eq!(generalize_zip("90210", 5), "90210");
    }

    #[test]
    fn generalize_zip_short_input() {
        // Handle ZIPs shorter than 5 digits gracefully
        assert_eq!(generalize_zip("902", 3), "902**");
    }

    // ========================================================================
    // Numeric Generalization Tests
    // ========================================================================

    #[test]
    fn generalize_numeric_salary() {
        assert_eq!(generalize_numeric(75000, 10000), "70000-79999");
        assert_eq!(generalize_numeric(50000, 10000), "50000-59999");
        assert_eq!(generalize_numeric(99999, 10000), "90000-99999");
        assert_eq!(generalize_numeric(100_000, 10000), "100000-109999");
    }

    #[test]
    fn generalize_numeric_weight() {
        assert_eq!(generalize_numeric(82, 10), "80-89");
        assert_eq!(generalize_numeric(70, 10), "70-79");
        assert_eq!(generalize_numeric(5, 10), "0-9");
    }

    // ========================================================================
    // K-Anonymity Tests
    // ========================================================================

    #[test]
    fn k_anonymity_satisfied() {
        let records = vec![
            "20-29,902**".to_string(),
            "20-29,902**".to_string(),
            "30-39,902**".to_string(),
            "30-39,902**".to_string(),
            "30-39,902**".to_string(),
        ];

        let result = check_k_anonymity(records.into_iter(), 2);

        assert!(result.satisfies_target);
        assert_eq!(result.k, 2); // Smallest group has 2 members
        assert_eq!(result.equivalence_classes, 2);
        assert_eq!(result.smallest_class_size, 2);
    }

    #[test]
    fn k_anonymity_not_satisfied() {
        let records = vec![
            "20-29,902**".to_string(),
            "30-39,902**".to_string(), // Only 1 in this group
            "40-49,902**".to_string(),
            "40-49,902**".to_string(),
        ];

        let result = check_k_anonymity(records.into_iter(), 2);

        assert!(!result.satisfies_target);
        assert_eq!(result.k, 1); // Smallest group has only 1 member
        assert_eq!(result.equivalence_classes, 3);
    }

    #[test]
    fn k_anonymity_empty() {
        let result = check_k_anonymity(std::iter::empty(), 2);

        assert!(!result.satisfies_target);
        assert_eq!(result.k, 0);
        assert_eq!(result.equivalence_classes, 0);
    }

    // ========================================================================
    // Property-Based Tests
    // ========================================================================

    use proptest::prelude::*;

    proptest! {
        /// Property: generalize_age always produces valid ranges
        #[test]
        fn prop_generalize_age_valid_format(age in 0u8..=255u8, bucket_size in 1u8..=50u8) {
            let result = generalize_age(age, bucket_size);

            if age >= 90 {
                // HIPAA safe harbor: ages 90+ grouped
                prop_assert_eq!(result, "90+");
            } else {
                // Should be in format "X-Y" or "90+"
                if result != "90+" {
                    let parts: Vec<&str> = result.split('-').collect();
                    prop_assert_eq!(parts.len(), 2, "age range must have format X-Y");

                    // Parse bounds
                    let lower: u8 = parts[0].parse().expect("lower bound must be numeric");
                    let upper: u8 = parts[1].parse().expect("upper bound must be numeric");

                    // Verify age falls in range
                    prop_assert!(age >= lower && age <= upper,
                        "age {} must be in range {}-{}", age, lower, upper);

                    // Verify bucket size
                    prop_assert_eq!(upper - lower + 1, bucket_size,
                        "range size must equal bucket size");
                }
            }
        }

        /// Property: generalize_age is deterministic
        #[test]
        fn prop_generalize_age_deterministic(age in 0u8..=100u8, bucket_size in 1u8..=20u8) {
            let result1 = generalize_age(age, bucket_size);
            let result2 = generalize_age(age, bucket_size);

            prop_assert_eq!(result1, result2);
        }

        /// Property: generalize_zip preserves specified number of digits
        #[test]
        fn prop_generalize_zip_preserves_digits(
            zip in "[0-9]{5}",
            digits in 1usize..=5usize, // preserve_digits must be 1-5
        ) {
            let result = generalize_zip(&zip, digits);

            // Check length (should be 5 chars total)
            prop_assert_eq!(result.chars().count(), 5);

            // First N digits should match original
            for (i, (orig, generated)) in zip.chars().zip(result.chars()).enumerate() {
                if i < digits {
                    prop_assert_eq!(orig, generated, "digit {} should be preserved", i);
                } else {
                    prop_assert_eq!(generated, '*', "digit {} should be masked", i);
                }
            }
        }

        /// Property: generalize_zip is deterministic
        #[test]
        fn prop_generalize_zip_deterministic(
            zip in "[0-9]{5}",
            digits in 1usize..=5usize, // preserve_digits must be 1-5
        ) {
            let result1 = generalize_zip(&zip, digits);
            let result2 = generalize_zip(&zip, digits);

            prop_assert_eq!(result1, result2);
        }

        /// Property: mask with PerCharacter produces correct length
        #[test]
        fn prop_mask_per_character_length(
            value in "\\PC{1,100}",
            mask_char in any::<char>().prop_filter("printable char", |c| c.is_ascii_graphic()),
        ) {
            let result = mask(&value, MaskStyle::PerCharacter(mask_char));
            let value_len = value.chars().count();
            let result_len = result.chars().count();

            prop_assert_eq!(result_len, value_len);

            // All chars should be mask_char
            for ch in result.chars() {
                prop_assert_eq!(ch, mask_char);
            }
        }

        /// Property: mask with Fixed always returns the fixed string
        #[test]
        fn prop_mask_fixed_constant(
            value in "\\PC{1,100}",
            placeholder in "\\PC{1,20}",
        ) {
            let result = mask(&value, MaskStyle::Fixed(&placeholder));
            prop_assert_eq!(result, placeholder);
        }

        /// Property: mask PreservePrefix preserves first N chars
        #[test]
        fn prop_mask_preserve_prefix(
            value in "\\PC{5,100}",
            n in 1usize..=4usize,
        ) {
            let result = mask(&value, MaskStyle::PreservePrefix(n, '*'));
            let value_chars: Vec<char> = value.chars().collect();
            let result_chars: Vec<char> = result.chars().collect();

            // First n chars should match
            for i in 0..n.min(value_chars.len()) {
                prop_assert_eq!(result_chars[i], value_chars[i],
                    "char {} should be preserved", i);
            }

            // Remaining should be '*'
            for i in n..result_chars.len() {
                prop_assert_eq!(result_chars[i], '*',
                    "char {} should be masked", i);
            }
        }

        /// Property: mask PreserveSuffix preserves last N chars
        #[test]
        fn prop_mask_preserve_suffix(
            value in "\\PC{5,100}",
            n in 1usize..=4usize,
        ) {
            let result = mask(&value, MaskStyle::PreserveSuffix(n, '*'));
            let value_chars: Vec<char> = value.chars().collect();
            let result_chars: Vec<char> = result.chars().collect();
            let value_len = value_chars.len();

            // Last n chars should match
            let start_idx = value_len.saturating_sub(n);
            for (i, &ch) in value_chars.iter().skip(start_idx).enumerate() {
                let result_idx = start_idx + i;
                prop_assert_eq!(result_chars[result_idx], ch,
                    "char {} should be preserved", result_idx);
            }

            // Leading chars should be '*'
            for i in 0..start_idx {
                prop_assert_eq!(result_chars[i], '*',
                    "char {} should be masked", i);
            }
        }

        /// Property: generalize_numeric produces valid ranges
        #[test]
        fn prop_generalize_numeric_valid(
            value in 0u64..=1_000_000u64,
            bucket_size in 1u64..=10_000u64,
        ) {
            let result = generalize_numeric(value, bucket_size);
            let parts: Vec<&str> = result.split('-').collect();

            prop_assert_eq!(parts.len(), 2, "must have format X-Y");

            let lower: u64 = parts[0].parse().expect("lower bound must be numeric");
            let upper: u64 = parts[1].parse().expect("upper bound must be numeric");

            // Value must be in range
            prop_assert!(value >= lower && value <= upper,
                "value {} must be in range {}-{}", value, lower, upper);

            // Range size must equal bucket_size
            prop_assert_eq!(upper - lower + 1, bucket_size,
                "range size must equal bucket size");
        }

        /// Property: truncate_date is deterministic
        #[test]
        fn prop_truncate_date_deterministic(
            year in 1900u16..=2100u16,
            month in 1u8..=12u8,
            day in 1u8..=28u8, // Use 28 to avoid invalid dates
            precision_idx in 0usize..=3usize,
        ) {
            let precision = match precision_idx {
                0 => DatePrecision::Year,
                1 => DatePrecision::Month,
                2 => DatePrecision::Quarter,
                _ => DatePrecision::Day,
            };

            let result1 = truncate_date(year, month, day, precision);
            let result2 = truncate_date(year, month, day, precision);

            prop_assert_eq!(result1, result2);
        }

        /// Property: truncate_date with Year only includes year
        #[test]
        fn prop_truncate_date_year(
            year in 1900u16..=2100u16,
            month in 1u8..=12u8,
            day in 1u8..=28u8,
        ) {
            let result = truncate_date(year, month, day, DatePrecision::Year);
            prop_assert_eq!(result, year.to_string());
        }

        /// Property: truncate_date with Month includes year-month
        #[test]
        fn prop_truncate_date_month(
            year in 1900u16..=2100u16,
            month in 1u8..=12u8,
            day in 1u8..=28u8,
        ) {
            let result = truncate_date(year, month, day, DatePrecision::Month);
            let expected = format!("{}-{:02}", year, month);
            prop_assert_eq!(result, expected);
        }

        /// Property: k-anonymity check is monotonic (larger k requires more records)
        #[test]
        fn prop_k_anonymity_monotonic(
            group_sizes in prop::collection::vec(1usize..=10usize, 1..20),
        ) {
            // Generate records with known group sizes
            let mut records = Vec::new();
            for (group_id, &size) in group_sizes.iter().enumerate() {
                for _ in 0..size {
                    records.push(format!("group_{}", group_id));
                }
            }

            let min_group_size = *group_sizes.iter().min().unwrap_or(&0);

            // Check with k = min_group_size (should pass)
            let result_pass = check_k_anonymity(records.clone().into_iter(), min_group_size);
            prop_assert!(result_pass.satisfies_target);
            prop_assert_eq!(result_pass.k, min_group_size);

            // Check with k = min_group_size + 1 (should fail unless all groups are larger)
            let result_fail = check_k_anonymity(records.into_iter(), min_group_size + 1);
            prop_assert!(!result_fail.satisfies_target);
            prop_assert!(result_fail.k <= min_group_size);
        }

        /// Property: k-anonymity equivalence class count is correct
        #[test]
        fn prop_k_anonymity_equivalence_classes(
            num_classes in 1usize..=20usize,
            records_per_class in 1usize..=10usize,
        ) {
            // Generate exactly num_classes equivalence classes
            let mut records = Vec::new();
            for class_id in 0..num_classes {
                for _ in 0..records_per_class {
                    records.push(format!("class_{}", class_id));
                }
            }

            let result = check_k_anonymity(records.into_iter(), 1);

            prop_assert_eq!(result.equivalence_classes, num_classes);
            prop_assert_eq!(result.smallest_class_size, records_per_class);
            prop_assert_eq!(result.k, records_per_class);
        }
    }

    // ========================================================================
    // Additional Edge Case Tests
    // ========================================================================

    use test_case::test_case;

    #[test_case(0, 5 => "0-4"; "age 0")]
    #[test_case(89, 5 => "85-89"; "age 89")]
    #[test_case(90, 5 => "90+"; "age 90 HIPAA boundary")]
    #[test_case(255, 5 => "90+"; "age 255 max u8")]
    fn generalize_age_edge_cases(age: u8, bucket_size: u8) -> String {
        generalize_age(age, bucket_size)
    }

    #[test_case("00000", 1 => "0****"; "only first digit")]
    #[test_case("12345", 5 => "12345"; "none masked")]
    #[test_case("90210", 3 => "902**"; "first 3")]
    fn generalize_zip_edge_cases(zip: &str, digits: usize) -> String {
        generalize_zip(zip, digits)
    }

    #[test]
    fn mask_empty_string() {
        assert_eq!(mask("", MaskStyle::PerCharacter('*')), "");
        assert_eq!(mask("", MaskStyle::PreservePrefix(5, '*')), "");
        assert_eq!(mask("", MaskStyle::PreserveSuffix(5, '*')), "");
        assert_eq!(mask("", MaskStyle::Fixed("[REDACTED]")), "[REDACTED]");
    }

    #[test]
    fn mask_unicode_characters() {
        // Emoji
        let emoji_result = mask("😀😁😂", MaskStyle::PerCharacter('*'));
        assert_eq!(emoji_result.chars().count(), 3);

        // Japanese
        let japanese = "日本語";
        let result = mask(japanese, MaskStyle::PreservePrefix(1, '*'));
        let chars: Vec<char> = result.chars().collect();
        assert_eq!(chars[0], '日');
        assert_eq!(chars[1], '*');
        assert_eq!(chars[2], '*');
    }

    #[test]
    fn generalize_numeric_edge_values() {
        assert_eq!(generalize_numeric(0, 10), "0-9");
        // u64::MAX would cause overflow, so test a large value instead
        let large_value = 1_000_000_000_000u64;
        let result = generalize_numeric(large_value, 100);
        assert!(result.contains("1000000000000"));
    }

    #[test]
    fn truncate_date_quarter_boundaries() {
        // Q1: Jan-Mar
        assert_eq!(truncate_date(2024, 1, 1, DatePrecision::Quarter), "2024-Q1");
        assert_eq!(truncate_date(2024, 3, 31, DatePrecision::Quarter), "2024-Q1");

        // Q2: Apr-Jun
        assert_eq!(truncate_date(2024, 4, 1, DatePrecision::Quarter), "2024-Q2");
        assert_eq!(truncate_date(2024, 6, 30, DatePrecision::Quarter), "2024-Q2");

        // Q3: Jul-Sep
        assert_eq!(truncate_date(2024, 7, 1, DatePrecision::Quarter), "2024-Q3");
        assert_eq!(truncate_date(2024, 9, 30, DatePrecision::Quarter), "2024-Q3");

        // Q4: Oct-Dec
        assert_eq!(truncate_date(2024, 10, 1, DatePrecision::Quarter), "2024-Q4");
        assert_eq!(truncate_date(2024, 12, 31, DatePrecision::Quarter), "2024-Q4");
    }

    #[test]
    fn k_anonymity_single_record_fails() {
        let records: Vec<String> = vec!["unique".to_string()];
        let result = check_k_anonymity(records.into_iter(), 2);

        assert!(!result.satisfies_target);
        assert_eq!(result.k, 1);
        assert_eq!(result.equivalence_classes, 1);
    }

    #[test]
    fn k_anonymity_all_identical() {
        let records: Vec<String> = vec!["same".to_string(); 100];
        let result = check_k_anonymity(records.into_iter(), 2);

        assert!(result.satisfies_target);
        assert_eq!(result.k, 100);
        assert_eq!(result.equivalence_classes, 1);
        assert_eq!(result.smallest_class_size, 100);
    }

    #[test]
    fn k_anonymity_exact_boundary() {
        // 3 groups of size 5 each
        let records = vec![
            "A", "A", "A", "A", "A",
            "B", "B", "B", "B", "B",
            "C", "C", "C", "C", "C",
        ];

        // k=5 should pass
        let records_owned: Vec<String> = records.iter().map(|&s| s.to_string()).collect();
        let result_pass = check_k_anonymity(records_owned.clone().into_iter(), 5);
        assert!(result_pass.satisfies_target);

        // k=6 should fail
        let result_fail = check_k_anonymity(records_owned.into_iter(), 6);
        assert!(!result_fail.satisfies_target);
    }

    #[test]
    fn mask_style_consistency() {
        let value = "sensitive";

        // Masking should be deterministic
        assert_eq!(
            mask(value, MaskStyle::PerCharacter('X')),
            mask(value, MaskStyle::PerCharacter('X'))
        );

        assert_eq!(
            mask(value, MaskStyle::PreservePrefix(3, '*')),
            mask(value, MaskStyle::PreservePrefix(3, '*'))
        );
    }

    #[test]
    fn generalize_age_bucket_size_1() {
        // Bucket size of 1 means no generalization (exact age)
        assert_eq!(generalize_age(25, 1), "25-25");
        assert_eq!(generalize_age(50, 1), "50-50");
        assert_eq!(generalize_age(90, 1), "90+"); // HIPAA exception still applies
    }

    #[test]
    fn generalize_zip_handles_short_input() {
        // Handle ZIPs shorter than 5 digits
        assert_eq!(generalize_zip("123", 3), "123**");
        assert_eq!(generalize_zip("1", 1), "1****");
    }
}
