//! Simple PEM file parser for certificates and private keys.
//!
//! PEM (Privacy Enhanced Mail) format is a base64-encoded DER format with
//! header/footer markers. This module provides minimal parsing for TLS certificates
//! and private keys, following RFC 7468.

use base64::prelude::*;

/// PEM parsing error.
#[derive(Debug, thiserror::Error)]
pub enum PemError {
    #[error("invalid PEM format: {0}")]
    InvalidFormat(String),
    #[error("base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    #[error("no PEM blocks found")]
    NoPemBlocks,
}

/// A parsed PEM block with its label and binary content.
#[derive(Debug)]
pub struct PemBlock {
    /// The label from the BEGIN line (e.g., "CERTIFICATE", "PRIVATE KEY")
    pub label: String,
    /// The decoded binary content
    pub contents: Vec<u8>,
}

/// Parse PEM-encoded data and return all PEM blocks found.
///
/// Handles the standard PEM format:
/// ```text
/// -----BEGIN LABEL-----
/// base64data
/// -----END LABEL-----
/// ```
pub fn parse_pem(input: &[u8]) -> Result<Vec<PemBlock>, PemError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| PemError::InvalidFormat("not valid UTF-8".to_string()))?;

    let mut blocks = Vec::new();
    let mut lines = text.lines();

    while let Some(line) = lines.next() {
        let line = line.trim();

        // Look for BEGIN marker
        if let Some(label) = line.strip_prefix("-----BEGIN ").and_then(|s| s.strip_suffix("-----")) {
            let label = label.to_string();
            let mut base64_data = String::new();

            // Collect base64 data until END marker
            for line in lines.by_ref() {
                let line = line.trim();
                if let Some(end_label) = line.strip_prefix("-----END ").and_then(|s| s.strip_suffix("-----")) {
                    if end_label != label {
                        return Err(PemError::InvalidFormat(format!(
                            "mismatched PEM markers: BEGIN {label} but END {end_label}"
                        )));
                    }
                    // Decode the base64 data
                    let contents = BASE64_STANDARD.decode(base64_data.as_bytes())?;
                    blocks.push(PemBlock { label, contents });
                    break;
                }
                // Accumulate base64 data (skip empty lines and whitespace)
                if !line.is_empty() {
                    base64_data.push_str(line);
                }
            }
        }
    }

    if blocks.is_empty() {
        return Err(PemError::NoPemBlocks);
    }

    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_certificate() {
        let pem = b"-----BEGIN CERTIFICATE-----
MIIBkTCB+wIJAKHHCgVZU2W9MA0GCSqGSIb3DQEBCwUAMBMxETAPBgNVBAMMCGxv
Y2FsaG9zdDAeFw0yMTAxMDEwMDAwMDBaFw0yMjAxMDEwMDAwMDBaMBMxETAPBgNV
BAMMCGxvY2FsaG9zdA==
-----END CERTIFICATE-----";

        let blocks = parse_pem(pem).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].label, "CERTIFICATE");
        assert!(!blocks[0].contents.is_empty());
    }

    #[test]
    fn test_parse_multiple_certificates() {
        let pem = b"-----BEGIN CERTIFICATE-----
VGVzdERhdGExMjM0
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
QW5vdGhlckRhdGE=
-----END CERTIFICATE-----";

        let blocks = parse_pem(pem).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].label, "CERTIFICATE");
        assert_eq!(blocks[1].label, "CERTIFICATE");
        assert_eq!(blocks[0].contents, b"TestData1234");
        assert_eq!(blocks[1].contents, b"AnotherData");
    }

    #[test]
    fn test_parse_private_key() {
        let pem = b"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDW
-----END PRIVATE KEY-----";

        let blocks = parse_pem(pem).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].label, "PRIVATE KEY");
    }

    #[test]
    fn test_mismatched_markers() {
        let pem = b"-----BEGIN CERTIFICATE-----
data
-----END PRIVATE KEY-----";

        assert!(matches!(
            parse_pem(pem),
            Err(PemError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_no_pem_blocks() {
        let pem = b"just some random text";
        assert!(matches!(parse_pem(pem), Err(PemError::NoPemBlocks)));
    }

    #[test]
    fn test_invalid_base64() {
        let pem = b"-----BEGIN CERTIFICATE-----
not!valid@base64#data$
-----END CERTIFICATE-----";

        assert!(matches!(parse_pem(pem), Err(PemError::Base64Decode(_))));
    }
}
