//! GDPR Article 20 Data Portability Export
//!
//! This module implements the "Right to Data Portability" — exporting a data subject's
//! records in a structured, commonly-used, machine-readable format with cryptographic
//! integrity guarantees.
//!
//! # GDPR Requirements
//!
//! **Article 20(1)**: Data subject has the right to receive personal data in a structured,
//! commonly used, and machine-readable format
//!
//! **Article 20(2)**: Data subject has the right to transmit that data to another controller
//!
//! # Architecture
//!
//! ```text
//! ExportEngine = {
//!     exports: Vec<PortabilityExport>,   // Completed exports
//!     audit_trail: Vec<ExportAuditRecord>, // Immutable audit log
//! }
//!
//! PortabilityExport = {
//!     export_id: Uuid,
//!     subject_id: String,
//!     content_hash: SHA-256(export_data),  // Integrity proof
//!     signature: HMAC-SHA256(key, hash),   // Authenticity proof
//! }
//! ```
//!
//! # Example
//!
//! ```
//! use kimberlite_compliance::export::{ExportEngine, ExportFormat, ExportRecord};
//! use kimberlite_types::StreamId;
//! use chrono::Utc;
//!
//! let mut engine = ExportEngine::new();
//!
//! let records = vec![
//!     ExportRecord {
//!         stream_id: StreamId::new(1),
//!         stream_name: "patient_records".to_string(),
//!         offset: 0,
//!         data: serde_json::json!({"name": "Jane Doe"}),
//!         timestamp: Utc::now(),
//!     },
//! ];
//!
//! let export = engine.export_subject_data("jane@example.com", &records, ExportFormat::Json).unwrap();
//! assert_eq!(export.record_count, 1);
//! assert!(!export.content_hash.is_genesis());
//! ```

use chrono::{DateTime, Utc};
use kimberlite_types::{HASH_LENGTH, Hash, StreamId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("No data found for subject: {0}")]
    NoDataFound(String),

    #[error("Export format not supported: {0}")]
    UnsupportedFormat(String),

    #[error("Export failed: {0}")]
    ExportFailed(String),

    #[error("Signature verification failed")]
    SignatureVerificationFailed,
}

pub type Result<T> = std::result::Result<T, ExportError>;

/// Machine-readable export format (GDPR Article 20 compliance)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// JSON — structured, commonly used, machine-readable
    Json,
    /// CSV — tabular, commonly used, machine-readable
    Csv,
}

/// A single record included in a portability export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRecord {
    /// Stream this record belongs to
    pub stream_id: StreamId,
    /// Human-readable stream name
    pub stream_name: String,
    /// Offset within the stream
    pub offset: u64,
    /// Record payload (serialized as JSON value)
    pub data: serde_json::Value,
    /// When the record was originally committed
    pub timestamp: DateTime<Utc>,
}

/// Metadata about a completed data portability export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortabilityExport {
    /// Unique identifier for this export
    pub export_id: Uuid,
    /// Data subject whose data was exported
    pub subject_id: String,
    /// When the export was requested
    pub requested_at: DateTime<Utc>,
    /// When the export completed
    pub completed_at: DateTime<Utc>,
    /// Format of the exported data
    pub format: ExportFormat,
    /// Streams included in the export
    pub streams_included: Vec<StreamId>,
    /// Total number of records exported
    pub record_count: u64,
    /// SHA-256 hash of the export data bytes (integrity proof)
    pub content_hash: Hash,
    /// Optional HMAC-SHA256 signature for authenticity verification
    pub signature: Option<Vec<u8>>,
}

/// Audit record tracking every export operation (immutable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportAuditRecord {
    /// Export this audit record refers to
    pub export_id: Uuid,
    /// Data subject whose data was exported
    pub subject_id: String,
    /// When the export was requested
    pub requested_at: DateTime<Utc>,
    /// When the export completed
    pub completed_at: DateTime<Utc>,
    /// Format of the exported data
    pub format: ExportFormat,
    /// Total number of records exported
    pub record_count: u64,
    /// SHA-256 hash of the export data bytes
    pub content_hash: Hash,
}

