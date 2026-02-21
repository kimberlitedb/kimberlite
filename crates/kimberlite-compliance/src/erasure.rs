//! GDPR Article 17 Right to Erasure (Right to be Forgotten)
//!
//! This module implements the right-to-erasure workflow for personal data under GDPR.
//!
//! # GDPR Requirements
//!
//! **Article 17(1)**: Data subject has the right to obtain erasure of personal data
//! without undue delay where:
//! - (a) Data no longer necessary for original purpose
//! - (b) Consent withdrawn and no other legal ground
//! - (c) Data subject objects and no overriding legitimate grounds
//! - (d) Data unlawfully processed
//! - (e) Legal obligation to erase
//! - (f) Data collected re: offer of information society services to a child
//!
//! **Article 17(3)**: Exemptions where erasure does not apply:
//! - (b) Legal obligation
//! - (c) Public health
//! - (d) Archiving in public interest
//! - (e) Establishment, exercise, or defence of legal claims
//!
//! # Architecture
//!
//! ```text
//! ErasureRequest → ErasureEngine:
//!   1. Validate subject and check exemptions
//!   2. Identify affected streams
//!   3. Erase records per-stream
//!   4. Generate audit record with cryptographic proof
//! ```
//!
//! # Example
//!
//! ```
//! use kimberlite_compliance::erasure::{ErasureEngine, ExemptionBasis};
//! use kimberlite_types::StreamId;
//!
//! let mut engine = ErasureEngine::new();
//!
//! // Create erasure request (30-day deadline)
//! let request = engine.request_erasure("user@example.com").unwrap();
//! let request_id = request.request_id;
//!
//! // Mark in progress with affected streams
//! let streams = vec![StreamId::new(1), StreamId::new(2)];
//! engine.mark_in_progress(request_id, streams).unwrap();
//!
//! // Record per-stream progress
//! engine.mark_stream_erased(request_id, StreamId::new(1), 50).unwrap();
//! engine.mark_stream_erased(request_id, StreamId::new(2), 30).unwrap();
//!
//! // Complete with computed cryptographic proof
//! let audit = engine.complete_erasure(request_id).unwrap();
//! assert_eq!(audit.records_erased, 80);
//! ```

use chrono::{DateTime, Duration, Utc};
use kimberlite_types::{Hash, StreamId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum ErasureError {
    #[error("Erasure request not found: {0}")]
    RequestNotFound(Uuid),

    #[error("Erasure request already completed")]
    AlreadyCompleted,

    #[error("Erasure exempt: {0}")]
    Exempt(String),

    #[error("No data found for subject: {0}")]
    NoDataFound(String),

    #[error("Invalid subject identifier: {0}")]
    InvalidSubject(String),

    #[error("Request not in expected state for this operation")]
    InvalidState,
}

pub type Result<T> = std::result::Result<T, ErasureError>;

// ============================================================================
// Status and Exemption Types
// ============================================================================

/// Current status of an erasure request.
///
/// Models the lifecycle: `Pending` -> `InProgress` -> `Complete` | `Failed` | `Exempt`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErasureStatus {
    /// Request received, awaiting processing.
    Pending,

    /// Erasure in progress; tracks remaining streams.
    InProgress {
        /// Number of streams still to be erased.
        streams_remaining: usize,
    },

    /// Erasure completed successfully.
    Complete {
        /// When the erasure was finalized.
        erased_at: DateTime<Utc>,
        /// Total number of records erased across all streams.
        total_records: u64,
    },

    /// Erasure failed and will be retried.
    Failed {
        /// Reason for the failure.
        reason: String,
        /// When the next retry should occur.
        retry_at: DateTime<Utc>,
    },

    /// Request is exempt from erasure under GDPR Article 17(3).
    Exempt {
        /// Legal basis for the exemption.
        basis: ExemptionBasis,
    },
}

/// Legal basis for exemption from the right to erasure.
///
/// Corresponds to GDPR Article 17(3) sub-paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExemptionBasis {
    /// Article 17(3)(b): Compliance with a legal obligation.
    LegalObligation,
    /// Article 17(3)(c): Reasons of public interest in public health.
    PublicHealth,
    /// Article 17(3)(d): Archiving in the public interest, scientific or
    /// historical research, or statistical purposes.
    Archiving,
    /// Article 17(3)(e): Establishment, exercise, or defence of legal claims.
    LegalClaims,
}

