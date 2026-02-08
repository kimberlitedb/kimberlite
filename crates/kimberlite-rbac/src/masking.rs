//! Field-level data masking for RBAC.
//!
//! Implements 5 masking strategies for field-level data security,
//! supporting HIPAA SS 164.312(a)(1) "minimum necessary" principle.
//!
//! ## Strategies
//!
//! | Strategy   | Description                          | Reversible |
//! |------------|--------------------------------------|------------|
//! | Redact     | Pattern-aware partial redaction       | No         |
//! | Hash       | SHA-256 one-way hash                 | No         |
//! | Tokenize   | Deterministic BLAKE3 token           | No         |
//! | Truncate   | Keep first N characters              | No         |
//! | Null       | Replace with empty bytes             | No         |
//!
//! ## Examples
//!
//! ```
//! use kimberlite_rbac::masking::{
//!     FieldMask, MaskingPolicy, MaskingStrategy, RedactPattern, apply_mask,
//! };
//! use kimberlite_rbac::roles::Role;
//!
//! // Redact SSN: "123-45-6789" -> "***-**-6789"
//! let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
//!     .applies_to(Role::User)
//!     .applies_to(Role::Analyst)
//!     .exempt(Role::Admin);
//!
//! let value = b"123-45-6789";
//! let masked = apply_mask(value, &mask, &Role::User).unwrap();
//! assert_eq!(masked, b"***-**-6789");
//! ```

use crate::roles::Role;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during masking operations.
#[derive(Debug, Error)]
pub enum MaskingError {
    /// The value does not match the expected pattern for the redact strategy.
    #[error("Value does not match expected pattern for {pattern:?}: {reason}")]
    PatternMismatch {
        pattern: RedactPattern,
        reason: String,
    },

    /// A column referenced in the policy was not found in the row.
    #[error("Column '{column}' not found in row")]
    ColumnNotFound { column: String },

    /// Row length does not match column count.
    #[error("Row has {row_len} values but {col_len} columns were provided")]
    ColumnCountMismatch { row_len: usize, col_len: usize },
}

/// Result type for masking operations.
pub type Result<T> = std::result::Result<T, MaskingError>;

// ---------------------------------------------------------------------------
// Masking strategy types
// ---------------------------------------------------------------------------

/// Pattern for partial redaction of known data formats.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RedactPattern {
    /// SSN: `***-**-6789` (last 4 visible).
    Ssn,
    /// Phone: `***-***-1234` (last 4 visible).
    Phone,
    /// Email: `j***@example.com` (first char + domain visible).
    Email,
    /// Credit card: `****-****-****-1234` (last 4 visible).
    CreditCard,
    /// Custom pattern with a fixed replacement string.
    Custom {
        /// The replacement string (applied verbatim).
        replacement: String,
    },
}

/// Strategy used to mask a field value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaskingStrategy {
    /// Pattern-aware partial redaction (e.g. SSN, email).
    Redact(RedactPattern),
    /// SHA-256 one-way hash, hex-encoded.
    Hash,
    /// Deterministic BLAKE3 token prefixed with `tok_` (first 16 hex chars).
    Tokenize,
    /// Keep first `max_chars` characters, pad with `"..."`.
    Truncate { max_chars: usize },
    /// Replace with empty bytes.
    Null,
}

// ---------------------------------------------------------------------------
// FieldMask & MaskingPolicy
// ---------------------------------------------------------------------------

/// Describes how a single column should be masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMask {
    /// Column name this mask applies to.
    pub column: String,
    /// The masking strategy to apply.
    pub strategy: MaskingStrategy,
    /// Roles for which masking is applied.
    /// - `None` = all non-exempt roles (default)
    /// - `Some(vec![...])` = only the listed roles
    /// - `Some(vec![])` = no roles (masking disabled)
    pub applies_to_roles: Option<Vec<Role>>,
    /// Roles that are exempt from masking.
    pub exempt_roles: Vec<Role>,
}

impl FieldMask {
    /// Creates a new field mask for the given column and strategy.
    pub fn new(column: &str, strategy: MaskingStrategy) -> Self {
        assert!(!column.is_empty(), "Column name must not be empty");
        Self {
            column: column.to_string(),
            strategy,
            applies_to_roles: None,
            exempt_roles: Vec::new(),
        }
    }

