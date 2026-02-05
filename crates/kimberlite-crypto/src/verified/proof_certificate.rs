//! Proof Certificates from Coq Verification
//!
//! This module defines proof certificates that are embedded in verified
//! cryptographic implementations. Each certificate documents that the
//! implementation satisfies formal specifications proven in Coq.
//!
//! Certificates are extracted from Coq ProofCertificate records and
//! embedded as compile-time constants in Rust code.

use std::fmt;

/// Proof certificate documenting a formally verified theorem
///
/// This is extracted from the Coq ProofCertificate record type defined
/// in `specs/coq/Common.v`. Each verified function includes its certificate,
/// enabling:
/// - Audit trail of which theorems apply to the implementation
/// - Runtime verification that code matches specifications
/// - Compliance documentation generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProofCertificate {
    /// Unique theorem identifier
    ///
    /// IDs are allocated in ranges:
    /// - 100-199: SHA-256 theorems
    /// - 200-299: BLAKE3 theorems
    /// - 300-399: AES-GCM theorems
    /// - 400-499: Ed25519 theorems
    /// - 500-599: Key hierarchy theorems
    pub theorem_id: u32,

    /// Proof system identifier
    ///
    /// - 1: Coq 8.18
    /// - 2: TLAPS (TLA+ Proof System)
    /// - 3: Ivy
    /// - 4: Alloy
    pub proof_system_id: u32,

    /// Verification date (YYYYMMDD format)
    ///
    /// Example: 20260205 = February 5, 2026
    pub verified_at: u32,

    /// Number of computational assumptions (axioms)
    ///
    /// Lower is better. Zero assumptions means fully proven.
    pub assumption_count: u32,
}

impl ProofCertificate {
    /// Create a new proof certificate
    pub const fn new(
        theorem_id: u32,
        proof_system_id: u32,
        verified_at: u32,
        assumption_count: u32,
    ) -> Self {
        Self {
            theorem_id,
            proof_system_id,
            verified_at,
            assumption_count,
        }
    }

    /// Get the proof system name
    pub fn proof_system_name(&self) -> &'static str {
        match self.proof_system_id {
            1 => "Coq 8.18",
            2 => "TLAPS",
            3 => "Ivy",
            4 => "Alloy",
            _ => "Unknown",
        }
    }

    /// Get the verification year
    pub fn verification_year(&self) -> u32 {
        self.verified_at / 10000
    }

    /// Check if the proof is complete (no assumptions)
    pub fn is_complete(&self) -> bool {
        self.assumption_count == 0
    }
}

impl fmt::Display for ProofCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProofCertificate {{ theorem_id: {}, proof_system: {}, verified: {}, assumptions: {} }}",
            self.theorem_id,
            self.proof_system_name(),
            self.verified_at,
            self.assumption_count
        )
    }
}

/// Trait for types that carry proof certificates
pub trait Verified {
    /// Get the proof certificate for this implementation
    fn proof_certificate() -> ProofCertificate;

    /// Get the theorem name
    fn theorem_name() -> &'static str;

    /// Get a human-readable description of what was proven
    fn theorem_description() -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_certificate_display() {
        let cert = ProofCertificate::new(100, 1, 20260205, 1);
        let display = format!("{}", cert);
        assert!(display.contains("theorem_id: 100"));
        assert!(display.contains("Coq 8.18"));
        assert!(display.contains("20260205"));
        assert!(display.contains("assumptions: 1"));
    }

    #[test]
    fn test_proof_system_name() {
        let cert = ProofCertificate::new(100, 1, 20260205, 0);
        assert_eq!(cert.proof_system_name(), "Coq 8.18");
    }

    #[test]
    fn test_verification_year() {
        let cert = ProofCertificate::new(100, 1, 20260205, 0);
        assert_eq!(cert.verification_year(), 2026);
    }

    #[test]
    fn test_is_complete() {
        let complete = ProofCertificate::new(100, 1, 20260205, 0);
        assert!(complete.is_complete());

        let partial = ProofCertificate::new(100, 1, 20260205, 2);
        assert!(!partial.is_complete());
    }
}
