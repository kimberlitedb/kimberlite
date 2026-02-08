//! Enhanced Audit Logging for Compliance Operations
//!
//! Implements comprehensive audit event tracking satisfying:
//! - **SOC 2 CC7.2** - Change Detection (monitoring and logging of all changes)
//! - **ISO 27001 A.12.4.1** - Event Logging (recording user activities, exceptions, faults)
//!
//! # Architecture
//!
//! ```text
//! ComplianceAuditLog = {
//!     events: Vec<ComplianceAuditEvent>,   // Append-only, immutable
//!     append(action, actor, tenant) -> Uuid,
//!     query(filter) -> Vec<&Event>,
//!     export_json(filter) -> String,
//! }
//! ```
//!
//! The audit log is append-only: events cannot be modified or deleted after
//! insertion. This guarantees `AuditLogImmutability` -- a core compliance
//! property proven in the TLA+ meta-framework.
//!
//! # Example
//!
//! ```
//! use kimberlite_compliance::audit::{ComplianceAuditLog, ComplianceAuditAction, AuditQuery};
//! use uuid::Uuid;
//!
//! let mut log = ComplianceAuditLog::new();
//!
//! // Record a consent event
//! let event_id = log.append(
//!     ComplianceAuditAction::ConsentGranted {
//!         subject_id: "user@example.com".into(),
//!         purpose: "Marketing".into(),
//!         scope: "ContactInfo".into(),
//!     },
//!     Some("admin".into()),
//!     Some(1),
//! );
//!
//! // Query by subject
//! let query = AuditQuery::default().with_subject("user@example.com");
//! let results = log.query(&query);
//! assert_eq!(results.len(), 1);
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("Audit event not found: {0}")]
    EventNotFound(Uuid),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AuditError>;

/// Extended audit actions covering all compliance modules.
///
/// Each variant captures the structured context needed for compliance
/// reporting and forensic analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplianceAuditAction {
    // -- Consent management (GDPR Article 7) --
    /// Consent was granted by a data subject
    ConsentGranted {
        subject_id: String,
        purpose: String,
        scope: String,
    },
    /// Consent was withdrawn by a data subject
    ConsentWithdrawn {
        subject_id: String,
        consent_id: Uuid,
    },

    // -- Erasure (GDPR Article 17) --
    /// A data erasure request was submitted
    ErasureRequested {
        subject_id: String,
        request_id: Uuid,
    },
    /// Erasure completed successfully
    ErasureCompleted {
        subject_id: String,
        records_erased: u64,
        request_id: Uuid,
    },
    /// Erasure was denied under a legal exemption
    ErasureExempted {
        subject_id: String,
        request_id: Uuid,
        basis: String,
    },

    // -- Field masking --
    /// A field masking rule was applied
    FieldMasked {
        column: String,
        strategy: String,
        role: String,
    },

    // -- Breach detection (GDPR Article 33/34) --
    /// A potential data breach was detected
    BreachDetected {
        event_id: Uuid,
        severity: String,
        indicator: String,
        affected_subjects: Vec<String>,
    },
    /// Breach notification was sent to authorities/subjects
    BreachNotified {
        event_id: Uuid,
        notified_at: DateTime<Utc>,
        affected_subjects: Vec<String>,
    },
    /// A breach was resolved with remediation
    BreachResolved {
        event_id: Uuid,
        remediation: String,
        affected_subjects: Vec<String>,
    },

    // -- Data portability (GDPR Article 20) --
    /// Data was exported for a subject
    DataExported {
        subject_id: String,
        export_id: Uuid,
        format: String,
        record_count: u64,
    },

    // -- Access control --
    /// Access was granted to a resource
    AccessGranted {
        user_id: String,
        resource: String,
        role: String,
    },
    /// Access was denied to a resource
    AccessDenied {
        user_id: String,
        resource: String,
        reason: String,
    },

    // -- Policy changes --
    /// A compliance policy was changed
    PolicyChanged {
        policy_type: String,
        changed_by: String,
        details: String,
    },

    // -- PCI DSS tokenization (Requirement 3.4) --
    /// Data tokenization was applied to cardholder data
    TokenizationApplied {
        column: String,
        token_format: String,
        record_count: u64,
    },

    // -- 21 CFR Part 11 electronic signatures --
    /// An electronic record was signed (per-record Ed25519 signature)
    RecordSigned {
        record_id: String,
        signer_id: String,
        meaning: String,
    },
}