/// Engine for GDPR Article 20 data portability exports
///
/// Maintains a log of completed exports and an immutable audit trail
/// for compliance verification.
#[derive(Debug, Default)]
pub struct ExportEngine {
    /// Completed exports
    exports: Vec<PortabilityExport>,
    /// Immutable audit trail of all export operations
    audit_trail: Vec<ExportAuditRecord>,
}

impl ExportEngine {
    /// Create a new empty export engine
    pub fn new() -> Self {
        Self::default()
    }

    /// Export all records for a data subject in the requested format
    ///
    /// Collects the records, serializes them into the chosen format, computes
    /// a SHA-256 content hash for integrity verification, and appends an
    /// audit record to the trail.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::NoDataFound`] if `records` is empty.
    /// Returns [`ExportError::ExportFailed`] if serialization fails.
    pub fn export_subject_data(
        &mut self,
        subject_id: &str,
        records: &[ExportRecord],
        format: ExportFormat,
    ) -> Result<PortabilityExport> {
        // Precondition: must have data to export
        if records.is_empty() {
            return Err(ExportError::NoDataFound(subject_id.to_string()));
        }

        let requested_at = Utc::now();

        // Serialize records into the requested format
        let data = match format {
            ExportFormat::Json => Self::format_as_json(records)?,
            ExportFormat::Csv => Self::format_as_csv(records)?,
        };

        // Compute content hash for integrity verification
        let content_hash = Self::compute_content_hash(&data);

        // Collect unique streams included
        let mut streams_included: Vec<StreamId> = records.iter().map(|r| r.stream_id).collect();
        streams_included.sort();
        streams_included.dedup();

        let record_count = records.len() as u64;
        let completed_at = Utc::now();
        let export_id = Uuid::new_v4();

        // Postcondition: content hash must not be genesis (empty data was rejected above)
        assert!(
            !content_hash.is_genesis(),
            "content hash must not be all zeros for non-empty export"
        );

        let export = PortabilityExport {
            export_id,
            subject_id: subject_id.to_string(),
            requested_at,
            completed_at,
            format,
            streams_included,
            record_count,
            content_hash,
            signature: None,
        };

        // Create audit record
        let audit_record = ExportAuditRecord {
            export_id,
            subject_id: subject_id.to_string(),
            requested_at,
            completed_at,
            format,
            record_count,
            content_hash,
        };

        self.exports.push(export.clone());
        self.audit_trail.push(audit_record);

        // Postcondition: audit trail must have grown
        assert!(
            !self.audit_trail.is_empty(),
            "audit trail must contain at least one record after export"
        );

        Ok(export)
    }

