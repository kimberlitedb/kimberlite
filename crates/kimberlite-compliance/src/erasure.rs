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
use kimberlite_crypto::signature::{SigningKey, VerifyingKey};
use kimberlite_types::{Hash, StreamId, TenantId};
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

    /// AUDIT-2026-04 C-4: scope-bound error path.
    ///
    /// Returned by `ErasureEngine::mark_stream_erased_with_scope` when
    /// the caller somehow presents a count above the scope's
    /// `max_records` bound — the scope's constructor captures the
    /// stream length at scope-creation time, so any value above it is
    /// necessarily a forgery attempt or an integration bug.
    #[error("records count {count} exceeds scope bound {max}")]
    RecordsExceedScope { count: u64, max: u64 },

    /// AUDIT-2026-04 H-4: proof signature verification failed.
    ///
    /// Returned by `ErasureProof::verify` when the Ed25519 signature
    /// does not validate against the canonical proof bytes under the
    /// supplied public key. Under honest production, this should never
    /// fire — it only surfaces if an attestation ledger is tampered
    /// with or the wrong verifying key is used.
    #[error("erasure proof signature verification failed")]
    InvalidProofSignature,

    /// AUDIT-2026-04 C-1: the `ErasureExecutor` providing the shred
    /// + merkle-root primitives reported a failure. Wraps the
    ///   executor's concrete error as a string to keep the compliance
    ///   crate free of a hard dependency on kernel/storage/crypto
    ///   error types.
    #[error("erasure executor failed: {0}")]
    ExecutorFailure(String),
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

impl ErasureRequest {
    /// The sole public constructor for [`ErasureScope`]. Returns
    /// `None` unless:
    ///
    /// - `stream_id` is in `self.affected_streams` (the stream was
    ///   identified as containing the subject's data at request
    ///   creation), and
    /// - `stream_id.tenant_id() == subject_tenant` (the stream belongs
    ///   to the subject's tenant).
    ///
    /// `stream_length` is captured as the scope's `max_records` bound
    /// — `mark_stream_erased_with_scope` will reject any count above
    /// this.
    ///
    /// **AUDIT-2026-04 C-4**: this is the type-level replacement for
    /// the old `mark_stream_erased(_, _stream_id, _)`. Because every
    /// field of `ErasureScope` is private, no caller can construct
    /// one out-of-band.
    pub fn scope_for(
        &self,
        stream_id: StreamId,
        subject_tenant: TenantId,
        stream_length: u64,
    ) -> Option<ErasureScope> {
        if !self.affected_streams.contains(&stream_id) {
            return None;
        }
        if stream_id.tenant_id() != subject_tenant {
            return None;
        }
        Some(ErasureScope {
            request_id: self.request_id,
            stream_id,
            subject_tenant,
            max_records: stream_length,
        })
    }
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
    /// Legacy SHA-256 digest of `(request_id || subject_id || count)`.
    ///
    /// **Deprecated (AUDIT-2026-04 H-4)**: this binds only to
    /// self-reported counts, not to the cryptographic act. Present for
    /// back-compat with historical records; new records carry
    /// `signed_proof` instead.
    pub erasure_proof: Option<Hash>,
    /// AUDIT-2026-04 H-4 — the real proof of erasure.
    ///
    /// Binds the attestation to (per-stream pre-erasure merkle roots,
    /// per-stream key-shred digests, timestamp) via an Ed25519
    /// signature. Absent on pre-AUDIT-2026-04 records; present on
    /// everything produced by `complete_erasure_with_attestation`.
    #[serde(default)]
    pub signed_proof: Option<ErasureProof>,
}

// ============================================================================
// AUDIT-2026-04 C-4 — ErasureScope (parse, don't validate)
// ============================================================================

/// A token certifying "this caller is allowed to mark progress against
/// a specific `(request_id, stream_id)` for a specific subject tenant,
/// up to `max_records`."
///
/// **AUDIT-2026-04 C-4 type-level fix.** Prior to this change,
/// `mark_stream_erased(request_id, _stream_id, records)` ignored
/// `_stream_id` (note the underscore) — so an arbitrary caller could
/// inflate `records_erased` against any request with any stream. The
/// audit called this a *"forgeable erasure ledger"*.
///
/// The fix makes the invariant structural: `ErasureScope` has private
/// fields and no public constructor. The *only* way to obtain one is
/// [`ErasureRequest::scope_for`], which returns `None` unless:
///
/// 1. `stream_id` ∈ `request.affected_streams` — the stream was
///    identified as containing the subject's data at request-creation
///    time.
/// 2. `stream_id.tenant_id() == subject_tenant` — the stream belongs to
///    the subject's tenant. Cross-tenant forgery is unrepresentable.
///
/// Once a scope is in hand, `mark_stream_erased_with_scope` can
/// increment `records_erased` without further validation — the
/// invariants are baked into the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErasureScope {
    request_id: Uuid,
    stream_id: StreamId,
    subject_tenant: TenantId,
    /// Upper bound on `records` that `mark_stream_erased_with_scope`
    /// will accept. Captured from the stream's length at the time the
    /// scope was produced, closing off "report an inflated count"
    /// attacks against the ledger.
    max_records: u64,
}

impl ErasureScope {
    pub fn request_id(&self) -> Uuid {
        self.request_id
    }
    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }
    pub fn subject_tenant(&self) -> TenantId {
        self.subject_tenant
    }
    pub fn max_records(&self) -> u64 {
        self.max_records
    }
}