impl ComplianceAuditAction {
    /// Returns the action type prefix for filtering (e.g., "Consent", "Erasure").
    fn action_type_prefix(&self) -> &'static str {
        match self {
            Self::ConsentGranted { .. } | Self::ConsentWithdrawn { .. } => "Consent",
            Self::ErasureRequested { .. }
            | Self::ErasureCompleted { .. }
            | Self::ErasureExempted { .. } => "Erasure",
            Self::FieldMasked { .. } => "FieldMasked",
            Self::BreachDetected { .. }
            | Self::BreachNotified { .. }
            | Self::BreachResolved { .. } => "Breach",
            Self::DataExported { .. } => "DataExported",
            Self::AccessGranted { .. } | Self::AccessDenied { .. } => "Access",
            Self::PolicyChanged { .. } => "PolicyChanged",
            Self::TokenizationApplied { .. } => "Tokenization",
            Self::RecordSigned { .. } => "RecordSigned",
        }
    }

    /// Check if this action references the given subject identifier.
    ///
    /// Inspects all string fields that could represent a data subject.
    fn matches_subject(&self, subject_id: &str) -> bool {
        match self {
            Self::ConsentGranted {
                subject_id: sid, ..
            }
            | Self::ConsentWithdrawn {
                subject_id: sid, ..
            }
            | Self::ErasureRequested {
                subject_id: sid, ..
            }
            | Self::ErasureCompleted {
                subject_id: sid, ..
            }
            | Self::ErasureExempted {
                subject_id: sid, ..
            }
            | Self::DataExported {
                subject_id: sid, ..
            } => sid == subject_id,

            Self::AccessGranted { user_id, .. } | Self::AccessDenied { user_id, .. } => {
                user_id == subject_id
            }

            Self::PolicyChanged { changed_by, .. } => changed_by == subject_id,

            Self::RecordSigned { signer_id, .. } => signer_id == subject_id,

            Self::BreachDetected {
                affected_subjects, ..
            }
            | Self::BreachNotified {
                affected_subjects, ..
            }
            | Self::BreachResolved {
                affected_subjects, ..
            } => affected_subjects.iter().any(|s| s == subject_id),

            // Actions without subject identifiers
            Self::FieldMasked { .. } | Self::TokenizationApplied { .. } => false,
        }
    }
}

/// A single audit event with full context.
///
/// Once appended to the log, an event is immutable. All fields are set at
/// creation time and cannot be changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceAuditEvent {
    /// Unique event identifier
    pub event_id: Uuid,
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// What happened
    pub action: ComplianceAuditAction,
    /// Who performed the action (operator, system, etc.)
    pub actor: Option<String>,
    /// Tenant context (multi-tenant isolation)
    pub tenant_id: Option<u64>,
    /// Source IP address for access auditing
    pub ip_address: Option<String>,
    /// Correlation ID linking related events across a workflow
    pub correlation_id: Option<Uuid>,
    /// Source country for location-based audit trail (ISO 3166-1 alpha-2)
    pub source_country: Option<String>,
}

/// Query filter for the audit log.
///
/// All fields are optional. When multiple fields are set, they are combined
/// with AND logic. Use builder methods for ergonomic construction.
#[derive(Debug, Default, Clone)]
pub struct AuditQuery {
    pub subject_id: Option<String>,
    pub action_type: Option<String>,
    pub time_from: Option<DateTime<Utc>>,
    pub time_to: Option<DateTime<Utc>>,
    pub actor: Option<String>,
    pub tenant_id: Option<u64>,
    pub limit: Option<usize>,
}

impl AuditQuery {
    /// Filter by data subject identifier.
    pub fn with_subject(mut self, subject_id: &str) -> Self {
        self.subject_id = Some(subject_id.to_string());
        self
    }

