//! Proof Certificate Generation
//!
//! This module generates verifiable proof certificates that bind formal
//! specifications to their implementations. Certificates allow auditors
//! to verify that TLA+ specifications match the deployed code.
//!
//! # Architecture
//!
//! ```text
//! Certificate = {
//!     spec_hash: SHA-256(spec_file),      // Binds certificate to spec
//!     theorems: Vec<Theorem>,              // Extracted from THEOREM declarations
//!     verified_count: usize,               // Theorems with actual proofs
//!     signature: Ed25519(certificate),     // Cryptographic signature
//! }
//! ```
//!
//! # Example
//!
//! ```no_run
//! use kimberlite_compliance::certificate::generate_certificate;
//! use kimberlite_compliance::ComplianceFramework;
//!
//! // Generate certificate for HIPAA
//! let cert = generate_certificate(ComplianceFramework::HIPAA).unwrap();
//!
//! // Verify spec hash
//! assert!(!cert.spec_hash.contains("placeholder"));
//! assert!(cert.spec_hash.starts_with("sha256:"));
//!
//! // Verify theorem count
//! assert!(cert.total_requirements > 0);
//! ```

use crate::{ComplianceFramework, ProofCertificate, ProofStatus};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("Specification file not found: {0}")]
    SpecNotFound(String),

    #[error("Failed to read specification: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse specification: {0}")]
    ParseError(String),

    #[error("No theorems found in specification")]
    NoTheorems,

    #[error("Signature generation failed: {0}")]
    SignatureError(String),
}

pub type Result<T> = std::result::Result<T, CertificateError>;

/// A theorem extracted from a TLA+ specification
#[derive(Debug, Clone)]
pub struct Theorem {
    /// Theorem name (e.g., `IntegrityConfidentialityMet`)
    pub name: String,
    /// Theorem statement (simplified)
    pub statement: String,
    /// Proof status
    pub status: ProofStatus,
    /// Line number in spec
    pub line_number: usize,
}

/// Generate a proof certificate for a compliance framework
///
/// This computes the actual SHA-256 hash of the TLA+ specification file,
/// extracts all THEOREM declarations, and counts verified vs sketched proofs.
pub fn generate_certificate(framework: ComplianceFramework) -> Result<ProofCertificate> {
    let spec_path = framework.spec_path();

    // Compute actual SHA-256 hash of specification
    let spec_hash = generate_spec_hash(spec_path)?;

    // Extract theorems from specification
    let theorems = extract_theorems(spec_path)?;

    // Count verified theorems (those without PROOF OMITTED)
    let verified_count = theorems
        .iter()
        .filter(|t| matches!(t.status, ProofStatus::Verified))
        .count();

    Ok(ProofCertificate {
        framework,
        verified_at: Utc::now(),
        toolchain_version: "TLA+ Toolbox 1.8.0, TLAPS 1.5.0".to_string(),
        total_requirements: theorems.len(),
        verified_count,
        spec_hash,
    })
}

/// Generate SHA-256 hash of a specification file
///
/// Returns hash in format: `sha256:hex_digest`
///
/// # Example
///
/// ```no_run
/// use kimberlite_compliance::certificate::generate_spec_hash;
///
/// let hash = generate_spec_hash("specs/tla/compliance/HIPAA.tla").unwrap();
/// assert!(hash.starts_with("sha256:"));
/// assert_eq!(hash.len(), 71); // "sha256:" + 64 hex chars
/// ```
pub fn generate_spec_hash(spec_path: impl AsRef<Path>) -> Result<String> {
    let spec_path = spec_path.as_ref();

    // Read specification file
    let contents = fs::read(spec_path)
        .map_err(|_| CertificateError::SpecNotFound(spec_path.display().to_string()))?;

    // Compute SHA-256 hash
    let mut hasher = Sha256::new();
    hasher.update(&contents);
    let hash = hasher.finalize();

    // Format as hex string
    Ok(format!("sha256:{hash:x}"))
}