    /// Adds a role for which this mask is applied.
    ///
    /// The first call to `applies_to` transitions from `None` (all roles)
    /// to `Some(vec![role])`. Subsequent calls append to the list.
    pub fn applies_to(mut self, role: Role) -> Self {
        let roles = self.applies_to_roles.get_or_insert_with(Vec::new);
        if !roles.contains(&role) {
            roles.push(role);
        }
        self
    }

    /// Adds a role that is exempt from this mask.
    pub fn exempt(mut self, role: Role) -> Self {
        if !self.exempt_roles.contains(&role) {
            self.exempt_roles.push(role);
        }
        self
    }

    /// Returns `true` if the given role should have masking applied.
    pub fn should_mask(&self, role: &Role) -> bool {
        // Exempt roles are never masked
        if self.exempt_roles.contains(role) {
            return false;
        }
        match &self.applies_to_roles {
            // None = mask all non-exempt roles
            None => true,
            // Some(roles) = only mask roles in the list
            Some(roles) => roles.contains(role),
        }
    }
}

/// A collection of field masks forming a complete masking policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskingPolicy {
    /// Ordered list of field masks.
    masks: Vec<FieldMask>,
}

impl MaskingPolicy {
    /// Creates an empty masking policy.
    pub fn new() -> Self {
        Self { masks: Vec::new() }
    }

    /// Adds a field mask to the policy.
    pub fn with_mask(mut self, mask: FieldMask) -> Self {
        self.masks.push(mask);
        self
    }

    /// Returns the field mask for the given column, if any.
    pub fn mask_for_column(&self, column: &str) -> Option<&FieldMask> {
        self.masks.iter().find(|m| m.column == column)
    }

    /// Returns all field masks.
    pub fn masks(&self) -> &[FieldMask] {
        &self.masks
    }
}

// ---------------------------------------------------------------------------
// Core masking functions
// ---------------------------------------------------------------------------

/// Applies a masking strategy to a single value.
///
/// If the role is exempt from the mask, the original value is returned
/// unchanged.
///
/// # Errors
///
/// Returns `MaskingError::PatternMismatch` when using `Redact` and the
/// value does not match the expected format.
pub fn apply_mask(value: &[u8], mask: &FieldMask, role: &Role) -> Result<Vec<u8>> {
    // Precondition: mask must reference a non-empty column
    assert!(
        !mask.column.is_empty(),
        "FieldMask column must not be empty"
    );

    // Exempt roles see the original value
    if !mask.should_mask(role) {
        return Ok(value.to_vec());
    }

    let result = match &mask.strategy {
        MaskingStrategy::Redact(pattern) => apply_redact(value, pattern)?,
        MaskingStrategy::Hash => apply_hash(value),
        MaskingStrategy::Tokenize => apply_tokenize(value),
        MaskingStrategy::Truncate { max_chars } => apply_truncate(value, *max_chars),
        MaskingStrategy::Null => apply_null(),
    };

    // Postcondition: Null strategy always returns empty, others return non-empty
    // (unless the input itself was empty for truncate)
    debug_assert!(
        matches!(mask.strategy, MaskingStrategy::Null) || !result.is_empty() || value.is_empty(),
        "Non-null masking strategy should produce non-empty output for non-empty input"
    );

    Ok(result)
}

/// Applies masking to an entire row of values based on the column policy.
///
/// Each element in `row` corresponds to the column at the same index in
/// `columns`. Columns without a matching mask are returned unchanged.
///
/// # Errors
///
/// Returns `MaskingError::ColumnCountMismatch` if `row.len() != columns.len()`.
/// Returns `MaskingError::PatternMismatch` if a redaction pattern fails.
pub fn apply_masks_to_row(
    row: &[Vec<u8>],
    columns: &[String],
    policy: &MaskingPolicy,
    role: &Role,
) -> Result<Vec<Vec<u8>>> {
    // Precondition: row and columns must have matching lengths
    if row.len() != columns.len() {
        return Err(MaskingError::ColumnCountMismatch {
            row_len: row.len(),
            col_len: columns.len(),
        });
    }

    let masked_row: Vec<Vec<u8>> = row
        .iter()
        .zip(columns.iter())
        .map(|(value, col_name)| {
            match policy.mask_for_column(col_name) {
                Some(mask) => apply_mask(value, mask, role),
                None => Ok(value.clone()), // No mask for this column
            }
        })
        .collect::<Result<Vec<_>>>()?;

    // Postcondition: output row has same number of columns as input
    assert_eq!(
        masked_row.len(),
        row.len(),
        "Masked row must have same column count as input"
    );

    Ok(masked_row)
}