    /// Filter by action type prefix (e.g., "Consent", "Erasure", "Breach").
    pub fn with_action_type(mut self, action_type: &str) -> Self {
        self.action_type = Some(action_type.to_string());
        self
    }

    /// Filter to events within a time range (inclusive).
    pub fn with_time_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.time_from = Some(from);
        self.time_to = Some(to);
        self
    }

    /// Filter by the actor who performed the action.
    pub fn with_actor(mut self, actor: &str) -> Self {
        self.actor = Some(actor.to_string());
        self
    }

    /// Filter by tenant ID.
    pub fn with_tenant(mut self, tenant_id: u64) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Limit the number of results returned.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Immutable, append-only audit log for compliance operations.
///
/// Satisfies:
/// - SOC 2 CC7.2: All compliance-relevant changes are recorded
/// - ISO 27001 A.12.4.1: User activities and security events are logged
/// - GDPR Article 30: Records of processing activities
///
/// The log enforces append-only semantics: events can be added but never
/// modified or removed. This is a structural guarantee -- the API provides
/// no mutation or deletion methods.
#[derive(Debug, Default)]
pub struct ComplianceAuditLog {
    events: Vec<ComplianceAuditEvent>,
}

impl ComplianceAuditLog {
    /// Create a new empty audit log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an audit event and return its unique ID.
    ///
    /// This is the primary entry point for recording compliance events.
    /// The event is timestamped at the moment of insertion.
    ///
    /// # Assertions
    ///
    /// - Post: event count increases by exactly 1
    pub fn append(
        &mut self,
        action: ComplianceAuditAction,
        actor: Option<String>,
        tenant_id: Option<u64>,
    ) -> Uuid {
        self.append_with_context(action, actor, tenant_id, None, None)
    }

    /// Append an audit event with full contextual metadata.
    ///
    /// Use this when IP address or correlation ID are available.
    ///
    /// # Assertions
    ///
    /// - Post: event count increases by exactly 1
    /// - Post: the returned ID can be retrieved with `get_event`
    pub fn append_with_context(
        &mut self,
        action: ComplianceAuditAction,
        actor: Option<String>,
        tenant_id: Option<u64>,
        ip_address: Option<String>,
        correlation_id: Option<Uuid>,
    ) -> Uuid {
        let count_before = self.events.len();

        let event_id = Uuid::new_v4();
        let event = ComplianceAuditEvent {
            event_id,
            timestamp: Utc::now(),
            action,
            actor,
            tenant_id,
            ip_address,
            correlation_id,
            source_country: None,
        };

        self.events.push(event);

        // Post-condition: exactly one event was added
        assert_eq!(
            self.events.len(),
            count_before + 1,
            "Audit log append must increase event count by exactly 1"
        );

        event_id
    }

    /// Query events matching the given filter.
    ///
    /// All filter fields use AND logic. An empty query returns all events.
    /// Results are returned in insertion order (chronological).
    pub fn query(&self, filter: &AuditQuery) -> Vec<&ComplianceAuditEvent> {
        let mut results: Vec<&ComplianceAuditEvent> = self
            .events
            .iter()
            .filter(|event| Self::matches_filter(event, filter))
            .collect();

        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        results
    }

    /// Look up a single event by its unique ID.
    pub fn get_event(&self, event_id: Uuid) -> Option<&ComplianceAuditEvent> {
        self.events.iter().find(|e| e.event_id == event_id)
    }

    /// Return all events recorded since the given timestamp (inclusive).
    pub fn events_since(&self, since: DateTime<Utc>) -> Vec<&ComplianceAuditEvent> {
        self.events
            .iter()
            .filter(|e| e.timestamp >= since)
            .collect()
    }

    /// Return all events that reference the given data subject.
    ///
    /// Inspects the action payload of each event to find matching subject
    /// identifiers, user IDs, or actor fields.
    pub fn events_for_subject(&self, subject_id: &str) -> Vec<&ComplianceAuditEvent> {
        assert!(
            !subject_id.is_empty(),
            "subject_id must not be empty for subject query"
        );

        self.events
            .iter()
            .filter(|e| e.action.matches_subject(subject_id))
            .collect()
    }