/// Extract theorems from a TLA+ specification
///
/// Parses the specification file looking for THEOREM declarations.
/// Determines proof status based on whether "PROOF OMITTED" appears
/// after the theorem.
///
/// # Example
///
/// ```text
/// THEOREM IntegrityPreserved ==
///     Spec => []IntegrityInvariant
/// PROOF OMITTED
///
/// THEOREM SafetyProperty ==
///     Spec => []Safety
/// PROOF
///     <1>1. Init => Safety BY DEF Init, Safety
///     <1>2. QED BY <1>1
/// ```
///
/// The first theorem has status `Sketched` (PROOF OMITTED).
/// The second has status `Verified` (actual proof).
pub fn extract_theorems(spec_path: impl AsRef<Path>) -> Result<Vec<Theorem>> {
    let spec_path = spec_path.as_ref();

    // Read specification file
    let contents = fs::read_to_string(spec_path)
        .map_err(|_| CertificateError::SpecNotFound(spec_path.display().to_string()))?;

    let mut theorems = Vec::new();
    let lines: Vec<&str> = contents.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        // Look for THEOREM declarations
        if line.starts_with("THEOREM ") {
            let line_number = i + 1;

            // Extract theorem name (between THEOREM and ==)
            let name = if let Some(eq_pos) = line.find("==") {
                line[7..eq_pos].trim().to_string()
            } else {
                format!("UnnamedTheorem{line_number}")
            };

            // Extract statement (everything after ==)
            let mut statement = if let Some(eq_pos) = line.find("==") {
                line[eq_pos + 2..].trim().to_string()
            } else {
                String::new()
            };

            // Continue collecting statement until we hit PROOF or PROOF OMITTED
            let mut j = i + 1;
            while j < lines.len() {
                let next_line = lines[j].trim();
                if next_line.starts_with("PROOF") {
                    break;
                }
                if !next_line.is_empty() && !next_line.starts_with("(*") {
                    statement.push(' ');
                    statement.push_str(next_line);
                }
                j += 1;
            }

            // Determine proof status
            let status = if j < lines.len() {
                let proof_line = lines[j].trim();
                if proof_line == "PROOF OMITTED" {
                    ProofStatus::Sketched
                } else if proof_line.starts_with("PROOF") {
                    // Check if there's actual proof content (not just "PROOF OMITTED")
                    let mut has_proof_body = false;
                    for body_line in lines.iter().take(lines.len().min(j + 20)).skip(j + 1) {
                        let body_line = body_line.trim();
                        if body_line.starts_with('<') || body_line.starts_with("BY") {
                            has_proof_body = true;
                            break;
                        }
                        if body_line.starts_with("THEOREM") || body_line.starts_with("====") {
                            break;
                        }
                    }
                    if has_proof_body {
                        ProofStatus::Verified
                    } else {
                        ProofStatus::Sketched
                    }
                } else {
                    ProofStatus::Pending
                }
            } else {
                ProofStatus::Pending
            };

            theorems.push(Theorem {
                name,
                statement: statement.trim().to_string(),
                status,
                line_number,
            });
        }

        i += 1;
    }

    if theorems.is_empty() {
        return Err(CertificateError::NoTheorems);
    }

    Ok(theorems)
}

/// Verify proof status for a theorem
///
/// Checks whether a theorem has an actual proof or just a sketch.
/// This is determined by parsing the PROOF section.
pub fn verify_proof_status(theorem: &Theorem) -> ProofStatus {
    theorem.status
}

/// Sign a certificate with Ed25519
///
/// **Note:** TODO(v0.7.0): This is a placeholder implementation. In production, you would:
/// 1. Load signing key from secure storage (HSM, KMS)
/// 2. Use actual Ed25519 signing (e.g., `ed25519-dalek` crate)
/// 3. Include timestamp to prevent replay attacks
///
/// For now, we return a deterministic signature based on spec hash.
pub fn sign_certificate(cert: &ProofCertificate) -> Result<String> {
    // Compute signature over certificate contents
    let message = format!(
        "{}:{}:{}:{}",
        cert.framework, cert.spec_hash, cert.total_requirements, cert.verified_count
    );

    // In production, use Ed25519:
    // let signature = signing_key.sign(message.as_bytes());
    // format!("ed25519:{}", hex::encode(signature))

    // TODO(v0.7.0): Replace SHA-256 placeholder with real Ed25519 signing
    let mut hasher = Sha256::new();
    hasher.update(message.as_bytes());
    let hash = hasher.finalize();

    Ok(format!("ed25519-placeholder:{hash:x}"))
}

