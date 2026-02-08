//! Breach detection and notification for HIPAA Section 164.404 and GDPR Article 33.
//!
//! This module provides automated breach detection with 6 indicators and enforces
//! the 72-hour notification deadline required by both HIPAA and GDPR.
//!
//! # Indicators
//!
//! - **Mass Data Export**: Records exported exceed configurable threshold
//! - **Unauthorized Access Pattern**: Denied access attempts exceed threshold in window
//! - **Privilege Escalation**: Any role escalation triggers a breach event
//! - **Anomalous Query Volume**: Query rate exceeds baseline multiplier
//! - **Unusual Access Time**: Access outside business hours (09:00-17:00)
//! - **Data Exfiltration Pattern**: Bytes exported exceed configurable threshold
//!
//! # Lifecycle
//!
//! ```text
//! Detected -> UnderInvestigation -> Confirmed { notification } -> Resolved
//!                                -> FalsePositive
//! ```

use chrono::{DateTime, Duration, Utc};
use kimberlite_types::DataClass;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Notification deadline: 72 hours per HIPAA Section 164.404 and GDPR Article 33.
const NOTIFICATION_DEADLINE_HOURS: i64 = 72;

/// Business hours start (inclusive).
const BUSINESS_HOURS_START: u8 = 9;

/// Business hours end (exclusive).
const BUSINESS_HOURS_END: u8 = 17;

#[derive(Debug, Error)]
pub enum BreachError {
    #[error("Breach event not found: {0}")]
    EventNotFound(Uuid),
    #[error("Invalid state transition: {0}")]
    InvalidTransition(String),
}

pub type Result<T> = std::result::Result<T, BreachError>;

/// Indicator that triggered a breach detection event.
///
/// Each variant captures the specific metrics that exceeded thresholds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreachIndicator {
    MassDataExport {
        records: u64,
        threshold: u64,
    },
    UnauthorizedAccessPattern {
        denied_attempts: u64,
        window_secs: u64,
    },
    PrivilegeEscalation {
        from_role: String,
        to_role: String,
    },
    AnomalousQueryVolume {
        queries_per_min: u64,
        baseline: u64,
    },
    UnusualAccessTime {
        hour: u8,
        is_business_hours: bool,
    },
    DataExfiltrationPattern {
        bytes_exported: u64,
        threshold: u64,
    },
}

/// Severity level for a breach event, ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BreachSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Status of a breach event through its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreachStatus {
    Detected,
    UnderInvestigation,
    Confirmed {
        notification_sent_at: Option<DateTime<Utc>>,
    },
    FalsePositive {
        dismissed_by: String,
        reason: String,
    },
    Resolved {
        resolved_at: DateTime<Utc>,
        remediation: String,
    },
}

/// A single breach detection event with full audit trail metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreachEvent {
    pub event_id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub indicator: BreachIndicator,
    pub severity: BreachSeverity,
    pub affected_subjects: Option<u64>,
    pub affected_data_classes: Vec<DataClass>,
    /// 72-hour deadline from detection (HIPAA Section 164.404 / GDPR Article 33).
    pub notification_deadline: DateTime<Utc>,
    pub status: BreachStatus,
}

/// Configurable thresholds for breach detection indicators.
///
/// All fields are private and immutable after construction. Use
/// [`BreachThresholdsBuilder`] to create custom thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreachThresholds {
    /// Number of records exported that triggers a mass export alert. Default: 1000.
    mass_export_records: u64,
    /// Number of denied access attempts within the window. Default: 10.
    denied_attempts_window: u64,
    /// Multiplier over baseline query volume to trigger alert. Default: 5.0x.
    query_volume_multiplier: f64,
    /// Bytes exported that triggers an exfiltration alert. Default: 100MB.
    export_bytes_threshold: u64,
    /// Business hours start (inclusive, 0-23 UTC). Default: 9.
    business_hours_start: u8,
    /// Business hours end (exclusive, 0-23 UTC). Default: 17.
    business_hours_end: u8,
}

impl BreachThresholds {
    /// Returns the mass export record threshold.
    pub fn mass_export_records(&self) -> u64 {
        self.mass_export_records
    }