    /// Total number of events in the log.
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// Export filtered events as a JSON array string.
    ///
    /// Useful for compliance reporting, external audit tools, and SIEM
    /// integration.
    pub fn export_json(&self, filter: &AuditQuery) -> Result<String> {
        let events = self.query(filter);
        serde_json::to_string_pretty(&events).map_err(AuditError::from)
    }

    /// Check whether a single event matches all active filter criteria.
    fn matches_filter(event: &ComplianceAuditEvent, filter: &AuditQuery) -> bool {
        // Subject filter
        if let Some(ref subject_id) = filter.subject_id {
            if !event.action.matches_subject(subject_id) {
                return false;
            }
        }

        // Action type prefix filter
        if let Some(ref action_type) = filter.action_type {
            if !event
                .action
                .action_type_prefix()
                .starts_with(action_type.as_str())
            {
                return false;
            }
        }

        // Time range filter (inclusive)
        if let Some(from) = filter.time_from {
            if event.timestamp < from {
                return false;
            }
        }
        if let Some(to) = filter.time_to {
            if event.timestamp > to {
                return false;
            }
        }

        // Actor filter
        if let Some(ref actor) = filter.actor {
            match &event.actor {
                Some(event_actor) if event_actor == actor => {}
                _ => return false,
            }
        }

        // Tenant filter
        if let Some(tenant_id) = filter.tenant_id {
            match event.tenant_id {
                Some(event_tenant) if event_tenant == tenant_id => {}
                _ => return false,
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_append_event() {
        let mut log = ComplianceAuditLog::new();
        assert_eq!(log.count(), 0);

        let event_id = log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "user@example.com".into(),
                purpose: "Marketing".into(),
                scope: "ContactInfo".into(),
            },
            Some("admin".into()),
            Some(1),
        );

        assert_eq!(log.count(), 1);

        let event = log
            .get_event(event_id)
            .expect("event must exist after append");
        assert_eq!(event.event_id, event_id);
        assert_eq!(event.actor.as_deref(), Some("admin"));
        assert_eq!(event.tenant_id, Some(1));
    }

    #[test]
    fn test_query_by_subject() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "alice@example.com".into(),
                purpose: "Analytics".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::ErasureRequested {
                subject_id: "bob@example.com".into(),
                request_id: Uuid::new_v4(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::ConsentWithdrawn {
                subject_id: "alice@example.com".into(),
                consent_id: Uuid::new_v4(),
            },
            None,
            None,
        );

        let query = AuditQuery::default().with_subject("alice@example.com");
        let results = log.query(&query);
        assert_eq!(results.len(), 2, "should find both events for alice");

        let query = AuditQuery::default().with_subject("bob@example.com");
        let results = log.query(&query);
        assert_eq!(results.len(), 1, "should find one event for bob");

        let query = AuditQuery::default().with_subject("nobody@example.com");
        let results = log.query(&query);
        assert_eq!(
            results.len(),
            0,
            "should find no events for unknown subject"
        );
    }

    #[test]
    fn test_query_by_action_type() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "user@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::ConsentWithdrawn {
                subject_id: "user@example.com".into(),
                consent_id: Uuid::new_v4(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::ErasureRequested {
                subject_id: "user@example.com".into(),
                request_id: Uuid::new_v4(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::BreachDetected {
                event_id: Uuid::new_v4(),
                severity: "High".into(),
                indicator: "unusual_access".into(),
                affected_subjects: vec![],
            },
            None,
            None,
        );

        // "Consent" prefix should match ConsentGranted and ConsentWithdrawn
        let query = AuditQuery::default().with_action_type("Consent");
        let results = log.query(&query);
        assert_eq!(results.len(), 2, "Consent prefix should match 2 events");

        // "Erasure" prefix should match ErasureRequested
        let query = AuditQuery::default().with_action_type("Erasure");
        let results = log.query(&query);
        assert_eq!(results.len(), 1, "Erasure prefix should match 1 event");

        // "Breach" prefix should match BreachDetected
        let query = AuditQuery::default().with_action_type("Breach");
        let results = log.query(&query);
        assert_eq!(results.len(), 1, "Breach prefix should match 1 event");
    }

    #[test]
    fn test_query_by_time_range() {
        let mut log = ComplianceAuditLog::new();

        // Insert events with known timing
        let before = Utc::now();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "user@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        let after_first = Utc::now();

        // Use events_since to verify time-based filtering
        let since_before = log.events_since(before);
        assert_eq!(since_before.len(), 1, "event should be after 'before'");

        // Query with time range that includes the event
        let query = AuditQuery::default().with_time_range(before, after_first);
        let results = log.query(&query);
        assert_eq!(results.len(), 1, "event should be in range");

        // Query with time range in the past
        let past_start = before - Duration::hours(2);
        let past_end = before - Duration::hours(1);
        let query = AuditQuery::default().with_time_range(past_start, past_end);
        let results = log.query(&query);
        assert_eq!(results.len(), 0, "no events should be in past range");
    }

    #[test]
    fn test_query_by_actor() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::PolicyChanged {
                policy_type: "retention".into(),
                changed_by: "cto".into(),
                details: "extended to 7 years".into(),
            },
            Some("admin_alice".into()),
            None,
        );

        log.append(
            ComplianceAuditAction::AccessGranted {
                user_id: "new_hire".into(),
                resource: "production_db".into(),
                role: "reader".into(),
            },
            Some("admin_bob".into()),
            None,
        );

        let query = AuditQuery::default().with_actor("admin_alice");
        let results = log.query(&query);
        assert_eq!(results.len(), 1);

        let query = AuditQuery::default().with_actor("admin_bob");
        let results = log.query(&query);
        assert_eq!(results.len(), 1);

        let query = AuditQuery::default().with_actor("unknown");
        let results = log.query(&query);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_immutability() {
        let mut log = ComplianceAuditLog::new();

        let event_id = log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "user@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            Some("admin".into()),
            Some(1),
        );

        // Retrieve event -- we only get a shared reference
        let event = log.get_event(event_id).expect("event must exist");
        let original_timestamp = event.timestamp;
        let original_actor = event.actor.clone();

        // Append another event -- the first event must be unchanged
        log.append(
            ComplianceAuditAction::ErasureRequested {
                subject_id: "other@example.com".into(),
                request_id: Uuid::new_v4(),
            },
            Some("other_admin".into()),
            None,
        );

        let event = log
            .get_event(event_id)
            .expect("original event must still exist");
        assert_eq!(event.timestamp, original_timestamp);
        assert_eq!(event.actor, original_actor);
        assert_eq!(log.count(), 2);
    }