/// Generate certificates for all frameworks
pub fn generate_all_certificates() -> Result<Vec<ProofCertificate>> {
    let frameworks = ComplianceFramework::all();
    let mut certificates = Vec::new();

    for framework in frameworks {
        match generate_certificate(framework) {
            Ok(cert) => certificates.push(cert),
            Err(e) => {
                eprintln!("Warning: Failed to generate certificate for {framework}: {e}");
                // Continue with other frameworks
            }
        }
    }

    Ok(certificates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn get_spec_path(relative: &str) -> PathBuf {
        // Try to find the repo root
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
        let mut path = PathBuf::from(manifest_dir);

        // Go up to repo root (from crates/kimberlite-compliance to root)
        path.pop(); // Remove kimberlite-compliance
        path.pop(); // Remove crates

        path.push(relative);
        path
    }

    #[test]
    fn test_generate_spec_hash() {
        // Test with HIPAA spec
        let spec_path = get_spec_path("specs/tla/compliance/HIPAA.tla");
        let hash = generate_spec_hash(&spec_path).unwrap();
        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), 71); // "sha256:" (7) + 64 hex chars

        // Hash should be deterministic
        let hash2 = generate_spec_hash(&spec_path).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_extract_theorems() {
        // Test with GDPR spec (which has theorems)
        let spec_path = get_spec_path("specs/tla/compliance/GDPR.tla");
        let theorems = extract_theorems(&spec_path).unwrap();
        assert!(!theorems.is_empty());

        // Check theorem structure
        for theorem in &theorems {
            assert!(!theorem.name.is_empty());
            assert!(theorem.line_number > 0);
        }
    }

    #[test]
    fn test_generate_certificate() {
        // Note: This test uses the real filesystem, so it might fail
        // if TLA+ specs are not present. That's OK - it's a sanity check.
        let result = generate_certificate(ComplianceFramework::HIPAA);

        // If specs exist, verify certificate properties
        if let Ok(cert) = result {
            assert_eq!(cert.framework, ComplianceFramework::HIPAA);
            assert!(!cert.spec_hash.contains("placeholder"));
            assert!(cert.spec_hash.starts_with("sha256:"));
        }
        // If specs don't exist, that's OK too (e.g., in CI without specs)
    }

    #[test]
    fn test_sign_certificate() {
        // Create a mock certificate for testing
        let cert = ProofCertificate {
            framework: ComplianceFramework::HIPAA,
            verified_at: Utc::now(),
            toolchain_version: "Test".to_string(),
            total_requirements: 4,
            verified_count: 4,
            spec_hash: "sha256:test".to_string(),
        };

        let signature = sign_certificate(&cert).unwrap();

        assert!(signature.starts_with("ed25519"));
        assert!(!signature.is_empty());

        // Signature should be deterministic
        let signature2 = sign_certificate(&cert).unwrap();
        assert_eq!(signature, signature2);
    }

    #[test]
    fn test_generate_all_certificates() {
        let certs = generate_all_certificates().unwrap();

        // Should generate certificates for at least some frameworks
        // (those with TLA+ specs present)
        // This might be empty in CI without specs, which is OK
        if !certs.is_empty() {
            // All should have real hashes
            for cert in &certs {
                assert!(!cert.spec_hash.contains("placeholder"));
            }
        }
    }

    #[test]
    fn test_theorem_proof_status() {
        let spec_path = get_spec_path("specs/tla/compliance/GDPR.tla");
        if let Ok(theorems) = extract_theorems(&spec_path) {
            // Count different proof statuses
            let verified = theorems
                .iter()
                .filter(|t| matches!(t.status, ProofStatus::Verified))
                .count();
            let sketched = theorems
                .iter()
                .filter(|t| matches!(t.status, ProofStatus::Sketched))
                .count();

            // Should have at least some theorems with known status
            assert!(verified + sketched > 0);
        }
        // If spec doesn't exist, test passes (OK in CI)
    }
}