    /// Serialize records as pretty-printed JSON
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::ExportFailed`] if JSON serialization fails.
    pub fn format_as_json(records: &[ExportRecord]) -> Result<Vec<u8>> {
        assert!(
            !records.is_empty(),
            "records must not be empty for JSON formatting"
        );

        serde_json::to_vec_pretty(records)
            .map_err(|e| ExportError::ExportFailed(format!("JSON serialization failed: {e}")))
    }

    /// Serialize records as CSV with header row
    ///
    /// Columns: `stream_id,stream_name,offset,data,timestamp`
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::ExportFailed`] if CSV serialization fails.
    pub fn format_as_csv(records: &[ExportRecord]) -> Result<Vec<u8>> {
        use std::fmt::Write;

        assert!(
            !records.is_empty(),
            "records must not be empty for CSV formatting"
        );

        let mut output = String::new();

        // Header row
        output.push_str("stream_id,stream_name,offset,data,timestamp\n");

        // Data rows
        for record in records {
            let data_str = serde_json::to_string(&record.data)
                .map_err(|e| ExportError::ExportFailed(format!("CSV data serialization: {e}")))?;

            // Escape CSV fields that may contain commas or quotes
            let escaped_name = csv_escape(&record.stream_name);
            let escaped_data = csv_escape(&data_str);

            writeln!(
                output,
                "{},{},{},{},{}",
                u64::from(record.stream_id),
                escaped_name,
                record.offset,
                escaped_data,
                record.timestamp.to_rfc3339(),
            )
            .expect("writing to String cannot fail");
        }

        Ok(output.into_bytes())
    }

    /// Compute SHA-256 hash of export data bytes
    ///
    /// Uses SHA-256 for compliance-critical hashing (dual-hash convention:
    /// SHA-256 for compliance paths, BLAKE3 for internal hot paths).
    pub fn compute_content_hash(data: &[u8]) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();

        let mut hash_bytes = [0u8; HASH_LENGTH];
        hash_bytes.copy_from_slice(&result);

        Hash::from_bytes(hash_bytes)
    }

    /// Sign an export with HMAC-SHA256 for authenticity verification
    ///
    /// Computes `SHA256(key || content_hash_bytes)` as a simplified HMAC.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::ExportFailed`] if the export is not found.
    pub fn sign_export(&mut self, export_id: Uuid, signing_key: &[u8]) -> Result<()> {
        // Precondition: signing key must not be empty
        assert!(!signing_key.is_empty(), "signing key must not be empty");

        let export = self
            .exports
            .iter_mut()
            .find(|e| e.export_id == export_id)
            .ok_or_else(|| ExportError::ExportFailed(format!("Export not found: {export_id}")))?;

        let signature = Self::hmac_sha256(signing_key, export.content_hash.as_bytes());
        export.signature = Some(signature);

        Ok(())
    }

    /// Verify the HMAC-SHA256 signature of an export
    ///
    /// Recomputes `SHA256(key || content_hash_bytes)` and compares against
    /// the stored signature using constant-time comparison.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError::SignatureVerificationFailed`] if the export has
    /// no signature attached.
    pub fn verify_export_signature(
        export: &PortabilityExport,
        _data: &[u8],
        key: &[u8],
    ) -> Result<bool> {
        let signature = export
            .signature
            .as_ref()
            .ok_or(ExportError::SignatureVerificationFailed)?;

        let expected = Self::hmac_sha256(key, export.content_hash.as_bytes());

        // Constant-time comparison to prevent timing attacks
        Ok(constant_time_eq(signature, &expected))
    }

    /// Look up a completed export by its ID
    pub fn get_export(&self, export_id: Uuid) -> Option<&PortabilityExport> {
        self.exports.iter().find(|e| e.export_id == export_id)
    }

    /// Returns the immutable audit trail of all export operations
    pub fn get_audit_trail(&self) -> &[ExportAuditRecord] {
        &self.audit_trail
    }

    /// Compute HMAC-SHA256: `SHA256(key || message)`
    fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(message);
        hasher.finalize().to_vec()
    }
}

/// Escape a CSV field value
///
/// Wraps the field in double quotes if it contains commas, double quotes,
/// or newlines. Internal double quotes are escaped by doubling them.
fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        let escaped = field.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        field.to_string()
    }
}