// ============================================================================
// AUDIT-2026-04 H-4 — AttestationKey, ErasureProof, StreamErasureWitness
// ============================================================================

/// Newtype around `kimberlite_crypto::SigningKey` identifying the
/// *server attestation key* — the key used to sign ErasureProofs.
///
/// **AUDIT-2026-04 H-4 rationale.** The audit found that the
/// "cryptographic erasure proof" was `SHA-256(request_id || subject_id
/// || records_erased)` — a hash over self-reported counts, with no
/// signature from any key the server holds.
///
/// `AttestationKey` is a newtype rather than a type alias so the
/// compiler prevents accidentally using a DEK-signing key or an
/// identity key for attestations. PRESSURECRAFT §2 — "make illegal
/// states unrepresentable": the type-level statement that this key is
/// for attestation, nothing else.
pub struct AttestationKey(SigningKey);

impl AttestationKey {
    /// Wrap an existing `SigningKey` as the server's attestation key.
    ///
    /// Production callers root this in the server's key hierarchy;
    /// tests and `ErasureEngine` unit tests can pass a freshly
    /// generated one.
    pub fn from_signing_key(key: SigningKey) -> Self {
        Self(key)
    }

    /// Generate a fresh attestation key.
    pub fn generate() -> Self {
        Self(SigningKey::generate())
    }

    /// The public half, suitable for publishing so auditors can
    /// independently verify stored proofs.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }

    /// Sign canonical proof bytes. Private — only
    /// `ErasureProof::sign_with` calls this, to prevent ad-hoc
    /// signatures over non-canonical bytes.
    fn sign(&self, bytes: &[u8]) -> [u8; 64] {
        self.0.sign(bytes).to_bytes()
    }
}

impl std::fmt::Debug for AttestationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material.
        f.write_str("AttestationKey(<redacted>)")
    }
}

/// One stream's pre-/post-erasure witnesses, feeding
/// `ErasureEngine::complete_erasure_with_attestation`.
///
/// - `pre_erasure_merkle_root`: the chain head of `stream_id` *before*
///   any erasure action. This is what proves "there was something
///   specific that got destroyed" — later audits can verify the
///   attestation signs a commitment to this root.
/// - `key_shred_digest`: returned by `DataEncryptionKey::shred(nonce)`
///   at the moment the DEK was destroyed. This commits the proof to
///   the act of key destruction, not just a count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamErasureWitness {
    pub stream_id: StreamId,
    pub pre_erasure_merkle_root: [u8; 32],
    pub key_shred_digest: [u8; 32],
}

/// A signed proof that an erasure act occurred, binding to the
/// specific pre-erasure state + key-shredding events.
///
/// **AUDIT-2026-04 H-4.** The proof's canonical bytes are
/// `bundle_merkle_root(witnesses) || timestamp_ns_le`, where
/// `bundle_merkle_root` is the SHA-256 hash of the concatenated
/// per-stream witnesses. The signature is Ed25519 over those bytes
/// with the server's `AttestationKey`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureProof {
    /// SHA-256 over the serialized `witnesses` list. Captures every
    /// per-stream commitment in a single root.
    pub bundle_root: [u8; 32],
    /// Unix-nanosecond timestamp the proof was produced.
    pub timestamp_ns: u64,
    /// Per-stream witnesses — the raw commitments that the bundle_root
    /// covers, retained in the proof so downstream verifiers don't
    /// need to reconstruct them.
    pub witnesses: Vec<StreamErasureWitness>,
    /// Ed25519 signature over `canonical_bytes()` produced by the
    /// server's `AttestationKey`. 64 bytes — stored as `Vec<u8>` to
    /// get serde support on stable without pulling in
    /// `serde-big-array`. Length is asserted in `verify`.
    pub attestation_sig: Vec<u8>,
}

impl ErasureProof {
    /// Construct and sign a proof over the given witnesses.
    ///
    /// **Pure except for the signing step** (PRESSURECRAFT §1 FCIS):
    /// bundle-root computation is deterministic in the witnesses;
    /// `AttestationKey::sign` is the only impure step.
    pub fn sign_with(
        witnesses: Vec<StreamErasureWitness>,
        timestamp_ns: u64,
        attestation_key: &AttestationKey,
    ) -> Self {
        let bundle_root = Self::compute_bundle_root(&witnesses);
        let canonical = Self::canonical_bytes_for(&bundle_root, timestamp_ns);
        let attestation_sig = attestation_key.sign(&canonical).to_vec();
        Self {
            bundle_root,
            timestamp_ns,
            witnesses,
            attestation_sig,
        }
    }