    #[test]
    fn test_export_json() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::AccessGranted {
                user_id: "alice".into(),
                resource: "secrets".into(),
                role: "admin".into(),
            },
            Some("system".into()),
            Some(42),
        );

        log.append(
            ComplianceAuditAction::AccessDenied {
                user_id: "mallory".into(),
                resource: "secrets".into(),
                reason: "insufficient_permissions".into(),
            },
            Some("system".into()),
            Some(42),
        );

        let json = log
            .export_json(&AuditQuery::default())
            .expect("export must succeed");

        // Verify it's valid JSON by parsing it back
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&json).expect("exported JSON must parse");
        assert_eq!(parsed.len(), 2);

        // Verify content
        assert!(json.contains("AccessGranted"));
        assert!(json.contains("AccessDenied"));
        assert!(json.contains("alice"));
        assert!(json.contains("mallory"));
    }

    #[test]
    fn test_correlation_id() {
        let mut log = ComplianceAuditLog::new();
        let correlation = Uuid::new_v4();

        // Two events sharing a correlation ID (e.g., breach lifecycle)
        let detect_id = log.append_with_context(
            ComplianceAuditAction::BreachDetected {
                event_id: Uuid::new_v4(),
                severity: "Critical".into(),
                indicator: "exfiltration_pattern".into(),
                affected_subjects: vec![],
            },
            Some("ids_system".into()),
            Some(1),
            Some("10.0.0.1".into()),
            Some(correlation),
        );

        let notify_id = log.append_with_context(
            ComplianceAuditAction::BreachNotified {
                event_id: Uuid::new_v4(),
                notified_at: Utc::now(),
                affected_subjects: vec![],
            },
            Some("ids_system".into()),
            Some(1),
            Some("10.0.0.1".into()),
            Some(correlation),
        );

        // An unrelated event
        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "unrelated@example.com".into(),
                purpose: "Analytics".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        let detect_event = log.get_event(detect_id).expect("detect event must exist");
        let notify_event = log.get_event(notify_id).expect("notify event must exist");

        assert_eq!(detect_event.correlation_id, Some(correlation));
        assert_eq!(notify_event.correlation_id, Some(correlation));
        assert_eq!(detect_event.correlation_id, notify_event.correlation_id);

        // Verify ip_address was recorded
        assert_eq!(detect_event.ip_address.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn test_empty_query_returns_all() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "a@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::ErasureRequested {
                subject_id: "b@example.com".into(),
                request_id: Uuid::new_v4(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::BreachDetected {
                event_id: Uuid::new_v4(),
                severity: "Low".into(),
                indicator: "anomaly".into(),
                affected_subjects: vec![],
            },
            None,
            None,
        );

        let query = AuditQuery::default();
        let results = log.query(&query);
        assert_eq!(results.len(), 3, "empty query must return all events");
        assert_eq!(results.len(), log.count());
    }

    #[test]
    fn test_query_with_limit() {
        let mut log = ComplianceAuditLog::new();

        for i in 0..10 {
            log.append(
                ComplianceAuditAction::AccessGranted {
                    user_id: format!("user_{i}"),
                    resource: "db".into(),
                    role: "reader".into(),
                },
                None,
                None,
            );
        }

        let query = AuditQuery::default().with_limit(3);
        let results = log.query(&query);
        assert_eq!(results.len(), 3, "limit should cap results");

        // Results should be the first 3 (insertion order)
        assert!(results[0].action.matches_subject("user_0"));
    }

    #[test]
    fn test_events_for_subject() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "target@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::DataExported {
                subject_id: "target@example.com".into(),
                export_id: Uuid::new_v4(),
                format: "JSON".into(),
                record_count: 42,
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::AccessDenied {
                user_id: "other@example.com".into(),
                resource: "db".into(),
                reason: "no_role".into(),
            },
            None,
            None,
        );

        let results = log.events_for_subject("target@example.com");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_query_combined_filters() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "alice@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            Some("admin".into()),
            Some(1),
        );

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "alice@example.com".into(),
                purpose: "Analytics".into(),
                scope: "AllData".into(),
            },
            Some("admin".into()),
            Some(2),
        );

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "bob@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            Some("admin".into()),
            Some(1),
        );

        // Combine subject + tenant filter
        let query = AuditQuery::default()
            .with_subject("alice@example.com")
            .with_tenant(1);
        let results = log.query(&query);
        assert_eq!(
            results.len(),
            1,
            "only alice's event in tenant 1 should match"
        );
    }

    #[test]
    fn test_export_json_with_filter() {
        let mut log = ComplianceAuditLog::new();

        log.append(
            ComplianceAuditAction::ConsentGranted {
                subject_id: "user@example.com".into(),
                purpose: "Marketing".into(),
                scope: "AllData".into(),
            },
            None,
            None,
        );

        log.append(
            ComplianceAuditAction::BreachDetected {
                event_id: Uuid::new_v4(),
                severity: "High".into(),
                indicator: "anomaly".into(),
                affected_subjects: vec![],
            },
            None,
            None,
        );

        // Export only consent events
        let query = AuditQuery::default().with_action_type("Consent");
        let json = log.export_json(&query).expect("export must succeed");

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).expect("JSON must parse");
        assert_eq!(parsed.len(), 1);
    }
}
