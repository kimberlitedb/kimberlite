//! # vdb-crypto: Cryptographic primitives for `VerityDB`
//!
//! This crate provides the cryptographic foundation for `VerityDB`'s
//! tamper-evident append-only log.
//!
//! ## Modules
//!
//! | Module | Purpose | Status |
//! |--------|---------|--------|
//! | [`chain`] | Hash chains for tamper evidence (SHA-256) | ✅ Ready |
//! | [`hash`] | Dual-hash abstraction (SHA-256/BLAKE3) | ✅ Ready |
//! | [`signature`] | Ed25519 signatures for non-repudiation | ✅ Ready |
//! | `encryption` | Envelope encryption for tenant isolation | 🚧 Stub (not yet implemented) |
//!
//! ## Quick Start
//!
//! ```
//! use vdb_crypto::{chain_hash, ChainHash, SigningKey, internal_hash, HashPurpose};
//!
//! // Build a tamper-evident chain of records (SHA-256 for compliance)
//! let hash0 = chain_hash(None, b"genesis record");
//! let hash1 = chain_hash(Some(&hash0), b"second record");
//!
//! // Fast internal hash (BLAKE3) for deduplication
//! let fingerprint = internal_hash(b"content to deduplicate");
//!
//! // Sign records for non-repudiation
//! let signing_key = SigningKey::generate();
//! let signature = signing_key.sign(hash1.as_bytes());
//!
//! // Verify the signature
//! let verifying_key = signing_key.verifying_key();
//! assert!(verifying_key.verify(hash1.as_bytes(), &signature).is_ok());
//! ```
//!
//! ## Planned Features
//!
//! - **Envelope Encryption**: Per-tenant data encryption with key rotation

pub mod chain;
pub mod encryption;
pub mod error;
pub mod hash;
pub mod signature;

// Re-export primary types at crate root for convenience
pub use chain::{chain_hash, ChainHash, HASH_LENGTH};
pub use error::CryptoError;
pub use hash::{hash_with_purpose, internal_hash, HashAlgorithm, HashPurpose, InternalHash};
pub use signature::{Signature, SigningKey, VerifyingKey};