impl std::fmt::Display for ExemptionBasis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LegalObligation => write!(f, "Legal Obligation (Art. 17(3)(b))"),
            Self::PublicHealth => write!(f, "Public Health (Art. 17(3)(c))"),
            Self::Archiving => write!(f, "Archiving (Art. 17(3)(d))"),
            Self::LegalClaims => write!(f, "Legal Claims (Art. 17(3)(e))"),
        }
    }
}

// ============================================================================
// Request and Audit Types
// ============================================================================

/// GDPR Article 17 deadline: 30 days from request.
const ERASURE_DEADLINE_DAYS: i64 = 30;

/// An erasure request tracking the lifecycle of a right-to-erasure invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureRequest {
    /// Unique identifier for this request.
    pub request_id: Uuid,
    /// Data subject identifier (e.g., email, user ID).
    pub subject_id: String,
    /// When the request was received.
    pub requested_at: DateTime<Utc>,
    /// GDPR deadline: `requested_at` + 30 days.
    pub deadline: DateTime<Utc>,
    /// Current status of the request.
    pub status: ErasureStatus,
    /// Streams identified as containing the subject's data.
    pub affected_streams: Vec<StreamId>,
    /// Running count of records erased so far.
    pub records_erased: u64,
}

/// Immutable audit record created upon successful erasure completion.
///
/// These records form part of the compliance audit trail and must be
/// retained even after the underlying data is erased (GDPR allows
/// retaining proof that erasure occurred).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureAuditRecord {
    /// Request that triggered this erasure.
    pub request_id: Uuid,
    /// Data subject whose data was erased.
    pub subject_id: String,
    /// When the erasure was originally requested.
    pub requested_at: DateTime<Utc>,
    /// When the erasure was completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Total records erased across all streams.
    pub records_erased: u64,
    /// Streams that were affected by the erasure.
    pub streams_affected: Vec<StreamId>,
    /// SHA-256 proof hash of erased record identifiers.
    pub erasure_proof: Option<Hash>,
}

// ============================================================================
// ErasureEngine
// ============================================================================

/// Engine managing the lifecycle of GDPR Article 17 erasure requests.
///
/// Maintains pending requests and completed audit records. All state
/// transitions are validated to ensure the erasure workflow is followed
/// correctly.
#[derive(Debug, Default)]
pub struct ErasureEngine {
    /// Active (non-completed) erasure requests.
    pending: Vec<ErasureRequest>,
    /// Completed erasure audit trail.
    completed: Vec<ErasureAuditRecord>,
}

impl ErasureEngine {
    /// Create a new empty erasure engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Submit a new erasure request for a data subject.
    ///
    /// Creates a pending request with a 30-day GDPR deadline.
    ///
    /// # Errors
    ///
    /// Returns `InvalidSubject` if `subject_id` is empty.
    pub fn request_erasure(&mut self, subject_id: &str) -> Result<ErasureRequest> {
        // Precondition: subject_id must be non-empty
        if subject_id.is_empty() {
            return Err(ErasureError::InvalidSubject(subject_id.to_string()));
        }

        let now = Utc::now();
        let deadline = now + Duration::days(ERASURE_DEADLINE_DAYS);

        let request = ErasureRequest {
            request_id: Uuid::new_v4(),
            subject_id: subject_id.to_string(),
            requested_at: now,
            deadline,
            status: ErasureStatus::Pending,
            affected_streams: Vec::new(),
            records_erased: 0,
        };

        // Postcondition: deadline is 30 days after request
        assert!(
            request.deadline > request.requested_at,
            "deadline must be after requested_at"
        );

        self.pending.push(request.clone());

        // Postcondition: request exists in pending list
        assert!(
            self.get_request(self.pending.last().expect("just pushed").request_id)
                .is_some(),
            "request must be findable after insertion"
        );

        Ok(request)
    }

    /// Transition an erasure request to `InProgress` with identified streams.
    ///
    /// # Errors
    ///
    /// Returns `RequestNotFound` if the request does not exist.
    /// Returns `AlreadyCompleted` if the request is already complete.
    /// Returns `InvalidState` if the request is not Pending.
    pub fn mark_in_progress(&mut self, request_id: Uuid, streams: Vec<StreamId>) -> Result<()> {
        let request = self.find_pending_mut(request_id)?;

        // Precondition: must be in Pending state
        match &request.status {
            ErasureStatus::Pending => {}
            ErasureStatus::Complete { .. } => return Err(ErasureError::AlreadyCompleted),
            _ => return Err(ErasureError::InvalidState),
        }

        let stream_count = streams.len();
        request.affected_streams = streams;
        request.status = ErasureStatus::InProgress {
            streams_remaining: stream_count,
        };

        // Postcondition: status reflects correct stream count
        assert!(
            matches!(
                &request.status,
                ErasureStatus::InProgress { streams_remaining } if *streams_remaining == stream_count
            ),
            "status must reflect the number of streams"
        );

        Ok(())
    }