    /// Returns the denied access attempts threshold.
    pub fn denied_attempts_window(&self) -> u64 {
        self.denied_attempts_window
    }

    /// Returns the query volume multiplier threshold.
    pub fn query_volume_multiplier(&self) -> f64 {
        self.query_volume_multiplier
    }

    /// Returns the export bytes threshold.
    pub fn export_bytes_threshold(&self) -> u64 {
        self.export_bytes_threshold
    }

    /// Returns the business hours start (inclusive, 0-23 UTC).
    pub fn business_hours_start(&self) -> u8 {
        self.business_hours_start
    }

    /// Returns the business hours end (exclusive, 0-23 UTC).
    pub fn business_hours_end(&self) -> u8 {
        self.business_hours_end
    }
}

impl Default for BreachThresholds {
    fn default() -> Self {
        Self {
            mass_export_records: 1000,
            denied_attempts_window: 10,
            query_volume_multiplier: 5.0,
            export_bytes_threshold: 104_857_600,
            business_hours_start: BUSINESS_HOURS_START,
            business_hours_end: BUSINESS_HOURS_END,
        }
    }
}

/// Builder for [`BreachThresholds`].
#[derive(Debug, Clone)]
pub struct BreachThresholdsBuilder {
    thresholds: BreachThresholds,
}

impl BreachThresholdsBuilder {
    /// Creates a new builder with default thresholds.
    pub fn new() -> Self {
        Self {
            thresholds: BreachThresholds::default(),
        }
    }

    /// Sets the mass export record threshold.
    pub fn mass_export_records(mut self, value: u64) -> Self {
        self.thresholds.mass_export_records = value;
        self
    }

    /// Sets the denied access attempts threshold.
    pub fn denied_attempts_window(mut self, value: u64) -> Self {
        self.thresholds.denied_attempts_window = value;
        self
    }

    /// Sets the query volume multiplier threshold.
    pub fn query_volume_multiplier(mut self, value: f64) -> Self {
        self.thresholds.query_volume_multiplier = value;
        self
    }

    /// Sets the export bytes threshold.
    pub fn export_bytes_threshold(mut self, value: u64) -> Self {
        self.thresholds.export_bytes_threshold = value;
        self
    }

    /// Sets the business hours start (inclusive, 0-23 UTC).
    pub fn business_hours_start(mut self, hour: u8) -> Self {
        assert!(hour < 24, "business_hours_start must be 0-23, got {hour}");
        self.thresholds.business_hours_start = hour;
        self
    }

    /// Sets the business hours end (exclusive, 0-23 UTC).
    pub fn business_hours_end(mut self, hour: u8) -> Self {
        assert!(hour < 24, "business_hours_end must be 0-23, got {hour}");
        self.thresholds.business_hours_end = hour;
        self
    }

    /// Builds the immutable `BreachThresholds`.
    pub fn build(self) -> BreachThresholds {
        self.thresholds
    }
}

impl Default for BreachThresholdsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Comprehensive breach report for regulatory notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreachReport {
    pub event: BreachEvent,
    pub timeline: Vec<String>,
    pub affected_subject_count: u64,
    pub data_categories: Vec<DataClass>,
    pub remediation_steps: Vec<String>,
    pub notification_status: String,
}

/// Automated breach detector implementing HIPAA Section 164.404 and GDPR Article 33.
///
/// Tracks 6 breach indicators with configurable thresholds and manages the
/// breach lifecycle from detection through resolution or dismissal.
#[derive(Debug)]
pub struct BreachDetector {
    events: Vec<BreachEvent>,
    thresholds: BreachThresholds,
    denied_access_count: u64,
    denied_access_window_start: Option<DateTime<Utc>>,
    query_count: u64,
    query_window_start: Option<DateTime<Utc>>,
    baseline_queries_per_min: u64,
}