// ---------------------------------------------------------------------------
// Strategy implementations
// ---------------------------------------------------------------------------

/// Applies pattern-based redaction.
fn apply_redact(value: &[u8], pattern: &RedactPattern) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(value);

    let redacted = match pattern {
        RedactPattern::Ssn => redact_ssn(&text, pattern)?,
        RedactPattern::Phone => redact_phone(&text, pattern)?,
        RedactPattern::Email => redact_email(&text, pattern)?,
        RedactPattern::CreditCard => redact_credit_card(&text, pattern)?,
        RedactPattern::Custom { replacement } => replacement.clone(),
    };

    Ok(redacted.into_bytes())
}

/// Redacts SSN: `123-45-6789` -> `***-**-6789`.
fn redact_ssn(text: &str, pattern: &RedactPattern) -> Result<String> {
    // Accept both formatted (XXX-XX-XXXX) and unformatted (XXXXXXXXX) SSNs
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();

    if digits.len() != 9 {
        return Err(MaskingError::PatternMismatch {
            pattern: pattern.clone(),
            reason: format!(
                "Expected 9 digits for SSN, found {} in '{text}'",
                digits.len(),
            ),
        });
    }

    let last_four = &digits[5..9];

    // Postcondition: last 4 digits are preserved
    debug_assert_eq!(last_four.len(), 4, "SSN last-four must be 4 digits");

    Ok(format!("***-**-{last_four}"))
}

/// Redacts phone: `555-123-4567` -> `***-***-4567`.
fn redact_phone(text: &str, pattern: &RedactPattern) -> Result<String> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();

    if digits.len() < 10 {
        return Err(MaskingError::PatternMismatch {
            pattern: pattern.clone(),
            reason: format!(
                "Expected at least 10 digits for phone, found {} in '{text}'",
                digits.len(),
            ),
        });
    }

    let last_four = &digits[digits.len() - 4..];

    debug_assert_eq!(last_four.len(), 4, "Phone last-four must be 4 digits");

    Ok(format!("***-***-{last_four}"))
}

/// Redacts email: `john@example.com` -> `j***@example.com`.
fn redact_email(text: &str, pattern: &RedactPattern) -> Result<String> {
    let parts: Vec<&str> = text.splitn(2, '@').collect();

    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(MaskingError::PatternMismatch {
            pattern: pattern.clone(),
            reason: format!("Invalid email format: '{text}'"),
        });
    }

    let first_char = &parts[0][..1];
    let domain = parts[1];

    // Postcondition: domain is preserved
    debug_assert!(!domain.is_empty(), "Email domain must not be empty");

    Ok(format!("{first_char}***@{domain}"))
}

/// Redacts credit card: `1234-5678-9012-3456` -> `****-****-****-3456`.
fn redact_credit_card(text: &str, pattern: &RedactPattern) -> Result<String> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();

    if digits.len() < 13 || digits.len() > 19 {
        return Err(MaskingError::PatternMismatch {
            pattern: pattern.clone(),
            reason: format!(
                "Expected 13-19 digits for credit card, found {} in '{text}'",
                digits.len(),
            ),
        });
    }

    let last_four = &digits[digits.len() - 4..];

    debug_assert_eq!(last_four.len(), 4, "Credit card last-four must be 4 digits");

    Ok(format!("****-****-****-{last_four}"))
}

/// Applies SHA-256 one-way hash, returned as hex-encoded bytes.
fn apply_hash(value: &[u8]) -> Vec<u8> {
    use sha2::Digest;

    let hash = sha2::Sha256::digest(value);
    let hex = bytes_to_hex(&hash);

    // Postcondition: SHA-256 hex is always 64 characters
    debug_assert_eq!(hex.len(), 64, "SHA-256 hex must be 64 characters");

    hex.into_bytes()
}

/// Converts a byte slice to a lowercase hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").expect("writing to String should not fail");
    }
    hex
}

/// Applies deterministic BLAKE3 tokenization.
///
/// Returns `tok_` followed by the first 16 hex characters of the BLAKE3 hash.
fn apply_tokenize(value: &[u8]) -> Vec<u8> {
    let hash = blake3::hash(value);
    let hex = hash.to_hex();
    let token = format!("tok_{}", &hex[..16]);

    // Postcondition: token is always "tok_" (4) + 16 hex chars = 20 chars
    debug_assert_eq!(token.len(), 20, "Token must be exactly 20 characters");

    token.into_bytes()
}