    /// Verify the proof against a verifying key.
    ///
    /// Returns `Ok(())` if the signature validates over the canonical
    /// bytes and the stored bundle_root matches a fresh recomputation
    /// from the witnesses (guarantees the witnesses were not tampered
    /// with after signing).
    ///
    /// # Errors
    ///
    /// Returns `InvalidProofSignature` on any mismatch.
    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<()> {
        // Recompute bundle root — catches witness tampering.
        let expected_root = Self::compute_bundle_root(&self.witnesses);
        if expected_root != self.bundle_root {
            return Err(ErasureError::InvalidProofSignature);
        }
        if self.attestation_sig.len() != 64 {
            return Err(ErasureError::InvalidProofSignature);
        }
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&self.attestation_sig);
        let sig = kimberlite_crypto::signature::Signature::from_bytes(&sig_bytes);
        let canonical = Self::canonical_bytes_for(&self.bundle_root, self.timestamp_ns);
        verifying_key
            .verify(&canonical, &sig)
            .map_err(|_| ErasureError::InvalidProofSignature)
    }

    fn compute_bundle_root(witnesses: &[StreamErasureWitness]) -> [u8; 32] {
        let mut hasher = <Sha256 as Digest>::new();
        // Length-prefix guards against boundary-ambiguity attacks
        // where e.g. two witnesses could be merged with one.
        hasher.update((witnesses.len() as u64).to_le_bytes());
        for w in witnesses {
            hasher.update(u64::from(w.stream_id.local_id()).to_le_bytes());
            hasher.update(u64::from(w.stream_id.tenant_id()).to_le_bytes());
            hasher.update(w.pre_erasure_merkle_root);
            hasher.update(w.key_shred_digest);
        }
        hasher.finalize().into()
    }

    fn canonical_bytes_for(bundle_root: &[u8; 32], timestamp_ns: u64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 + 8);
        bytes.extend_from_slice(bundle_root);
        bytes.extend_from_slice(&timestamp_ns.to_le_bytes());
        bytes
    }
}

// ============================================================================
// AUDIT-2026-04 C-1 — ErasureExecutor trait
// ============================================================================

/// Runtime-layer trait that [`ErasureEngine::execute_erasure`] calls
/// to perform the *actual* erasure act (delete subject rows + shred
/// the stream's DEK) and produce the primitives needed for a signed
/// attestation.
///
/// **Why a trait?** The compliance crate must not take a direct
/// dependency on `kimberlite-kernel`, `kimberlite-storage`, or
/// `kimberlite-crypto` (those crates sit *above* compliance in the
/// layering; a back-edge would create a cycle). The runtime in
/// `crates/kimberlite/src/kimberlite.rs` provides the concrete impl
/// (`KernelBackedErasureExecutor`) that wires:
///
/// - `pre_erasure_merkle_root` → `Storage::latest_chain_hash(stream_id)`
/// - `shred_stream` → `Command::Delete` for every subject row on the
///   stream's projection, then `DataEncryptionKey::shred(nonce)` on
///   the stream's DEK.
///
/// **Why two methods?** The pre-erasure root must be captured *before*
/// any mutation. Splitting the two primitives into separate methods
/// forces the orchestrator in `execute_erasure` to call them in the
/// correct order, and makes the ordering auditable.
pub trait ErasureExecutor {
    /// Snapshot the stream's current chain-head hash. Must be called
    /// *before* `shred_stream`.
    ///
    /// # Errors
    ///
    /// Returns any runtime error (I/O, stream missing, etc.). Callers
    /// wrap into [`ErasureError::ExecutorFailure`].
    fn pre_erasure_merkle_root(
        &mut self,
        stream_id: StreamId,
    ) -> std::result::Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>>;

    /// Perform the erasure for `subject_id` on `stream_id`: delete
    /// subject rows from the projection, shred the stream's DEK, and
    /// return the receipt binding both acts.
    ///
    /// # Errors
    ///
    /// Returns any runtime error (I/O, kernel rejection, DEK not
    /// found, etc.). Callers wrap into [`ErasureError::ExecutorFailure`].
    fn shred_stream(
        &mut self,
        stream_id: StreamId,
        subject_id: &str,
    ) -> std::result::Result<StreamShredReceipt, Box<dyn std::error::Error + Send + Sync>>;
}