impl Default for BreachDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl BreachDetector {
    /// Creates a new breach detector with default thresholds.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            thresholds: BreachThresholds::default(),
            denied_access_count: 0,
            denied_access_window_start: None,
            query_count: 0,
            query_window_start: None,
            baseline_queries_per_min: 100,
        }
    }

    /// Creates a new breach detector with custom thresholds.
    pub fn with_thresholds(thresholds: BreachThresholds) -> Self {
        Self {
            events: Vec::new(),
            thresholds,
            denied_access_count: 0,
            denied_access_window_start: None,
            query_count: 0,
            query_window_start: None,
            baseline_queries_per_min: 100,
        }
    }

    /// Checks whether a mass data export exceeds the configured threshold.
    ///
    /// Returns a breach event if `records_exported` exceeds `mass_export_records`.
    pub fn check_mass_export(
        &mut self,
        records_exported: u64,
        data_classes: &[DataClass],
    ) -> Option<BreachEvent> {
        if records_exported <= self.thresholds.mass_export_records {
            return None;
        }

        let indicator = BreachIndicator::MassDataExport {
            records: records_exported,
            threshold: self.thresholds.mass_export_records,
        };
        let severity = classify_severity(&indicator, data_classes);
        let event = self.create_event(indicator, severity, data_classes);
        Some(event)
    }

    /// Records a denied access attempt and checks if the threshold is breached.
    ///
    /// Resets the window counter if 60 seconds have elapsed since the window start.
    pub fn check_denied_access(&mut self, now: DateTime<Utc>) -> Option<BreachEvent> {
        let window_start = self.denied_access_window_start.get_or_insert(now);
        let elapsed = now.signed_duration_since(*window_start);

        // Reset window if 60 seconds have elapsed
        if elapsed > Duration::seconds(60) {
            self.denied_access_count = 0;
            self.denied_access_window_start = Some(now);
        }

        self.denied_access_count += 1;

        if self.denied_access_count < self.thresholds.denied_attempts_window {
            return None;
        }

        let indicator = BreachIndicator::UnauthorizedAccessPattern {
            denied_attempts: self.denied_access_count,
            window_secs: 60,
        };
        let severity = classify_severity(&indicator, &[]);
        let event = self.create_event(indicator, severity, &[]);

        // Reset after triggering
        self.denied_access_count = 0;
        self.denied_access_window_start = None;

        Some(event)
    }

    /// Detects privilege escalation attempts, which always trigger a breach event.
    pub fn check_privilege_escalation(
        &mut self,
        from_role: &str,
        to_role: &str,
    ) -> Option<BreachEvent> {
        assert!(!from_role.is_empty(), "from_role must not be empty");
        assert!(!to_role.is_empty(), "to_role must not be empty");

        let indicator = BreachIndicator::PrivilegeEscalation {
            from_role: from_role.to_string(),
            to_role: to_role.to_string(),
        };
        let severity = classify_severity(&indicator, &[]);
        let event = self.create_event(indicator, severity, &[]);
        Some(event)
    }

    /// Records a query and checks whether volume is anomalous.
    ///
    /// Triggers if queries per minute exceed `baseline * query_volume_multiplier`.
    pub fn check_query_volume(&mut self, now: DateTime<Utc>) -> Option<BreachEvent> {
        let window_start = self.query_window_start.get_or_insert(now);
        let elapsed = now.signed_duration_since(*window_start);

        // Reset window if 60 seconds have elapsed
        if elapsed > Duration::seconds(60) {
            self.query_count = 0;
            self.query_window_start = Some(now);
        }

        self.query_count += 1;

        #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
        let threshold =
            (self.baseline_queries_per_min as f64 * self.thresholds.query_volume_multiplier) as u64;

        if self.query_count < threshold {
            return None;
        }

        let indicator = BreachIndicator::AnomalousQueryVolume {
            queries_per_min: self.query_count,
            baseline: self.baseline_queries_per_min,
        };
        let severity = classify_severity(&indicator, &[]);
        let event = self.create_event(indicator, severity, &[]);

        // Reset after triggering
        self.query_count = 0;
        self.query_window_start = None;

        Some(event)
    }

    /// Checks whether access is occurring outside business hours (09:00-17:00).
    pub fn check_unusual_access_time(&mut self, hour: u8) -> Option<BreachEvent> {
        assert!(hour < 24, "hour must be 0-23, got {hour}");

        let is_business_hours =
            (self.thresholds.business_hours_start()..self.thresholds.business_hours_end())
                .contains(&hour);

        if is_business_hours {
            return None;
        }

        let indicator = BreachIndicator::UnusualAccessTime {
            hour,
            is_business_hours,
        };
        let severity = classify_severity(&indicator, &[]);
        let event = self.create_event(indicator, severity, &[]);
        Some(event)
    }

    /// Checks whether bytes exported exceed the exfiltration threshold.
    pub fn check_data_exfiltration(
        &mut self,
        bytes_exported: u64,
        data_classes: &[DataClass],
    ) -> Option<BreachEvent> {
        if bytes_exported <= self.thresholds.export_bytes_threshold {
            return None;
        }

        let indicator = BreachIndicator::DataExfiltrationPattern {
            bytes_exported,
            threshold: self.thresholds.export_bytes_threshold,
        };
        let severity = classify_severity(&indicator, data_classes);
        let event = self.create_event(indicator, severity, data_classes);
        Some(event)
    }

    /// Moves a breach event from `Detected` to `UnderInvestigation`.
    pub fn escalate(&mut self, event_id: Uuid) -> Result<()> {
        let event = self.find_event_mut(event_id)?;

        if event.status != BreachStatus::Detected {
            let status = &event.status;
            return Err(BreachError::InvalidTransition(format!(
                "cannot escalate from {status:?}, expected Detected"
            )));
        }

        event.status = BreachStatus::UnderInvestigation;
        Ok(())
    }

    /// Moves a breach event to `Confirmed` with an optional notification timestamp.
    pub fn confirm(&mut self, event_id: Uuid) -> Result<()> {
        let event = self.find_event_mut(event_id)?;

        match &event.status {
            BreachStatus::Detected | BreachStatus::UnderInvestigation => {}
            other => {
                return Err(BreachError::InvalidTransition(format!(
                    "cannot confirm from {other:?}, expected Detected or UnderInvestigation"
                )));
            }
        }

        event.status = BreachStatus::Confirmed {
            notification_sent_at: Some(Utc::now()),
        };
        Ok(())
    }

    /// Dismisses a breach event as a false positive.
    pub fn dismiss(&mut self, event_id: Uuid, dismissed_by: &str, reason: &str) -> Result<()> {
        assert!(!dismissed_by.is_empty(), "dismissed_by must not be empty");
        assert!(!reason.is_empty(), "reason must not be empty");

        let event = self.find_event_mut(event_id)?;

        match &event.status {
            BreachStatus::Detected | BreachStatus::UnderInvestigation => {}
            other => {
                return Err(BreachError::InvalidTransition(format!(
                    "cannot dismiss from {other:?}, expected Detected or UnderInvestigation"
                )));
            }
        }

        event.status = BreachStatus::FalsePositive {
            dismissed_by: dismissed_by.to_string(),
            reason: reason.to_string(),
        };
        Ok(())
    }

    /// Marks a confirmed breach event as resolved with remediation notes.
    pub fn resolve(&mut self, event_id: Uuid, remediation: &str) -> Result<()> {
        assert!(!remediation.is_empty(), "remediation must not be empty");

        let event = self.find_event_mut(event_id)?;

        match &event.status {
            BreachStatus::Confirmed { .. } => {}
            other => {
                return Err(BreachError::InvalidTransition(format!(
                    "cannot resolve from {other:?}, expected Confirmed"
                )));
            }
        }

        event.status = BreachStatus::Resolved {
            resolved_at: Utc::now(),
            remediation: remediation.to_string(),
        };
        Ok(())
    }

    /// Returns breach events that are past the 72-hour notification deadline
    /// without a notification being sent.
    pub fn check_notification_deadlines(&self, now: DateTime<Utc>) -> Vec<&BreachEvent> {
        self.events
            .iter()
            .filter(|e| now > e.notification_deadline && !Self::has_notification(e))
            .collect()
    }

    /// Generates a comprehensive breach report for the specified event.
    pub fn generate_report(&self, event_id: Uuid) -> Result<BreachReport> {
        let event = self
            .get_event(event_id)
            .ok_or(BreachError::EventNotFound(event_id))?;

        let notification_status = Self::format_notification_status(event);
        let timeline = Self::build_timeline(event);
        let remediation_steps = Self::build_remediation_steps(&event.indicator);

        Ok(BreachReport {
            event: event.clone(),
            timeline,
            affected_subject_count: event.affected_subjects.unwrap_or(0),
            data_categories: event.affected_data_classes.clone(),
            remediation_steps,
            notification_status,
        })
    }

    /// Looks up a breach event by its ID.
    pub fn get_event(&self, event_id: Uuid) -> Option<&BreachEvent> {
        self.events.iter().find(|e| e.event_id == event_id)
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    fn create_event(
        &mut self,
        indicator: BreachIndicator,
        severity: BreachSeverity,
        data_classes: &[DataClass],
    ) -> BreachEvent {
        let now = Utc::now();
        let deadline = now + Duration::hours(NOTIFICATION_DEADLINE_HOURS);

        let event = BreachEvent {
            event_id: Uuid::new_v4(),
            detected_at: now,
            indicator,
            severity,
            affected_subjects: None,
            affected_data_classes: data_classes.to_vec(),
            notification_deadline: deadline,
            status: BreachStatus::Detected,
        };

        // Postcondition: deadline is exactly 72 hours after detection
        debug_assert_eq!(
            event.notification_deadline,
            event.detected_at + Duration::hours(NOTIFICATION_DEADLINE_HOURS),
            "notification deadline must be 72h after detection"
        );

        self.events.push(event.clone());
        event
    }

    fn find_event_mut(&mut self, event_id: Uuid) -> Result<&mut BreachEvent> {
        self.events
            .iter_mut()
            .find(|e| e.event_id == event_id)
            .ok_or(BreachError::EventNotFound(event_id))
    }

    fn has_notification(event: &BreachEvent) -> bool {
        matches!(
            &event.status,
            BreachStatus::Confirmed {
                notification_sent_at: Some(_)
            } | BreachStatus::Resolved { .. }
                | BreachStatus::FalsePositive { .. }
        )
    }

    fn format_notification_status(event: &BreachEvent) -> String {
        match &event.status {
            BreachStatus::Detected => "Pending - not yet investigated".to_string(),
            BreachStatus::UnderInvestigation => "Pending - under investigation".to_string(),
            BreachStatus::Confirmed {
                notification_sent_at: Some(sent_at),
            } => format!("Notification sent at {sent_at}"),
            BreachStatus::Confirmed {
                notification_sent_at: None,
            } => "Confirmed - notification pending".to_string(),
            BreachStatus::FalsePositive { .. } => "Dismissed as false positive".to_string(),
            BreachStatus::Resolved { .. } => "Resolved - notification completed".to_string(),
        }
    }

    fn build_timeline(event: &BreachEvent) -> Vec<String> {
        let mut timeline = vec![format!("Detected at {}", event.detected_at)];

        match &event.status {
            BreachStatus::Detected => {}
            BreachStatus::UnderInvestigation => {
                timeline.push("Escalated to investigation".to_string());
            }
            BreachStatus::Confirmed {
                notification_sent_at,
            } => {
                timeline.push("Escalated to investigation".to_string());
                timeline.push("Breach confirmed".to_string());
                if let Some(sent_at) = notification_sent_at {
                    timeline.push(format!("Notification sent at {sent_at}"));
                }
            }
            BreachStatus::FalsePositive {
                dismissed_by,
                reason,
            } => {
                timeline.push(format!("Dismissed by {dismissed_by}: {reason}"));
            }
            BreachStatus::Resolved {
                resolved_at,
                remediation,
            } => {
                timeline.push("Breach confirmed".to_string());
                timeline.push(format!("Resolved at {resolved_at}: {remediation}"));
            }
        }

        timeline
    }

    fn build_remediation_steps(indicator: &BreachIndicator) -> Vec<String> {
        match indicator {
            BreachIndicator::MassDataExport { .. } => vec![
                "Revoke export permissions for affected accounts".to_string(),
                "Audit all recent export operations".to_string(),
                "Review data loss prevention policies".to_string(),
            ],
            BreachIndicator::UnauthorizedAccessPattern { .. } => vec![
                "Lock affected accounts pending investigation".to_string(),
                "Review access control policies".to_string(),
                "Enable additional authentication factors".to_string(),
            ],
            BreachIndicator::PrivilegeEscalation { .. } => vec![
                "Revoke escalated privileges immediately".to_string(),
                "Audit all actions taken with elevated privileges".to_string(),
                "Review role assignment procedures".to_string(),
            ],
            BreachIndicator::AnomalousQueryVolume { .. } => vec![
                "Throttle affected accounts".to_string(),
                "Review query patterns for data harvesting".to_string(),
                "Implement rate limiting controls".to_string(),
            ],
            BreachIndicator::UnusualAccessTime { .. } => vec![
                "Verify identity of accessing user".to_string(),
                "Review access logs for the session".to_string(),
                "Consider implementing time-based access controls".to_string(),
            ],
            BreachIndicator::DataExfiltrationPattern { .. } => vec![
                "Block outbound data transfers immediately".to_string(),
                "Identify destination of exported data".to_string(),
                "Engage incident response team".to_string(),
            ],
        }
    }
}

