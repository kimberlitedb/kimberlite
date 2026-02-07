//! Qualified timestamping for eIDAS compliance.
//!
//! Implements RFC 3161 Timestamp Protocol types for qualified timestamping
//! from a Qualified Trust Service Provider (QTSP). Follows FCIS: the pure
//! core validates timestamp tokens, the impure shell makes HTTP requests to
//! the TSP.
//!
//! # eIDAS Requirements
//!
//! - Article 42: Qualified timestamps shall bind data to a particular time,
//!   establishing evidence that the data existed at that time.
//! - The timestamp must be issued by a Qualified Trust Service Provider
//!   on the EU Trusted List.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a timestamp request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimestampStatus {
    /// Timestamp successfully issued by TSP.
    Granted,
    /// Request rejected by TSP.
    Rejected,
    /// Waiting for TSP response.
    Pending,
}

/// A timestamp request per RFC 3161 § 2.4.
///
/// Contains the hash of the datum to be timestamped. The hash algorithm
/// must match what the TSP accepts (typically SHA-256).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampRequest {
    /// SHA-256 hash of the data to timestamp.
    pub message_imprint: Vec<u8>,
    /// OID of the hash algorithm (e.g., "2.16.840.1.101.3.4.2.1" for SHA-256).
    pub hash_algorithm: String,
    /// Optional nonce for replay protection.
    pub nonce: Option<u64>,
    /// Whether to request the TSP's certificate in the response.
    pub cert_req: bool,
}

impl TimestampRequest {
    /// Creates a new SHA-256 timestamp request.
    pub fn sha256(message_imprint: Vec<u8>) -> Self {
        assert_eq!(
            message_imprint.len(),
            32,
            "SHA-256 hash must be exactly 32 bytes, got {}",
            message_imprint.len()
        );
        Self {
            message_imprint,
            hash_algorithm: "2.16.840.1.101.3.4.2.1".to_string(),
            nonce: None,
            cert_req: true,
        }
    }

    /// Sets the nonce for replay protection.
    pub fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }
}

/// A timestamp token per RFC 3161 § 2.4.2.
///
/// Issued by a Qualified Trust Service Provider. Contains the signed
/// timestamp binding the data hash to a specific time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampToken {
    /// The original message imprint that was timestamped.
    pub message_imprint: Vec<u8>,
    /// The timestamp assigned by the TSP.
    pub gen_time: DateTime<Utc>,
    /// Serial number assigned by the TSP (unique per TSP).
    pub serial_number: String,
    /// The TSP that issued this token (URL or name).
    pub tsa_name: String,
    /// The DER-encoded CMS `SignedData` containing the timestamp.
    pub token_bytes: Vec<u8>,
    /// Status of this timestamp.
    pub status: TimestampStatus,
    /// Nonce from the request (for matching responses).
    pub nonce: Option<u64>,
}

impl TimestampToken {
    /// Validates that this token matches the given request.
    ///
    /// Checks:
    /// - Message imprint matches
    /// - Nonce matches (if present in request)
    /// - Status is Granted
    /// - Token bytes are non-empty
    pub fn validate_against_request(&self, request: &TimestampRequest) -> Result<(), String> {
        if self.status != TimestampStatus::Granted {
            return Err(format!(
                "timestamp status is {:?}, expected Granted",
                self.status
            ));
        }
        if self.message_imprint != request.message_imprint {
            return Err("message imprint mismatch".to_string());
        }
        if let Some(req_nonce) = request.nonce {
            match self.nonce {
                Some(tok_nonce) if tok_nonce == req_nonce => {}
                Some(tok_nonce) => {
                    return Err(format!(
                        "nonce mismatch: request={req_nonce}, token={tok_nonce}"
                    ));
                }
                None => return Err("request included nonce but token did not".to_string()),
            }
        }
        if self.token_bytes.is_empty() {
            return Err("token_bytes is empty".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_request_sha256() {
        let hash = vec![0u8; 32];
        let req = TimestampRequest::sha256(hash.clone());
        assert_eq!(req.message_imprint, hash);
        assert_eq!(req.hash_algorithm, "2.16.840.1.101.3.4.2.1");
        assert!(req.cert_req);
        assert!(req.nonce.is_none());
    }

    #[test]
    fn test_timestamp_request_with_nonce() {
        let hash = vec![0u8; 32];
        let req = TimestampRequest::sha256(hash).with_nonce(42);
        assert_eq!(req.nonce, Some(42));
    }

    #[test]
    #[should_panic(expected = "SHA-256 hash must be exactly 32 bytes")]
    fn test_timestamp_request_invalid_hash_length() {
        TimestampRequest::sha256(vec![0u8; 16]);
    }

    #[test]
    fn test_validate_matching_token() {
        let hash = vec![1u8; 32];
        let req = TimestampRequest::sha256(hash.clone()).with_nonce(99);
        let token = TimestampToken {
            message_imprint: hash,
            gen_time: Utc::now(),
            serial_number: "TSP-001".to_string(),
            tsa_name: "Example QTSP".to_string(),
            token_bytes: vec![0xDE, 0xAD],
            status: TimestampStatus::Granted,
            nonce: Some(99),
        };
        assert!(token.validate_against_request(&req).is_ok());
    }

    #[test]
    fn test_validate_rejected_token() {
        let hash = vec![1u8; 32];
        let req = TimestampRequest::sha256(hash.clone());
        let token = TimestampToken {
            message_imprint: hash,
            gen_time: Utc::now(),
            serial_number: "TSP-002".to_string(),
            tsa_name: "Example QTSP".to_string(),
            token_bytes: vec![0xDE],
            status: TimestampStatus::Rejected,
            nonce: None,
        };
        let result = token.validate_against_request(&req);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Rejected"));
    }

    #[test]
    fn test_validate_nonce_mismatch() {
        let hash = vec![1u8; 32];
        let req = TimestampRequest::sha256(hash.clone()).with_nonce(42);
        let token = TimestampToken {
            message_imprint: hash,
            gen_time: Utc::now(),
            serial_number: "TSP-003".to_string(),
            tsa_name: "Example QTSP".to_string(),
            token_bytes: vec![0xDE],
            status: TimestampStatus::Granted,
            nonce: Some(99),
        };
        let result = token.validate_against_request(&req);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonce mismatch"));
    }

    #[test]
    fn test_validate_imprint_mismatch() {
        let req = TimestampRequest::sha256(vec![1u8; 32]);
        let token = TimestampToken {
            message_imprint: vec![2u8; 32],
            gen_time: Utc::now(),
            serial_number: "TSP-004".to_string(),
            tsa_name: "Example QTSP".to_string(),
            token_bytes: vec![0xDE],
            status: TimestampStatus::Granted,
            nonce: None,
        };
        let result = token.validate_against_request(&req);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("imprint mismatch"));
    }
}