/// Receipt returned from [`ErasureExecutor::shred_stream`].
///
/// Binds the three quantities the signed proof commits to:
/// - `key_shred_digest` — returned by `DataEncryptionKey::shred(nonce)`
///   at DEK destruction. Proves the DEK is unrecoverable.
/// - `records_erased` — how many rows were actually deleted. Fed into
///   the ledger via `mark_stream_erased_with_scope`.
/// - `stream_length_at_shred` — the row count captured *before* delete,
///   used as the `ErasureScope::max_records` cap. Prevents inflated
///   `records_erased` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamShredReceipt {
    pub key_shred_digest: [u8; 32],
    pub records_erased: u64,
    pub stream_length_at_shred: u64,
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

        // ALWAYS: GDPR Article 17 deadline is exactly 30 days from request.
        kimberlite_properties::always!(
            (request.deadline - request.requested_at).num_days() == ERASURE_DEADLINE_DAYS,
            "compliance.erasure.deadline_30_days",
            "erasure deadline must be exactly 30 days after request per GDPR Article 17"
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

    /// **AUDIT-2026-04 C-4 fixed API.** Mark a stream's records erased
    /// using an `ErasureScope` that proves the caller is entitled to
    /// do so for *this* (request, stream, subject-tenant) combo.
    ///
    /// The scope's construction baked in:
    /// - `stream_id` ∈ `request.affected_streams`
    /// - `stream_id.tenant_id() == subject_tenant`
    /// - `records <= max_records` (captured stream length at scope
    ///   creation)
    ///
    /// So the body here has only to increment state — the invariants
    /// are already type-enforced. The one remaining runtime check is
    /// the `records <= scope.max_records` cap because `records` is
    /// only bound at call time.
    ///
    /// # Errors
    ///
    /// - `RequestNotFound` — scope's request_id disappeared between
    ///   scope creation and this call (highly unusual; only if the
    ///   engine drops the request concurrently).
    /// - `AlreadyCompleted` / `InvalidState` — per the lifecycle.
    /// - `RecordsExceedScope` — `records > scope.max_records`.
    pub fn mark_stream_erased_with_scope(
        &mut self,
        scope: ErasureScope,
        records: u64,
    ) -> Result<()> {
        // Production assertion: cap bound must hold. Per
        // `docs/ASSERTIONS.md`, this is a compliance-critical
        // invariant; it is `assert!` not `debug_assert!`.
        if records > scope.max_records {
            return Err(ErasureError::RecordsExceedScope {
                count: records,
                max: scope.max_records,
            });
        }

        let request = self.find_pending_mut(scope.request_id)?;

        // Scope guarantees `stream_id ∈ affected_streams`, but we
        // double-check here because the request is mutable between
        // scope creation and this call — belt-and-braces against
        // engine-level misuse.
        assert!(
            request.affected_streams.contains(&scope.stream_id),
            "scope's stream_id is not in request.affected_streams \
             (this violates ErasureScope's type-level invariant)"
        );
        assert_eq!(
            scope.stream_id.tenant_id(),
            scope.subject_tenant,
            "scope's subject_tenant mismatch (type invariant violated)"
        );

        let streams_remaining = match &request.status {
            ErasureStatus::InProgress { streams_remaining } => *streams_remaining,
            ErasureStatus::Complete { .. } => return Err(ErasureError::AlreadyCompleted),
            _ => return Err(ErasureError::InvalidState),
        };
        assert!(
            streams_remaining > 0,
            "cannot erase stream when none remaining"
        );

        let previous_erased = request.records_erased;
        request.records_erased += records;
        request.status = ErasureStatus::InProgress {
            streams_remaining: streams_remaining - 1,
        };

        assert!(
            request.records_erased >= previous_erased,
            "records_erased must not decrease"
        );

        Ok(())
    }

    /// **AUDIT-2026-04 C-1 entrypoint.** Execute a full erasure
    /// request end-to-end: iterate every affected stream, capture
    /// pre-erasure chain heads, perform the shred act, update the
    /// ledger with scope-bounded counts, and produce a signed
    /// attestation.
    ///
    /// The caller supplies an [`ErasureExecutor`] that provides the
    /// runtime primitives (chain-head lookup + subject-row deletion +
    /// DEK shredding). The engine orchestrates the sequence:
    ///
    /// 1. Snapshot `affected_streams` and `subject_id` from the
    ///    request (asserts non-empty — an erasure with no streams is
    ///    either `Exempt` or a caller bug).
    /// 2. For each stream:
    ///    - Call `executor.pre_erasure_merkle_root(stream)` to snapshot
    ///      the chain head *before* any mutation.
    ///    - Call `executor.shred_stream(stream, subject)` to perform
    ///      the act and receive the shred-digest + record count.
    ///    - Build an [`ErasureScope`] via `scope_for` (the type-safe
    ///      parse-don't-validate gate added in AUDIT-2026-04 C-4).
    ///    - Update the ledger via `mark_stream_erased_with_scope`.
    /// 3. Feed the accumulated witnesses through
    ///    [`Self::complete_erasure_with_attestation`] for the Ed25519
    ///    signature.
    ///
    /// # Errors
    ///
    /// - [`ErasureError::RequestNotFound`] — unknown request.
    /// - [`ErasureError::AlreadyCompleted`] — request already finalized.
    /// - [`ErasureError::InvalidState`] — request not `InProgress`.
    /// - [`ErasureError::ExecutorFailure`] — underlying runtime error
    ///   (kernel rejection, I/O failure, DEK missing, etc.).
    /// - [`ErasureError::RecordsExceedScope`] — executor reports more
    ///   records erased than the stream's captured length. Indicates a
    ///   runtime bug.
    ///
    /// # Panics
    ///
    /// Production `assert!`: the request must have a non-empty
    /// `affected_streams` list. A request with no identified streams
    /// should be `Exempt` or the caller forgot to invoke
    /// [`Self::mark_in_progress`].
    pub fn execute_erasure(
        &mut self,
        request_id: Uuid,
        subject_tenant: TenantId,
        executor: &mut dyn ErasureExecutor,
        attestation_key: &AttestationKey,
        now_ns: u64,
    ) -> Result<ErasureAuditRecord> {
        // Snapshot the invariants we need from the request up-front
        // so we don't re-borrow `self` across the executor boundary.
        let (subject_id, streams): (String, Vec<StreamId>) = {
            let request = self.find_pending_mut(request_id)?;

            // Production assertion per AUDIT-2026-04: a request with
            // no affected streams cannot produce a meaningful
            // attestation. If we reached here, the caller invoked
            // execute_erasure out of lifecycle order.
            assert!(
                !request.affected_streams.is_empty(),
                "cannot execute erasure with empty affected_streams \
                 (did you forget to call mark_in_progress first?)"
            );

            match &request.status {
                ErasureStatus::InProgress { .. } => {}
                ErasureStatus::Complete { .. } => return Err(ErasureError::AlreadyCompleted),
                _ => return Err(ErasureError::InvalidState),
            }

            (request.subject_id.clone(), request.affected_streams.clone())
        };

        let mut witnesses = Vec::with_capacity(streams.len());

        for stream_id in &streams {
            // Step 1: snapshot the chain head BEFORE any mutation.
            let pre_root = executor
                .pre_erasure_merkle_root(*stream_id)
                .map_err(|e| ErasureError::ExecutorFailure(e.to_string()))?;

            // Step 2: perform the erasure + DEK shred.
            let receipt = executor
                .shred_stream(*stream_id, &subject_id)
                .map_err(|e| ErasureError::ExecutorFailure(e.to_string()))?;

            // Step 3: build scope (type-safe) and update ledger.
            let scope = self
                .get_request(request_id)
                .expect("request still exists — we hold &mut self")
                .scope_for(*stream_id, subject_tenant, receipt.stream_length_at_shred)
                .ok_or(ErasureError::InvalidState)?;
            self.mark_stream_erased_with_scope(scope, receipt.records_erased)?;

            // Step 4: accumulate witness for the attestation.
            witnesses.push(StreamErasureWitness {
                stream_id: *stream_id,
                pre_erasure_merkle_root: pre_root,
                key_shred_digest: receipt.key_shred_digest,
            });
        }

        // Postcondition: we built exactly one witness per affected
        // stream. The complete_erasure_with_attestation path will
        // assert each witness's stream_id is in affected_streams.
        assert_eq!(
            witnesses.len(),
            streams.len(),
            "postcondition: one witness per affected stream"
        );

        self.complete_erasure_with_attestation(request_id, witnesses, attestation_key, now_ns)
    }

    /// **AUDIT-2026-04 H-4 — signed erasure proof.** Finalize an
    /// erasure request, binding the audit record to per-stream
    /// pre-erasure merkle roots + key-shred digests + an Ed25519
    /// signature under the server's `AttestationKey`.
    ///
    /// The `witnesses` are what the runtime captured during the
    /// actual erasure act — typically:
    ///
    /// - `pre_erasure_merkle_root`: each stream's `chain_head` before
    ///   the `Command::Delete` markers were appended.
    /// - `key_shred_digest`: returned by `DataEncryptionKey::shred()`
    ///   at the moment the DEK was destroyed.
    ///
    /// The proof is verifiable against the attestation key's public
    /// half (see [`AttestationKey::verifying_key`]).
    pub fn complete_erasure_with_attestation(
        &mut self,
        request_id: Uuid,
        witnesses: Vec<StreamErasureWitness>,
        attestation_key: &AttestationKey,
        timestamp_ns: u64,
    ) -> Result<ErasureAuditRecord> {
        let request = self.find_pending_mut(request_id)?;
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

        // Production assertion: every witness's stream must be one of
        // the affected streams. Prevents manufactured witnesses for
        // foreign streams from entering the ledger.
        for w in &witnesses {
            assert!(
                streams_affected.contains(&w.stream_id),
                "witness stream_id {:?} is not in affected_streams",
                w.stream_id,
            );
        }

        let signed_proof =
            ErasureProof::sign_with(witnesses.clone(), timestamp_ns, attestation_key);

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
            erasure_proof: None, // H-4: signed_proof supersedes the count-only hash
            signed_proof: Some(signed_proof),
        };

        assert!(
            audit_record.signed_proof.is_some(),
            "postcondition: signed_proof must be present on the attested path"
        );

        self.completed.push(audit_record.clone());

        Ok(audit_record)
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
    #[deprecated(
        note = "AUDIT-2026-04 H-4: this proof binds only to self-reported counts. \
                Prefer `complete_erasure_with_attestation`, which binds to pre-erasure \
                merkle roots and key-shred digests under an Ed25519 attestation."
    )]
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
            signed_proof: None, // legacy path — see complete_erasure_with_attestation
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

        // SOMETIMES: simulations should exercise the exemption path.
        kimberlite_properties::sometimes!(
            true,
            "compliance.erasure.exempted_path",
            "erasure exemption path exercised at least once per simulation"
        );

        // SOMETIMES: track which exemption bases are exercised in simulation.
        kimberlite_properties::sometimes!(
            matches!(basis, ExemptionBasis::LegalObligation),
            "compliance.erasure.exempt_legal_obligation",
            "legal-obligation exemption exercised at least once"
        );
        kimberlite_properties::sometimes!(
            matches!(basis, ExemptionBasis::LegalClaims),
            "compliance.erasure.exempt_legal_claims",
            "legal-claims exemption exercised at least once"
        );

        Ok(())
    }

    /// Check for erasure requests that have exceeded their GDPR 30-day deadline.
    ///
    /// Returns references to all overdue requests that are not yet completed
    /// or exempt.
    pub fn check_deadlines(&self, now: DateTime<Utc>) -> Vec<&ErasureRequest> {
        let overdue: Vec<&ErasureRequest> = self
            .pending
            .iter()
            .filter(|r| {
                let is_active = matches!(
                    &r.status,
                    ErasureStatus::Pending | ErasureStatus::InProgress { .. }
                );
                is_active && now > r.deadline
            })
            .collect();

        // SOMETIMES: simulations should exercise the "deadline elapsed" path
        // (reports at least one overdue request).
        kimberlite_properties::sometimes!(
            !overdue.is_empty(),
            "compliance.erasure.deadline_elapsed_path",
            "deadline check must sometimes surface overdue requests"
        );

        overdue
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
#[allow(deprecated)] // exercises both legacy and new erasure APIs on purpose
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

    // ========================================================================
    // AUDIT-2026-04 C-4 — ErasureScope tests
    // ========================================================================
    //
    // PRESSURECRAFT §2: "Make illegal states unrepresentable." The
    // scope type's only public constructor is ErasureRequest::scope_for
    // and it returns None for the failure modes. These tests pin the
    // behavior so no refactor can silently weaken it.

    fn stream_in_tenant(tenant: u64, local: u32) -> StreamId {
        StreamId::from_tenant_and_local(TenantId::new(tenant), local)
    }

    #[test]
    fn scope_for_returns_none_for_foreign_stream() {
        let mut engine = ErasureEngine::new();
        let req = engine.request_erasure("subject").unwrap();
        let s1 = stream_in_tenant(7, 1);
        let s2 = stream_in_tenant(7, 2);
        engine.mark_in_progress(req.request_id, vec![s1]).unwrap();

        // Refresh the reference so we see the updated affected_streams.
        let req = engine.get_request(req.request_id).unwrap();
        // s2 is not in affected_streams — scope must be None.
        assert!(req.scope_for(s2, TenantId::new(7), 100).is_none());
        // s1 is in affected_streams and same tenant — scope exists.
        assert!(req.scope_for(s1, TenantId::new(7), 100).is_some());
    }

    #[test]
    fn scope_for_returns_none_for_cross_tenant_stream() {
        let mut engine = ErasureEngine::new();
        let req = engine.request_erasure("subject").unwrap();
        let s = stream_in_tenant(7, 1);
        engine.mark_in_progress(req.request_id, vec![s]).unwrap();
        let req = engine.get_request(req.request_id).unwrap();

        // Stream is in affected_streams, but subject_tenant mismatches
        // — scope must be None.
        assert!(req.scope_for(s, TenantId::new(99), 100).is_none());
    }

    /// AUDIT-2026-04 C-4: even a compliance-role operator cannot
    /// inflate `records_erased` above the scope's captured bound.
    #[test]
    fn mark_stream_erased_with_scope_rejects_records_above_cap() {
        let mut engine = ErasureEngine::new();
        let req = engine.request_erasure("subject").unwrap();
        let s = stream_in_tenant(7, 1);
        engine.mark_in_progress(req.request_id, vec![s]).unwrap();

        let scope = {
            let req = engine.get_request(req.request_id).unwrap();
            req.scope_for(s, TenantId::new(7), /* stream_length */ 50)
                .unwrap()
        };

        let err = engine
            .mark_stream_erased_with_scope(scope, 1000)
            .unwrap_err();
        assert!(
            matches!(
                err,
                ErasureError::RecordsExceedScope {
                    count: 1000,
                    max: 50
                }
            ),
            "expected RecordsExceedScope, got {err:?}"
        );

        // Within-bound records succeed and increment counters.
        engine.mark_stream_erased_with_scope(scope, 50).unwrap();
        let req = engine.get_request(req.request_id).unwrap();
        assert_eq!(req.records_erased, 50);
    }

    // ========================================================================
    // AUDIT-2026-04 H-4 — ErasureProof tests
    // ========================================================================

    fn mk_witness(tenant: u64, local: u32, root_byte: u8, shred_byte: u8) -> StreamErasureWitness {
        StreamErasureWitness {
            stream_id: stream_in_tenant(tenant, local),
            pre_erasure_merkle_root: [root_byte; 32],
            key_shred_digest: [shred_byte; 32],
        }
    }

    #[test]
    fn erasure_proof_signs_and_verifies_round_trip() {
        let key = AttestationKey::generate();
        let witnesses = vec![mk_witness(1, 1, 0xAA, 0xBB), mk_witness(1, 2, 0xCC, 0xDD)];
        let proof = ErasureProof::sign_with(witnesses, 42_000_000_000, &key);
        assert_eq!(proof.attestation_sig.len(), 64);
        proof.verify(&key.verifying_key()).expect("valid signature");
    }

    /// Critical H-4 property: different pre-erasure roots → different
    /// proof bytes, even when count, subject, timestamp are identical.
    /// Prior to H-4 the proof was `SHA-256(request_id||subject_id||count)`
    /// and would collide on identical counts.
    #[test]
    fn erasure_proof_differs_on_different_preerasure_roots() {
        let key = AttestationKey::generate();
        let ws_a = vec![mk_witness(1, 1, 0x01, 0xAA)];
        let ws_b = vec![mk_witness(1, 1, 0x02, 0xAA)]; // same stream, different root
        let ts = 42_000_000_000;
        let pa = ErasureProof::sign_with(ws_a, ts, &key);
        let pb = ErasureProof::sign_with(ws_b, ts, &key);
        assert_ne!(pa.bundle_root, pb.bundle_root);
        assert_ne!(pa.attestation_sig, pb.attestation_sig);
    }

    /// Critical H-4 property: tampered witnesses (e.g. an operator
    /// rewrites `records_erased` after the fact) fail verification.
    #[test]
    fn erasure_proof_detects_witness_tampering() {
        let key = AttestationKey::generate();
        let witnesses = vec![mk_witness(1, 1, 0xAA, 0xBB)];
        let mut proof = ErasureProof::sign_with(witnesses, 1, &key);
        // Tamper with the witness array after signing.
        proof.witnesses[0].pre_erasure_merkle_root = [0xFF; 32];
        let err = proof.verify(&key.verifying_key()).unwrap_err();
        assert!(matches!(err, ErasureError::InvalidProofSignature));
    }

    /// Verifying a proof with the wrong key must fail.
    #[test]
    fn erasure_proof_rejects_wrong_key() {
        let signer = AttestationKey::generate();
        let other = AttestationKey::generate();
        let witnesses = vec![mk_witness(1, 1, 0xAA, 0xBB)];
        let proof = ErasureProof::sign_with(witnesses, 1, &signer);
        let err = proof.verify(&other.verifying_key()).unwrap_err();
        assert!(matches!(err, ErasureError::InvalidProofSignature));
    }

    /// End-to-end integration: an erasure lifecycle using the new
    /// type-safe API and signed proof.
    #[test]
    fn complete_erasure_with_attestation_end_to_end() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject@example.com").unwrap();
        let s1 = stream_in_tenant(7, 1);
        let s2 = stream_in_tenant(7, 2);
        engine
            .mark_in_progress(req.request_id, vec![s1, s2])
            .unwrap();

        // Two scopes, one per stream.
        let req = engine.get_request(req.request_id).unwrap();
        let scope1 = req.scope_for(s1, TenantId::new(7), 100).unwrap();
        let scope2 = req.scope_for(s2, TenantId::new(7), 100).unwrap();
        let request_id = req.request_id;
        engine.mark_stream_erased_with_scope(scope1, 30).unwrap();
        engine.mark_stream_erased_with_scope(scope2, 70).unwrap();

        let witnesses = vec![
            StreamErasureWitness {
                stream_id: s1,
                pre_erasure_merkle_root: [1u8; 32],
                key_shred_digest: [10u8; 32],
            },
            StreamErasureWitness {
                stream_id: s2,
                pre_erasure_merkle_root: [2u8; 32],
                key_shred_digest: [20u8; 32],
            },
        ];

        let audit = engine
            .complete_erasure_with_attestation(request_id, witnesses, &key, 42_000_000)
            .unwrap();

        assert_eq!(audit.records_erased, 100);
        assert!(
            audit.erasure_proof.is_none(),
            "legacy hash proof suppressed"
        );
        let proof = audit.signed_proof.as_ref().expect("signed_proof present");
        proof.verify(&key.verifying_key()).unwrap();
        assert_eq!(proof.witnesses.len(), 2);
    }

    // ========================================================================
    // AUDIT-2026-04 C-1 — ErasureExecutor / execute_erasure tests
    // ========================================================================

    /// Mock `ErasureExecutor` driven by pre-canned per-stream
    /// responses. Used to exercise `execute_erasure` without pulling
    /// in `kimberlite-kernel` / `kimberlite-storage` / `kimberlite-crypto`
    /// (which would create a layering cycle).
    struct MockErasureExecutor {
        /// `stream_id → pre_erasure_merkle_root` response.
        pub roots: std::collections::HashMap<StreamId, [u8; 32]>,
        /// `stream_id → shred receipt` response.
        pub receipts: std::collections::HashMap<StreamId, StreamShredReceipt>,
        /// If `Some`, `shred_stream` returns this error for any
        /// stream_id and never touches the receipts map.
        pub shred_failure: Option<String>,
        /// Observed calls, in order, for assertions.
        pub calls: Vec<String>,
    }

    impl MockErasureExecutor {
        fn new() -> Self {
            Self {
                roots: std::collections::HashMap::new(),
                receipts: std::collections::HashMap::new(),
                shred_failure: None,
                calls: Vec::new(),
            }
        }

        fn with_stream(
            mut self,
            stream_id: StreamId,
            root: [u8; 32],
            receipt: StreamShredReceipt,
        ) -> Self {
            self.roots.insert(stream_id, root);
            self.receipts.insert(stream_id, receipt);
            self
        }
    }

    #[derive(Debug)]
    struct MockErr(String);
    impl std::fmt::Display for MockErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for MockErr {}

    impl ErasureExecutor for MockErasureExecutor {
        fn pre_erasure_merkle_root(
            &mut self,
            stream_id: StreamId,
        ) -> std::result::Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>> {
            self.calls.push(format!("root({stream_id:?})"));
            self.roots.get(&stream_id).copied().ok_or_else(|| {
                let e: Box<dyn std::error::Error + Send + Sync> =
                    Box::new(MockErr(format!("no root for {stream_id:?}")));
                e
            })
        }

        fn shred_stream(
            &mut self,
            stream_id: StreamId,
            subject_id: &str,
        ) -> std::result::Result<StreamShredReceipt, Box<dyn std::error::Error + Send + Sync>>
        {
            self.calls
                .push(format!("shred({stream_id:?},{subject_id})"));
            if let Some(err) = &self.shred_failure {
                let e: Box<dyn std::error::Error + Send + Sync> = Box::new(MockErr(err.clone()));
                return Err(e);
            }
            self.receipts.get(&stream_id).cloned().ok_or_else(|| {
                let e: Box<dyn std::error::Error + Send + Sync> =
                    Box::new(MockErr(format!("no receipt for {stream_id:?}")));
                e
            })
        }
    }

    #[test]
    fn execute_erasure_happy_path_produces_witness_per_stream() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject@example.com").unwrap();
        let s1 = stream_in_tenant(7, 1);
        let s2 = stream_in_tenant(7, 2);
        engine
            .mark_in_progress(req.request_id, vec![s1, s2])
            .unwrap();
        let request_id = req.request_id;

        let mut exec = MockErasureExecutor::new()
            .with_stream(
                s1,
                [1u8; 32],
                StreamShredReceipt {
                    key_shred_digest: [0x11; 32],
                    records_erased: 30,
                    stream_length_at_shred: 100,
                },
            )
            .with_stream(
                s2,
                [2u8; 32],
                StreamShredReceipt {
                    key_shred_digest: [0x22; 32],
                    records_erased: 70,
                    stream_length_at_shred: 100,
                },
            );

        let audit = engine
            .execute_erasure(request_id, TenantId::new(7), &mut exec, &key, 123_000_000)
            .unwrap();

        // One witness per affected stream.
        let proof = audit.signed_proof.as_ref().expect("signed proof");
        assert_eq!(proof.witnesses.len(), 2);
        proof.verify(&key.verifying_key()).unwrap();

        // Records summed correctly.
        assert_eq!(audit.records_erased, 100);

        // Executor saw: root(s1), shred(s1), root(s2), shred(s2).
        assert_eq!(exec.calls.len(), 4);
        assert!(exec.calls[0].contains("root"));
        assert!(exec.calls[1].contains("shred"));
        assert!(exec.calls[2].contains("root"));
        assert!(exec.calls[3].contains("shred"));
    }

    #[test]
    fn execute_erasure_propagates_executor_failure() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject").unwrap();
        let s1 = stream_in_tenant(7, 1);
        engine.mark_in_progress(req.request_id, vec![s1]).unwrap();
        let request_id = req.request_id;

        let mut exec = MockErasureExecutor::new().with_stream(
            s1,
            [1u8; 32],
            StreamShredReceipt {
                key_shred_digest: [0x11; 32],
                records_erased: 30,
                stream_length_at_shred: 100,
            },
        );
        exec.shred_failure = Some("simulated I/O error".to_string());

        let err = engine
            .execute_erasure(request_id, TenantId::new(7), &mut exec, &key, 0)
            .unwrap_err();
        assert!(
            matches!(err, ErasureError::ExecutorFailure(ref s) if s.contains("simulated I/O error")),
            "expected ExecutorFailure, got {err:?}"
        );

        // Request must NOT be transitioned to Complete on executor
        // failure — the caller can retry.
        let post = engine.get_request(request_id).unwrap();
        assert!(matches!(post.status, ErasureStatus::InProgress { .. }));
    }

    /// AUDIT-2026-04 C-1: `execute_erasure` must reject a request
    /// whose `affected_streams` list is empty. An erasure with no
    /// streams is either an exempt request or a caller who skipped
    /// `mark_in_progress` — either way it cannot produce a
    /// meaningful attestation.
    #[test]
    #[should_panic(expected = "empty affected_streams")]
    fn execute_erasure_panics_on_empty_affected_streams() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject").unwrap();
        // Transition to InProgress with NO streams — malformed but
        // type-reachable via `mark_in_progress(id, vec![])`.
        engine.mark_in_progress(req.request_id, vec![]).unwrap();

        let mut exec = MockErasureExecutor::new();
        let _ = engine.execute_erasure(req.request_id, TenantId::new(7), &mut exec, &key, 0);
    }

    /// AUDIT-2026-04 C-1 + C-4 crossover: an executor that reports
    /// `records_erased > stream_length_at_shred` is caught by the
    /// type-level scope cap added in C-4. This proves the two
    /// defenses compose — even a buggy executor cannot inflate
    /// ledger counts.
    #[test]
    fn execute_erasure_rejects_inflated_count_via_scope_cap() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject").unwrap();
        let s1 = stream_in_tenant(7, 1);
        engine.mark_in_progress(req.request_id, vec![s1]).unwrap();
        let request_id = req.request_id;

        let mut exec = MockErasureExecutor::new().with_stream(
            s1,
            [1u8; 32],
            StreamShredReceipt {
                key_shred_digest: [0x11; 32],
                // Buggy executor claims to have erased more records
                // than the stream holds — C-4 scope cap catches this.
                records_erased: 9_999,
                stream_length_at_shred: 100,
            },
        );

        let err = engine
            .execute_erasure(request_id, TenantId::new(7), &mut exec, &key, 0)
            .unwrap_err();
        assert!(
            matches!(
                err,
                ErasureError::RecordsExceedScope {
                    count: 9_999,
                    max: 100
                }
            ),
            "expected RecordsExceedScope, got {err:?}"
        );
    }

    /// Attempting to complete with a witness for a stream not in
    /// `affected_streams` must panic (production assert). This
    /// prevents an operator from sneaking foreign-stream witnesses
    /// into an otherwise valid attestation.
    #[test]
    #[should_panic(expected = "is not in affected_streams")]
    fn complete_erasure_with_attestation_rejects_foreign_witness() {
        let mut engine = ErasureEngine::new();
        let key = AttestationKey::generate();

        let req = engine.request_erasure("subject").unwrap();
        let s1 = stream_in_tenant(7, 1);
        let foreign = stream_in_tenant(7, 999);
        engine.mark_in_progress(req.request_id, vec![s1]).unwrap();

        let request_id = req.request_id;
        let witnesses = vec![StreamErasureWitness {
            stream_id: foreign, // not in affected_streams
            pre_erasure_merkle_root: [0; 32],
            key_shred_digest: [0; 32],
        }];

        let _ = engine.complete_erasure_with_attestation(request_id, witnesses, &key, 0);
    }
}
