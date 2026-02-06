//! Verified Cryptographic Implementations
//!
//! This module provides cryptographic primitives with formal verification
//! guarantees from Coq proof assistant. Each implementation includes embedded
//! proof certificates documenting which theorems were proven.
//!
//! # Overview
//!
//! All implementations wrap well-tested cryptographic libraries (sha2, blake3,
//! aes-gcm, ed25519-dalek) with formal specifications proven in Coq. The proofs
//! establish properties like:
//! - Determinism (same input → same output)
//! - Non-degeneracy (no degenerate outputs)
//! - Correctness (encryption roundtrips, signature verification)
//! - Security (collision resistance, unforgeability)
//!
//! # Formal Verification
//!
//! All specifications are defined in `specs/coq/` and verified using Coq 8.18:
//! - `Common.v` - Shared definitions and proof infrastructure
//! - `SHA256.v` - SHA-256 hash function (6 theorems)
//! - `BLAKE3.v` - BLAKE3 tree hashing (6 theorems)
//! - `AES_GCM.v` - AES-256-GCM authenticated encryption (4 theorems)
//! - `Ed25519.v` - Ed25519 digital signatures (5 theorems)
//! - `KeyHierarchy.v` - 3-level key hierarchy (9 theorems)
//!
//! Run verification:
//! ```bash
//! ./scripts/verify_coq.sh  # All 6 files must pass
//! ```
//!
//! # Feature Flag
//!
//! Enable with `verified-crypto` feature:
//! ```toml
//! [dependencies]
//! kimberlite-crypto = { version = "0.2", features = ["verified-crypto"] }
//! ```
//!
//! # Usage Example
//!
//! ```rust
//! use kimberlite_crypto::verified::{VerifiedSha256, Verified};
//!
//! // Hash with determinism proof
//! let hash = VerifiedSha256::hash(b"data");
//!
//! // View proof certificate
//! let cert = VerifiedSha256::proof_certificate();
//! println!("Theorem: {}", VerifiedSha256::theorem_name());
//! println!("Verified: {}", cert.verified_at);
//! println!("Complete proof: {}", cert.is_complete());
//! ```

pub mod aes_gcm;
pub mod blake3;
pub mod ed25519;
pub mod key_hierarchy;
pub mod proof_certificate;
pub mod sha256;

// Re-export main types
pub use aes_gcm::VerifiedAesGcm;
pub use blake3::{VerifiedBlake3, VerifiedBlake3Hasher};
pub use ed25519::{VerifiedSignature, VerifiedSigningKey, VerifiedVerifyingKey};
pub use key_hierarchy::{VerifiedDEK, VerifiedKEK, VerifiedMasterKey, VerifiedWrappedDEK};
pub use proof_certificate::{ProofCertificate, Verified};
pub use sha256::VerifiedSha256;