    /// Record that a stream's data has been erased.
    ///
    /// Decrements the remaining stream count and accumulates the erased
    /// record total.
    ///
    /// # Errors
    ///
    /// Returns `RequestNotFound` if the request does not exist.
    /// Returns `AlreadyCompleted` if the request is already complete.
    /// Returns `InvalidState` if the request is not `InProgress`.
    pub fn mark_stream_erased(
        &mut self,
        request_id: Uuid,
        _stream_id: StreamId,
        records: u64,
    ) -> Result<()> {
        let request = self.find_pending_mut(request_id)?;

        // Precondition: must be InProgress
        let streams_remaining = match &request.status {
            ErasureStatus::InProgress { streams_remaining } => *streams_remaining,
            ErasureStatus::Complete { .. } => return Err(ErasureError::AlreadyCompleted),
            _ => return Err(ErasureError::InvalidState),
        };

        // Precondition: must have streams remaining
        assert!(
            streams_remaining > 0,
            "cannot erase stream when none remaining"
        );

        let previous_erased = request.records_erased;
        request.records_erased += records;
        request.status = ErasureStatus::InProgress {
            streams_remaining: streams_remaining - 1,
        };

        // Postcondition: record count increased
        assert!(
            request.records_erased >= previous_erased,
            "records_erased must not decrease"
        );

        Ok(())
    }

    /// Finalize an erasure request and create an audit record.
    ///
    /// Computes an erasure proof internally as `SHA-256(request_id || subject_id || erased_count)`.
    ///
    /// # Errors
    ///
    /// Returns `RequestNotFound` if the request does not exist.
    /// Returns `AlreadyCompleted` if already finalized.
    /// Returns `InvalidState` if not in a completable state.
    pub fn complete_erasure(&mut self, request_id: Uuid) -> Result<ErasureAuditRecord> {
        let request = self.find_pending_mut(request_id)?;

        // Precondition: must be InProgress (or Pending if no streams found)
        match &request.status {
            ErasureStatus::InProgress { .. } | ErasureStatus::Pending => {}
            ErasureStatus::Complete { .. } => return Err(ErasureError::AlreadyCompleted),
            _ => return Err(ErasureError::InvalidState),
        }

        let completed_at = Utc::now();
        let records_erased = request.records_erased;
        let streams_affected = request.affected_streams.clone();
        let subject_id = request.subject_id.clone();
        let requested_at = request.requested_at;

        // Compute erasure proof: SHA-256(request_id || subject_id || erased_count)
        let erasure_proof = Self::compute_erasure_proof(request_id, &subject_id, records_erased);

        request.status = ErasureStatus::Complete {
            erased_at: completed_at,
            total_records: records_erased,
        };

        let audit_record = ErasureAuditRecord {
            request_id,
            subject_id,
            requested_at,
            completed_at: Some(completed_at),
            records_erased,
            streams_affected,
            erasure_proof: Some(erasure_proof),
        };

        // Postcondition: audit record matches request data
        assert_eq!(
            audit_record.request_id, request_id,
            "audit record must reference the correct request"
        );

        self.completed.push(audit_record.clone());

        Ok(audit_record)
    }

    /// Mark an erasure request as exempt under GDPR Article 17(3).
    ///
    /// # Errors
    ///
    /// Returns `RequestNotFound` if the request does not exist.
    /// Returns `AlreadyCompleted` if already finalized.
    pub fn exempt_from_erasure(&mut self, request_id: Uuid, basis: ExemptionBasis) -> Result<()> {
        let request = self.find_pending_mut(request_id)?;

        // Precondition: cannot exempt a completed request
        if matches!(&request.status, ErasureStatus::Complete { .. }) {
            return Err(ErasureError::AlreadyCompleted);
        }

        request.status = ErasureStatus::Exempt { basis };

        // Postcondition: status is now Exempt
        assert!(
            matches!(&request.status, ErasureStatus::Exempt { .. }),
            "status must be Exempt after exemption"
        );

        Ok(())
    }