/// Classifies breach severity based on the indicator type and affected data classes.
///
/// - **Critical**: PHI or PCI data involved
/// - **High**: PII or Sensitive data involved
/// - **Medium**: Confidential data involved
/// - **Low**: No regulated data involved
pub fn classify_severity(
    indicator: &BreachIndicator,
    data_classes: &[DataClass],
) -> BreachSeverity {
    // Privilege escalation is always at least High severity
    let base_severity = match indicator {
        BreachIndicator::PrivilegeEscalation { .. } => BreachSeverity::High,
        _ => BreachSeverity::Low,
    };

    let data_severity = data_class_severity(data_classes);

    // Return the higher of the two
    if data_severity > base_severity {
        data_severity
    } else {
        base_severity
    }
}

/// Determines severity from the most sensitive data class present.
fn data_class_severity(data_classes: &[DataClass]) -> BreachSeverity {
    let mut severity = BreachSeverity::Low;

    for dc in data_classes {
        let class_severity = match dc {
            DataClass::PHI | DataClass::PCI => BreachSeverity::Critical,
            DataClass::PII | DataClass::Sensitive => BreachSeverity::High,
            DataClass::Confidential | DataClass::Financial => BreachSeverity::Medium,
            DataClass::Deidentified | DataClass::Public => BreachSeverity::Low,
        };

        if class_severity > severity {
            severity = class_severity;
        }
    }

    severity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mass_export_detection() {
        let mut detector = BreachDetector::new();

        // Below threshold: no event
        let result = detector.check_mass_export(500, &[DataClass::PII]);
        assert!(result.is_none());

        // At threshold: no event (must exceed, not equal)
        let result = detector.check_mass_export(1000, &[DataClass::PII]);
        assert!(result.is_none());

        // Above threshold: triggers event
        let result = detector.check_mass_export(1500, &[DataClass::PHI]);
        assert!(result.is_some());

        let event = result.expect("should have breach event");
        assert_eq!(event.severity, BreachSeverity::Critical); // PHI = Critical
        assert!(matches!(
            event.indicator,
            BreachIndicator::MassDataExport {
                records: 1500,
                threshold: 1000
            }
        ));
        assert_eq!(event.status, BreachStatus::Detected);
    }

    #[test]
    fn test_denied_access_threshold() {
        let mut detector = BreachDetector::new();
        let now = Utc::now();

        // 9 attempts: below threshold (default 10)
        for i in 0..9 {
            let result = detector.check_denied_access(now + Duration::seconds(i));
            assert!(result.is_none(), "attempt {i} should not trigger");
        }

        // 10th attempt: triggers
        let result = detector.check_denied_access(now + Duration::seconds(9));
        assert!(result.is_some(), "10th attempt should trigger");

        let event = result.expect("should have breach event");
        assert!(matches!(
            event.indicator,
            BreachIndicator::UnauthorizedAccessPattern { .. }
        ));
    }

    #[test]
    fn test_privilege_escalation_always_triggers() {
        let mut detector = BreachDetector::new();

        let result = detector.check_privilege_escalation("reader", "admin");
        assert!(result.is_some(), "privilege escalation must always trigger");

        let event = result.expect("should have breach event");
        assert!(event.severity >= BreachSeverity::High);
        assert!(matches!(
            event.indicator,
            BreachIndicator::PrivilegeEscalation { .. }
        ));
    }

    #[test]
    fn test_unusual_access_time() {
        let mut detector = BreachDetector::new();

        // Business hours: no event
        for hour in BUSINESS_HOURS_START..BUSINESS_HOURS_END {
            let result = detector.check_unusual_access_time(hour);
            assert!(result.is_none(), "hour {hour} is business hours");
        }

        // Outside business hours: triggers
        let result = detector.check_unusual_access_time(3);
        assert!(result.is_some(), "hour 3 is outside business hours");

        let result = detector.check_unusual_access_time(22);
        assert!(result.is_some(), "hour 22 is outside business hours");
    }

    #[test]
    fn test_breach_lifecycle() {
        let mut detector = BreachDetector::new();

        // Detect a breach
        let event = detector
            .check_privilege_escalation("user", "admin")
            .expect("should trigger");
        let event_id = event.event_id;

        // Detected -> UnderInvestigation
        detector
            .escalate(event_id)
            .expect("escalate should succeed");
        let event = detector.get_event(event_id).expect("event should exist");
        assert_eq!(event.status, BreachStatus::UnderInvestigation);

        // UnderInvestigation -> Confirmed
        detector.confirm(event_id).expect("confirm should succeed");
        let event = detector.get_event(event_id).expect("event should exist");
        assert!(matches!(
            event.status,
            BreachStatus::Confirmed {
                notification_sent_at: Some(_)
            }
        ));

        // Confirmed -> Resolved
        detector
            .resolve(event_id, "Revoked admin privileges and rotated credentials")
            .expect("resolve should succeed");
        let event = detector.get_event(event_id).expect("event should exist");
        assert!(matches!(event.status, BreachStatus::Resolved { .. }));
    }

    #[test]
    fn test_false_positive_dismissal() {
        let mut detector = BreachDetector::new();

        let event = detector
            .check_unusual_access_time(2)
            .expect("should trigger");
        let event_id = event.event_id;

        // Detected -> FalsePositive
        detector
            .dismiss(event_id, "security-team", "Scheduled maintenance window")
            .expect("dismiss should succeed");

        let event = detector.get_event(event_id).expect("event should exist");
        assert!(matches!(
            event.status,
            BreachStatus::FalsePositive {
                ref dismissed_by,
                ref reason,
            } if dismissed_by == "security-team" && reason == "Scheduled maintenance window"
        ));

        // Cannot resolve a false positive
        let result = detector.resolve(event_id, "n/a");
        assert!(result.is_err());
    }

    #[test]
    fn test_72h_deadline() {
        let mut detector = BreachDetector::new();

        let event = detector
            .check_privilege_escalation("viewer", "admin")
            .expect("should trigger");

        // Deadline is 72 hours after detection
        let deadline_diff = event
            .notification_deadline
            .signed_duration_since(event.detected_at);
        assert_eq!(
            deadline_diff.num_hours(),
            NOTIFICATION_DEADLINE_HOURS,
            "deadline must be exactly 72h after detection"
        );

        // Before deadline: no overdue events
        let now = event.detected_at + Duration::hours(71);
        let overdue = detector.check_notification_deadlines(now);
        assert!(overdue.is_empty(), "should not be overdue before 72h");

        // After deadline without notification: overdue
        let now = event.detected_at + Duration::hours(73);
        let overdue = detector.check_notification_deadlines(now);
        assert_eq!(overdue.len(), 1, "should be overdue after 72h");
        assert_eq!(overdue[0].event_id, event.event_id);
    }

    #[test]
    fn test_severity_classification() {
        // PHI -> Critical
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::PHI]
            ),
            BreachSeverity::Critical
        );

        // PCI -> Critical
        assert_eq!(
            classify_severity(
                &BreachIndicator::DataExfiltrationPattern {
                    bytes_exported: 200_000_000,
                    threshold: 100_000_000
                },
                &[DataClass::PCI]
            ),
            BreachSeverity::Critical
        );

        // PII -> High
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::PII]
            ),
            BreachSeverity::High
        );

        // Sensitive -> High
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::Sensitive]
            ),
            BreachSeverity::High
        );

        // Confidential -> Medium
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::Confidential]
            ),
            BreachSeverity::Medium
        );

        // Public -> Low (no regulated data)
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::Public]
            ),
            BreachSeverity::Low
        );

        // Privilege escalation with no data classes -> High (base severity)
        assert_eq!(
            classify_severity(
                &BreachIndicator::PrivilegeEscalation {
                    from_role: "user".to_string(),
                    to_role: "admin".to_string()
                },
                &[]
            ),
            BreachSeverity::High
        );

        // Mixed: highest wins (PHI + Public -> Critical)
        assert_eq!(
            classify_severity(
                &BreachIndicator::MassDataExport {
                    records: 5000,
                    threshold: 1000
                },
                &[DataClass::Public, DataClass::PHI]
            ),
            BreachSeverity::Critical
        );
    }

    #[test]
    fn test_breach_report_generation() {
        let mut detector = BreachDetector::new();

        let event = detector
            .check_mass_export(5000, &[DataClass::PHI, DataClass::PII])
            .expect("should trigger");
        let event_id = event.event_id;

        let report = detector
            .generate_report(event_id)
            .expect("report should succeed");

        assert_eq!(report.event.event_id, event_id);
        assert!(!report.timeline.is_empty());
        assert!(!report.remediation_steps.is_empty());
        assert_eq!(report.data_categories.len(), 2);
        assert!(report.notification_status.contains("Pending"));

        // Report for nonexistent event should fail
        let bad_id = Uuid::new_v4();
        let result = detector.generate_report(bad_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_transitions() {
        let mut detector = BreachDetector::new();

        let event = detector
            .check_privilege_escalation("user", "admin")
            .expect("should trigger");
        let event_id = event.event_id;

        // Cannot resolve from Detected (must be Confirmed first)
        let result = detector.resolve(event_id, "fix");
        assert!(result.is_err());

        // Escalate to UnderInvestigation
        detector.escalate(event_id).expect("should succeed");

        // Cannot escalate again
        let result = detector.escalate(event_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_data_exfiltration_detection() {
        let mut detector = BreachDetector::new();

        // Below threshold: no event
        let result = detector.check_data_exfiltration(50_000_000, &[DataClass::PII]);
        assert!(result.is_none());

        // Above threshold: triggers
        let result = detector.check_data_exfiltration(200_000_000, &[DataClass::PCI]);
        assert!(result.is_some());

        let event = result.expect("should have breach event");
        assert_eq!(event.severity, BreachSeverity::Critical); // PCI = Critical
    }

    #[test]
    fn test_custom_thresholds() {
        let thresholds = BreachThresholdsBuilder::new()
            .mass_export_records(50)
            .denied_attempts_window(3)
            .query_volume_multiplier(2.0)
            .export_bytes_threshold(1_000_000)
            .build();
        let mut detector = BreachDetector::with_thresholds(thresholds);

        // Lower threshold: triggers sooner
        let result = detector.check_mass_export(100, &[DataClass::Public]);
        assert!(result.is_some());
    }

    #[test]
    fn test_denied_access_window_reset() {
        let mut detector = BreachDetector::new();
        let start = Utc::now();

        // 5 attempts in first window
        for i in 0..5 {
            detector.check_denied_access(start + Duration::seconds(i));
        }

        // Jump past 60-second window: counter resets
        let new_window = start + Duration::seconds(120);
        for i in 0..9 {
            let result = detector.check_denied_access(new_window + Duration::seconds(i));
            assert!(
                result.is_none(),
                "attempt {i} in new window should not trigger"
            );
        }

        // 10th attempt in new window: triggers
        let result = detector.check_denied_access(new_window + Duration::seconds(9));
        assert!(result.is_some());
    }
}