/// Truncates value to `max_chars` characters, padding with `"..."`.
fn apply_truncate(value: &[u8], max_chars: usize) -> Vec<u8> {
    let text = String::from_utf8_lossy(value);

    if text.len() <= max_chars {
        return value.to_vec();
    }

    let truncated: String = text.chars().take(max_chars).collect();
    let result = format!("{truncated}...");

    result.into_bytes()
}

/// Returns empty bytes (null masking).
fn apply_null() -> Vec<u8> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_ssn() {
        let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(Role::User);

        let value = b"123-45-6789";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"***-**-6789");
    }

    #[test]
    fn test_redact_ssn_unformatted() {
        let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(Role::User);

        let value = b"123456789";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"***-**-6789");
    }

    #[test]
    fn test_redact_ssn_invalid() {
        let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(Role::User);

        let value = b"12345";
        let result = apply_mask(value, &mask, &Role::User);
        assert!(result.is_err());
    }

    #[test]
    fn test_redact_email() {
        let mask = FieldMask::new("email", MaskingStrategy::Redact(RedactPattern::Email))
            .applies_to(Role::User);

        let value = b"john@example.com";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"j***@example.com");
    }

    #[test]
    fn test_redact_email_invalid() {
        let mask = FieldMask::new("email", MaskingStrategy::Redact(RedactPattern::Email))
            .applies_to(Role::User);

        let value = b"not-an-email";
        let result = apply_mask(value, &mask, &Role::User);
        assert!(result.is_err());
    }

    #[test]
    fn test_redact_phone() {
        let mask = FieldMask::new("phone", MaskingStrategy::Redact(RedactPattern::Phone))
            .applies_to(Role::User);

        let value = b"555-123-4567";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"***-***-4567");
    }

    #[test]
    fn test_redact_credit_card() {
        let mask = FieldMask::new("cc", MaskingStrategy::Redact(RedactPattern::CreditCard))
            .applies_to(Role::User);

        let value = b"1234-5678-9012-3456";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"****-****-****-3456");
    }

    #[test]
    fn test_redact_custom() {
        let mask = FieldMask::new(
            "secret",
            MaskingStrategy::Redact(RedactPattern::Custom {
                replacement: "[REDACTED]".to_string(),
            }),
        )
        .applies_to(Role::User);

        let value = b"super secret data";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(masked, b"[REDACTED]");
    }

    #[test]
    fn test_hash_deterministic() {
        let mask = FieldMask::new("field", MaskingStrategy::Hash).applies_to(Role::User);

        let value = b"sensitive-data";

        let hash1 = apply_mask(value, &mask, &Role::User).unwrap();
        let hash2 = apply_mask(value, &mask, &Role::User).unwrap();

        // Same input must produce same hash
        assert_eq!(hash1, hash2);

        // Hash output is 64-char hex string
        assert_eq!(hash1.len(), 64);

        // Different input produces different hash
        let different = apply_mask(b"other-data", &mask, &Role::User).unwrap();
        assert_ne!(hash1, different);
    }

    #[test]
    fn test_tokenize() {
        let mask = FieldMask::new("field", MaskingStrategy::Tokenize).applies_to(Role::User);

        let value = b"sensitive-data";
        let token = apply_mask(value, &mask, &Role::User).unwrap();
        let token_str = String::from_utf8(token.clone()).unwrap();

        // Must start with "tok_"
        assert!(token_str.starts_with("tok_"));

        // Total length: "tok_" (4) + 16 hex chars = 20
        assert_eq!(token_str.len(), 20);

        // Deterministic: same input -> same token
        let token2 = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(token, token2);
    }

    #[test]
    fn test_truncate() {
        let mask = FieldMask::new("name", MaskingStrategy::Truncate { max_chars: 3 })
            .applies_to(Role::User);

        let value = b"Jonathan";
        let truncated = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(truncated, b"Jon...");
    }

    #[test]
    fn test_truncate_short_value() {
        let mask = FieldMask::new("name", MaskingStrategy::Truncate { max_chars: 20 })
            .applies_to(Role::User);

        let value = b"Jo";
        let truncated = apply_mask(value, &mask, &Role::User).unwrap();
        // Value is shorter than max_chars, so it's returned unchanged
        assert_eq!(truncated, b"Jo");
    }

    #[test]
    fn test_null_mask() {
        let mask = FieldMask::new("field", MaskingStrategy::Null).applies_to(Role::User);

        let value = b"sensitive-data";
        let masked = apply_mask(value, &mask, &Role::User).unwrap();
        assert!(masked.is_empty());
    }

    #[test]
    fn test_admin_exempt() {
        let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(Role::User)
            .applies_to(Role::Analyst)
            .exempt(Role::Admin);

        let value = b"123-45-6789";

        // Admin is exempt: sees original value
        let admin_result = apply_mask(value, &mask, &Role::Admin).unwrap();
        assert_eq!(admin_result, value);

        // User is not exempt: sees masked value
        let user_result = apply_mask(value, &mask, &Role::User).unwrap();
        assert_eq!(user_result, b"***-**-6789");

        // Analyst is not exempt: sees masked value
        let analyst_result = apply_mask(value, &mask, &Role::Analyst).unwrap();
        assert_eq!(analyst_result, b"***-**-6789");
    }

    #[test]
    fn test_role_not_in_applies_to() {
        let mask = FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
            .applies_to(Role::User);

        let value = b"123-45-6789";

        // Analyst is not in applies_to, so no masking
        let result = apply_mask(value, &mask, &Role::Analyst).unwrap();
        assert_eq!(result, value);
    }

    #[test]
    fn test_apply_masks_to_row() {
        let policy = MaskingPolicy::new()
            .with_mask(
                FieldMask::new("name", MaskingStrategy::Truncate { max_chars: 3 })
                    .applies_to(Role::User),
            )
            .with_mask(
                FieldMask::new("ssn", MaskingStrategy::Redact(RedactPattern::Ssn))
                    .applies_to(Role::User),
            )
            .with_mask(FieldMask::new("notes", MaskingStrategy::Null).applies_to(Role::User));

        let columns = vec![
            "name".to_string(),
            "ssn".to_string(),
            "age".to_string(), // No mask for this column
            "notes".to_string(),
        ];

        let row = vec![
            b"Jonathan".to_vec(),
            b"123-45-6789".to_vec(),
            b"42".to_vec(),
            b"Some private notes".to_vec(),
        ];

        let masked = apply_masks_to_row(&row, &columns, &policy, &Role::User).unwrap();

        assert_eq!(masked.len(), 4);
        assert_eq!(masked[0], b"Jon..."); // Truncated
        assert_eq!(masked[1], b"***-**-6789"); // SSN redacted
        assert_eq!(masked[2], b"42"); // Unmasked (no policy)
        assert!(masked[3].is_empty()); // Null masked
    }

    #[test]
    fn test_apply_masks_to_row_column_mismatch() {
        let policy = MaskingPolicy::new();

        let columns = vec!["a".to_string(), "b".to_string()];
        let row = vec![b"1".to_vec()]; // Only 1 value but 2 columns

        let result = apply_masks_to_row(&row, &columns, &policy, &Role::User);
        assert!(result.is_err());
    }

    #[test]
    fn test_masking_policy_lookup() {
        let policy = MaskingPolicy::new()
            .with_mask(FieldMask::new("ssn", MaskingStrategy::Hash))
            .with_mask(FieldMask::new(
                "email",
                MaskingStrategy::Redact(RedactPattern::Email),
            ));

        assert!(policy.mask_for_column("ssn").is_some());
        assert!(policy.mask_for_column("email").is_some());
        assert!(policy.mask_for_column("name").is_none());
        assert_eq!(policy.masks().len(), 2);
    }

    #[test]
    fn test_should_mask_empty_applies_to() {
        // No applies_to roles means mask applies to all non-exempt roles
        let mask = FieldMask::new("field", MaskingStrategy::Null).exempt(Role::Admin);

        assert!(mask.should_mask(&Role::User));
        assert!(mask.should_mask(&Role::Analyst));
        assert!(mask.should_mask(&Role::Auditor));
        assert!(!mask.should_mask(&Role::Admin)); // Exempt
    }

    #[test]
    #[should_panic(expected = "Column name must not be empty")]
    fn test_empty_column_name_panics() {
        FieldMask::new("", MaskingStrategy::Null);
    }
}