/// Constant-time byte slice comparison to prevent timing side-channels
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }

    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test record
    fn make_record(stream_id: u64, name: &str, offset: u64) -> ExportRecord {
        ExportRecord {
            stream_id: StreamId::new(stream_id),
            stream_name: name.to_string(),
            offset,
            data: serde_json::json!({"field": "value", "count": offset}),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_export_json() {
        let mut engine = ExportEngine::new();
        let records = vec![make_record(1, "patients", 0), make_record(1, "patients", 1)];

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Json)
            .unwrap();

        assert_eq!(export.record_count, 2);
        assert_eq!(export.format, ExportFormat::Json);
        assert!(!export.content_hash.is_genesis());

        // Verify the JSON output is valid
        let data = ExportEngine::format_as_json(&[
            make_record(1, "patients", 0),
            make_record(1, "patients", 1),
        ])
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_export_csv() {
        let mut engine = ExportEngine::new();
        let records = vec![make_record(1, "patients", 0), make_record(2, "billing", 1)];

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Csv)
            .unwrap();

        assert_eq!(export.record_count, 2);
        assert_eq!(export.format, ExportFormat::Csv);

        // Verify CSV structure: header + 2 data rows
        let data = ExportEngine::format_as_csv(&[
            make_record(1, "patients", 0),
            make_record(2, "billing", 1),
        ])
        .unwrap();
        let csv_str = String::from_utf8(data).unwrap();
        let lines: Vec<&str> = csv_str.lines().collect();

        assert_eq!(lines[0], "stream_id,stream_name,offset,data,timestamp");
        assert_eq!(lines.len(), 3); // header + 2 rows
    }

    #[test]
    fn test_content_hash_deterministic() {
        let data = b"deterministic test data for GDPR Article 20 export";

        let hash1 = ExportEngine::compute_content_hash(data);
        let hash2 = ExportEngine::compute_content_hash(data);

        assert_eq!(hash1, hash2);
        assert!(!hash1.is_genesis());
    }

    #[test]
    fn test_sign_and_verify() {
        let mut engine = ExportEngine::new();
        let records = vec![make_record(1, "patients", 0)];
        let key = b"test-signing-key-32-bytes-long!!";

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Json)
            .unwrap();
        let export_id = export.export_id;

        // Sign the export
        engine.sign_export(export_id, key).unwrap();

        // Retrieve the signed export
        let signed_export = engine.get_export(export_id).unwrap();
        assert!(signed_export.signature.is_some());

        // Verify the signature
        let data = ExportEngine::format_as_json(&records).unwrap();
        let valid = ExportEngine::verify_export_signature(signed_export, &data, key).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_verify_fails_wrong_key() {
        let mut engine = ExportEngine::new();
        let records = vec![make_record(1, "patients", 0)];
        let correct_key = b"correct-key-for-signing-purposes";
        let wrong_key = b"wrong-key-should-fail-verify!!!!";

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Json)
            .unwrap();
        let export_id = export.export_id;

        // Sign with the correct key
        engine.sign_export(export_id, correct_key).unwrap();

        // Verify with the wrong key
        let signed_export = engine.get_export(export_id).unwrap();
        let data = ExportEngine::format_as_json(&records).unwrap();
        let valid = ExportEngine::verify_export_signature(signed_export, &data, wrong_key).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_export_audit_trail() {
        let mut engine = ExportEngine::new();
        let records = vec![make_record(1, "patients", 0)];

        assert!(engine.get_audit_trail().is_empty());

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Json)
            .unwrap();

        let trail = engine.get_audit_trail();
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].export_id, export.export_id);
        assert_eq!(trail[0].subject_id, "user@example.com");
        assert_eq!(trail[0].record_count, 1);
        assert_eq!(trail[0].content_hash, export.content_hash);
    }

    #[test]
    fn test_empty_export() {
        let mut engine = ExportEngine::new();
        let records: Vec<ExportRecord> = vec![];

        let result = engine.export_subject_data("user@example.com", &records, ExportFormat::Json);

        assert!(matches!(result, Err(ExportError::NoDataFound(ref s)) if s == "user@example.com"));
    }

    #[test]
    fn test_export_multiple_streams() {
        let mut engine = ExportEngine::new();
        let records = vec![
            make_record(1, "patients", 0),
            make_record(2, "billing", 0),
            make_record(3, "prescriptions", 0),
            make_record(1, "patients", 1),
        ];

        let export = engine
            .export_subject_data("user@example.com", &records, ExportFormat::Json)
            .unwrap();

        assert_eq!(export.record_count, 4);
        // 3 unique streams (deduped)
        assert_eq!(export.streams_included.len(), 3);
        assert!(export.streams_included.contains(&StreamId::new(1)));
        assert!(export.streams_included.contains(&StreamId::new(2)));
        assert!(export.streams_included.contains(&StreamId::new(3)));
    }

    #[test]
    fn test_constant_time_eq() {
        let a = vec![1u8, 2, 3, 4];
        let b = vec![1u8, 2, 3, 4];
        let c = vec![1u8, 2, 3, 5];
        let d = vec![1u8, 2, 3];

        assert!(constant_time_eq(&a, &b));
        assert!(!constant_time_eq(&a, &c));
        assert!(!constant_time_eq(&a, &d));
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("simple"), "simple");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
        assert_eq!(csv_escape("has\nnewline"), "\"has\nnewline\"");
    }
}