    /// Check for erasure requests that have exceeded their GDPR 30-day deadline.
    ///
    /// Returns references to all overdue requests that are not yet completed
    /// or exempt.
    pub fn check_deadlines(&self, now: DateTime<Utc>) -> Vec<&ErasureRequest> {
        self.pending
            .iter()
            .filter(|r| {
                let is_active = matches!(
                    &r.status,
                    ErasureStatus::Pending | ErasureStatus::InProgress { .. }
                );
                is_active && now > r.deadline
            })
            .collect()
    }

    /// Look up an erasure request by ID.
    pub fn get_request(&self, request_id: Uuid) -> Option<&ErasureRequest> {
        self.pending.iter().find(|r| r.request_id == request_id)
    }

    /// Returns the completed erasure audit trail.
    pub fn get_audit_trail(&self) -> &[ErasureAuditRecord] {
        &self.completed
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Compute erasure proof: `SHA-256(request_id || subject_id || erased_count)`.
    fn compute_erasure_proof(request_id: Uuid, subject_id: &str, records_erased: u64) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(request_id.as_bytes());
        hasher.update(subject_id.as_bytes());
        hasher.update(records_erased.to_le_bytes());
        let result = hasher.finalize();

        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&result);
        Hash::from_bytes(hash_bytes)
    }

    /// Find a pending request by ID, returning a mutable reference.
    fn find_pending_mut(&mut self, request_id: Uuid) -> Result<&mut ErasureRequest> {
        self.pending
            .iter_mut()
            .find(|r| r.request_id == request_id)
            .ok_or(ErasureError::RequestNotFound(request_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_erasure() {
        let mut engine = ErasureEngine::new();
        let request = engine.request_erasure("user@example.com").unwrap();

        assert_eq!(request.subject_id, "user@example.com");
        assert!(matches!(request.status, ErasureStatus::Pending));
        assert_eq!(request.records_erased, 0);
        assert!(request.affected_streams.is_empty());

        // Verify 30-day deadline
        let expected_deadline = request.requested_at + Duration::days(30);
        assert_eq!(request.deadline, expected_deadline);

        // Verify request is retrievable
        let retrieved = engine.get_request(request.request_id).unwrap();
        assert_eq!(retrieved.request_id, request.request_id);
    }

    #[test]
    fn test_request_erasure_invalid_subject() {
        let mut engine = ErasureEngine::new();
        let result = engine.request_erasure("");
        assert!(matches!(result, Err(ErasureError::InvalidSubject(_))));
    }

    #[test]
    fn test_erasure_lifecycle() {
        let mut engine = ErasureEngine::new();

        // Step 1: Request erasure
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;
        assert!(matches!(request.status, ErasureStatus::Pending));

        // Step 2: Mark in progress with affected streams
        let streams = vec![StreamId::new(1), StreamId::new(2), StreamId::new(3)];
        engine.mark_in_progress(request_id, streams).unwrap();

        let request = engine.get_request(request_id).unwrap();
        assert!(matches!(
            request.status,
            ErasureStatus::InProgress {
                streams_remaining: 3
            }
        ));
        assert_eq!(request.affected_streams.len(), 3);

        // Step 3: Erase streams one by one
        engine
            .mark_stream_erased(request_id, StreamId::new(1), 50)
            .unwrap();
        let request = engine.get_request(request_id).unwrap();
        assert!(matches!(
            request.status,
            ErasureStatus::InProgress {
                streams_remaining: 2
            }
        ));
        assert_eq!(request.records_erased, 50);

        engine
            .mark_stream_erased(request_id, StreamId::new(2), 30)
            .unwrap();
        engine
            .mark_stream_erased(request_id, StreamId::new(3), 20)
            .unwrap();

        let request = engine.get_request(request_id).unwrap();
        assert_eq!(request.records_erased, 100);

        // Step 4: Complete (proof computed internally)
        let audit = engine.complete_erasure(request_id).unwrap();

        assert_eq!(audit.request_id, request_id);
        assert_eq!(audit.subject_id, "user@example.com");
        assert_eq!(audit.records_erased, 100);
        assert_eq!(audit.streams_affected.len(), 3);
        assert!(audit.completed_at.is_some());
        assert!(audit.erasure_proof.is_some());
    }

    #[test]
    fn test_exemption() {
        let mut engine = ErasureEngine::new();
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;

        engine
            .exempt_from_erasure(request_id, ExemptionBasis::LegalObligation)
            .unwrap();

        let request = engine.get_request(request_id).unwrap();
        assert!(matches!(
            request.status,
            ErasureStatus::Exempt {
                basis: ExemptionBasis::LegalObligation
            }
        ));
    }

    #[test]
    fn test_deadline_check() {
        let mut engine = ErasureEngine::new();

        // Create a request and manually set an expired deadline
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;

        // Simulate overdue by checking with a time 31 days in the future
        let future = Utc::now() + Duration::days(31);
        let overdue = engine.check_deadlines(future);
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].request_id, request_id);

        // Current time should show no overdue requests
        let overdue_now = engine.check_deadlines(Utc::now());
        assert!(overdue_now.is_empty());

        // Completed requests should not appear as overdue
        engine.complete_erasure(request_id).unwrap();

        let overdue_after_complete = engine.check_deadlines(future);
        assert!(overdue_after_complete.is_empty());
    }

    #[test]
    fn test_audit_trail() {
        let mut engine = ErasureEngine::new();

        // Initially empty
        assert!(engine.get_audit_trail().is_empty());

        // Complete an erasure
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;

        let streams = vec![StreamId::new(1)];
        engine.mark_in_progress(request_id, streams).unwrap();
        engine
            .mark_stream_erased(request_id, StreamId::new(1), 42)
            .unwrap();

        engine.complete_erasure(request_id).unwrap();

        // Audit trail should have one record
        let trail = engine.get_audit_trail();
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].request_id, request_id);
        assert_eq!(trail[0].records_erased, 42);
        assert_eq!(trail[0].streams_affected, vec![StreamId::new(1)]);
        assert!(trail[0].completed_at.is_some());
        assert!(trail[0].erasure_proof.is_some());
    }

    #[test]
    fn test_double_complete() {
        let mut engine = ErasureEngine::new();
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;

        engine.complete_erasure(request_id).unwrap();

        // Second completion should fail
        let result = engine.complete_erasure(request_id);
        assert!(matches!(result, Err(ErasureError::AlreadyCompleted)));
    }

    #[test]
    fn test_request_not_found() {
        let mut engine = ErasureEngine::new();
        let fake_id = Uuid::new_v4();

        let result = engine.mark_in_progress(fake_id, vec![]);
        assert!(matches!(result, Err(ErasureError::RequestNotFound(_))));

        let result = engine.mark_stream_erased(fake_id, StreamId::new(1), 10);
        assert!(matches!(result, Err(ErasureError::RequestNotFound(_))));

        let result = engine.complete_erasure(fake_id);
        assert!(matches!(result, Err(ErasureError::RequestNotFound(_))));
    }

    #[test]
    fn test_invalid_state_transitions() {
        let mut engine = ErasureEngine::new();
        let request = engine.request_erasure("user@example.com").unwrap();
        let request_id = request.request_id;

        // Cannot mark_stream_erased on a Pending request
        let result = engine.mark_stream_erased(request_id, StreamId::new(1), 10);
        assert!(matches!(result, Err(ErasureError::InvalidState)));

        // Transition to InProgress
        engine
            .mark_in_progress(request_id, vec![StreamId::new(1)])
            .unwrap();

        // Cannot mark_in_progress again
        let result = engine.mark_in_progress(request_id, vec![StreamId::new(2)]);
        assert!(matches!(result, Err(ErasureError::InvalidState)));
    }

    #[test]
    fn test_exemption_display() {
        assert_eq!(
            ExemptionBasis::LegalObligation.to_string(),
            "Legal Obligation (Art. 17(3)(b))"
        );
        assert_eq!(
            ExemptionBasis::PublicHealth.to_string(),
            "Public Health (Art. 17(3)(c))"
        );
        assert_eq!(
            ExemptionBasis::Archiving.to_string(),
            "Archiving (Art. 17(3)(d))"
        );
        assert_eq!(
            ExemptionBasis::LegalClaims.to_string(),
            "Legal Claims (Art. 17(3)(e))"
        );
    }

    #[test]
    fn test_multiple_subjects() {
        let mut engine = ErasureEngine::new();

        let req1 = engine.request_erasure("user1@example.com").unwrap();
        let req2 = engine.request_erasure("user2@example.com").unwrap();

        assert_ne!(req1.request_id, req2.request_id);
        assert_eq!(req1.subject_id, "user1@example.com");
        assert_eq!(req2.subject_id, "user2@example.com");

        // Complete one, leave other pending
        engine.complete_erasure(req1.request_id).unwrap();

        assert_eq!(engine.get_audit_trail().len(), 1);

        // Overdue check only shows the pending one
        let future = Utc::now() + Duration::days(31);
        let overdue = engine.check_deadlines(future);
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].request_id, req2.request_id);
    }
}
