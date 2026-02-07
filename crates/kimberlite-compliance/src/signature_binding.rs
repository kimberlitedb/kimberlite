//! Electronic signature binding for 21 CFR Part 11.
//!
//! Provides per-record Ed25519 signature linking — each electronic signature is
//! cryptographically bound to the record content, signer identity, and meaning.
//! Signatures are non-transferable and tamper-evident.
//!
//! # Signature Meanings (per 11.50)
//!
//! - **Authorship**: The signer created or authored the record
//! - **Review**: The signer has reviewed and verified the record
//! - **Approval**: The signer has approved the record for release
//!
//! # Operational Sequencing (per 11.10)
//!
//! Records requiring approval must follow: Authorship → Review → Approval.
//! The `validate_sequence` function enforces this ordering.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The meaning associated with an electronic signature per 21 CFR Part 11 § 11.50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignatureMeaning {
    /// The signer authored or created the record.
    Authorship,
    /// The signer reviewed and verified the record.
    Review,
    /// The signer approved the record for release.
    Approval,
}

impl std::fmt::Display for SignatureMeaning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authorship => write!(f, "Authorship"),
            Self::Review => write!(f, "Review"),
            Self::Approval => write!(f, "Approval"),
        }
    }
}

/// A cryptographically bound electronic signature on a record.
///
/// Once created, the signature binds the signer's identity, the record content
/// hash, and the signature meaning into an immutable unit. The Ed25519 signature
/// covers `record_hash || signer_id || meaning`, preventing signature transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordSignature {
    /// Unique identifier for this signature.
    pub signature_id: String,
    /// The record this signature is bound to (content hash).
    pub record_hash: Vec<u8>,
    /// Identity of the signer.
    pub signer_id: String,
    /// The meaning of this signature.
    pub meaning: SignatureMeaning,
    /// When the signature was applied.
    pub signed_at: DateTime<Utc>,
    /// The Ed25519 signature bytes (64 bytes).
    pub signature_bytes: Vec<u8>,
}

impl RecordSignature {
    /// Creates a new record signature.
    ///
    /// The caller is responsible for computing the Ed25519 signature externally
    /// (FCIS: pure core receives the signature, impure shell creates it).
    pub fn new(
        signature_id: String,
        record_hash: Vec<u8>,
        signer_id: String,
        meaning: SignatureMeaning,
        signed_at: DateTime<Utc>,
        signature_bytes: Vec<u8>,
    ) -> Self {
        assert!(!record_hash.is_empty(), "record_hash must not be empty");
        assert!(!signer_id.is_empty(), "signer_id must not be empty");
        assert_eq!(
            signature_bytes.len(),
            64,
            "Ed25519 signature must be exactly 64 bytes, got {}",
            signature_bytes.len()
        );

        Self {
            signature_id,
            record_hash,
            signer_id,
            meaning,
            signed_at,
            signature_bytes,
        }
    }
}

/// Validates that a sequence of signature meanings follows the required
/// operational sequencing: Authorship → Review → Approval.
///
/// Rules:
/// - Authorship must come before Review
/// - Review must come before Approval
/// - Multiple Reviews are allowed between Authorship and Approval
/// - An empty sequence is valid (no signatures yet)
/// - A single Authorship is valid (record authored but not yet reviewed)
pub fn validate_sequence(meanings: &[SignatureMeaning]) -> bool {
    if meanings.is_empty() {
        return true;
    }

    // Track the highest stage reached: 0=Authorship, 1=Review, 2=Approval
    let mut max_stage = 0u8;

    for meaning in meanings {
        let stage = match meaning {
            SignatureMeaning::Authorship => 0,
            SignatureMeaning::Review => 1,
            SignatureMeaning::Approval => 2,
        };

        // Each stage must be >= the previous (monotonic progression)
        // Exception: multiple reviews are allowed (stage 1 -> stage 1)
        if stage < max_stage {
            return false;
        }
        max_stage = stage;
    }

    // If we have Approval, we must also have at least one Review
    if max_stage == 2 && !meanings.contains(&SignatureMeaning::Review) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_full_sequence() {
        assert!(validate_sequence(&[
            SignatureMeaning::Authorship,
            SignatureMeaning::Review,
            SignatureMeaning::Approval,
        ]));
    }

    #[test]
    fn test_valid_author_only() {
        assert!(validate_sequence(&[SignatureMeaning::Authorship]));
    }

    #[test]
    fn test_valid_author_review() {
        assert!(validate_sequence(&[
            SignatureMeaning::Authorship,
            SignatureMeaning::Review,
        ]));
    }

    #[test]
    fn test_valid_multiple_reviews() {
        assert!(validate_sequence(&[
            SignatureMeaning::Authorship,
            SignatureMeaning::Review,
            SignatureMeaning::Review,
            SignatureMeaning::Approval,
        ]));
    }

    #[test]
    fn test_valid_empty() {
        assert!(validate_sequence(&[]));
    }

    #[test]
    fn test_invalid_approval_without_review() {
        assert!(!validate_sequence(&[
            SignatureMeaning::Authorship,
            SignatureMeaning::Approval,
        ]));
    }

    #[test]
    fn test_invalid_review_before_authorship() {
        assert!(!validate_sequence(&[
            SignatureMeaning::Review,
            SignatureMeaning::Authorship,
        ]));
    }

    #[test]
    fn test_invalid_approval_before_review() {
        assert!(!validate_sequence(&[
            SignatureMeaning::Approval,
            SignatureMeaning::Review,
        ]));
    }

    #[test]
    fn test_signature_creation() {
        let sig = RecordSignature::new(
            "sig-001".to_string(),
            vec![1, 2, 3, 4],
            "dr-smith".to_string(),
            SignatureMeaning::Authorship,
            Utc::now(),
            vec![0u8; 64],
        );
        assert_eq!(sig.meaning, SignatureMeaning::Authorship);
        assert_eq!(sig.signer_id, "dr-smith");
    }

    #[test]
    fn test_signature_meaning_display() {
        assert_eq!(SignatureMeaning::Authorship.to_string(), "Authorship");
        assert_eq!(SignatureMeaning::Review.to_string(), "Review");
        assert_eq!(SignatureMeaning::Approval.to_string(), "Approval");
    }
}
