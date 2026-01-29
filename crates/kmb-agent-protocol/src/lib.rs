//! Agent protocol types for `Kimberlite` cluster management.
//!
//! This crate defines the protocol used for communication between `Kimberlite`
//! cluster agents and control plane systems. It is designed to be used by:
//!
//! - **Self-hosters**: Building custom agents or control planes
//! - **Platform operators**: The official `Kimberlite` platform
//! - **Tooling authors**: Monitoring, automation, and integration tools
//!
//! # Protocol Overview
//!
//! The protocol uses a request-response pattern over WebSocket connections.
//! Agents connect to the control plane and exchange typed messages.
//!
//! ## Agent → Control Plane
//!
//! - [`AgentMessage::Heartbeat`] - Periodic health and status updates
//! - [`AgentMessage::MetricsBatch`] - Collected metrics samples
//! - [`AgentMessage::LogsBatch`] - Log entries from the node
//! - [`AgentMessage::ConfigAck`] - Acknowledgment of configuration changes
//! - [`AgentMessage::ControlAck`] - Acknowledgment of control messages
//! - [`AgentMessage::AuthResponse`] - Response to authentication challenge
//!
//! ## Control Plane → Agent
//!
//! - [`ControlMessage::ConfigUpdate`] - Push new configuration
//! - [`ControlMessage::AdminCommand`] - Administrative operations
//! - [`ControlMessage::HeartbeatRequest`] - Request immediate status
//! - [`ControlMessage::Shutdown`] - Graceful shutdown request
//! - [`ControlMessage::AuthChallenge`] - Authentication challenge
//! - [`ControlMessage::FlowControl`] - Backpressure signal
//! - [`ControlMessage::HealthCheck`] - Health check request
//!
//! # Connection Lifecycle
//!
//! Agents implement exponential backoff for reconnection using [`BackoffConfig`].
//! The protocol supports authentication via [`AgentCredentials`] and flow control
//! via [`FlowControlSignal`] for high-volume scenarios.
//!
//! # Example
//!
//! ```rust
//! use kmb_agent_protocol::{AgentMessage, NodeStatus, NodeRole, Resources};
//!
//! let heartbeat = AgentMessage::Heartbeat {
//!     node_id: "node-001".to_string(),
//!     status: NodeStatus::Healthy,
//!     role: NodeRole::Leader,
//!     resources: Resources {
//!         cpu_percent: 45.2,
//!         memory_used_bytes: 1_073_741_824,
//!         memory_total_bytes: 8_589_934_592,
//!         disk_used_bytes: 10_737_418_240,
//!         disk_total_bytes: 107_374_182_400,
//!     },
//!     replication: None,
//!     buffer_stats: vec![],
//! };
//! ```

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ============================================================================
// Core Identifiers
// ============================================================================

/// Unique identifier for a `Kimberlite` cluster.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClusterId(pub String);

/// Unique identifier for a node within a cluster.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// Configuration version identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ConfigVersion(pub u64);

/// Unique message identifier for correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub u64);

// ============================================================================
// Reconnection & Backoff
// ============================================================================

/// Configuration for exponential backoff reconnection strategy.
///
/// When a connection drops, agents should wait with exponential backoff
/// before attempting to reconnect. This prevents thundering herd problems
/// and reduces load on the control plane during outages.
///
/// # Example
///
/// ```rust
/// use kmb_agent_protocol::BackoffConfig;
///
/// let config = BackoffConfig::default();
/// assert_eq!(config.initial_delay_ms, 1000);
/// assert_eq!(config.max_delay_ms, 60_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Initial delay before first reconnection attempt (milliseconds).
    pub initial_delay_ms: u64,
    /// Maximum delay between reconnection attempts (milliseconds).
    pub max_delay_ms: u64,
    /// Multiplier applied to delay after each failed attempt.
    pub multiplier: f64,
    /// Maximum random jitter to add (as fraction of delay, 0.0-1.0).
    /// Jitter prevents synchronized reconnection storms.
    pub jitter_factor: f64,
    /// Maximum number of reconnection attempts (0 = unlimited).
    pub max_attempts: u32,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1_000,   // 1 second
            max_delay_ms: 60_000,      // 60 seconds
            multiplier: 2.0,
            jitter_factor: 0.25,
            max_attempts: 0,           // unlimited
        }
    }
}

impl BackoffConfig {
    /// Computes the delay for a given attempt number (0-indexed).
    ///
    /// Returns `None` if `max_attempts` is reached.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    pub fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        if self.max_attempts > 0 && attempt >= self.max_attempts {
            return None;
        }

        // Precision loss is acceptable for backoff delays (sub-millisecond precision not needed)
        let base_delay = self.initial_delay_ms as f64 * self.multiplier.powi(attempt as i32);
        let capped_delay = base_delay.min(self.max_delay_ms as f64);

        Some(Duration::from_millis(capped_delay as u64))
    }

    /// Computes the delay with jitter for a given attempt.
    ///
    /// The jitter is deterministic based on the provided seed, making it
    /// reproducible for testing while still distributing reconnection times.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    pub fn delay_with_jitter(&self, attempt: u32, jitter_seed: u64) -> Option<Duration> {
        self.delay_for_attempt(attempt).map(|base| {
            // Simple deterministic jitter based on seed
            // Precision loss is acceptable (sub-ms precision not needed for backoff)
            let jitter_range = (base.as_millis() as f64 * self.jitter_factor) as u64;
            if jitter_range == 0 {
                return base;
            }
            let jitter = (jitter_seed % jitter_range) as i64 - (jitter_range / 2) as i64;
            // max(1) ensures result is always positive
            let adjusted = (base.as_millis() as i64 + jitter).max(1) as u64;
            Duration::from_millis(adjusted)
        })
    }
}

/// State of the reconnection backoff strategy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackoffState {
    /// Number of consecutive failed connection attempts.
    pub consecutive_failures: u32,
    /// Timestamp of last connection attempt (milliseconds since epoch).
    pub last_attempt_ms: Option<u64>,
    /// Timestamp of last successful connection (milliseconds since epoch).
    pub last_success_ms: Option<u64>,
}

impl BackoffState {
    /// Records a failed connection attempt.
    pub fn record_failure(&mut self, now_ms: u64) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_attempt_ms = Some(now_ms);
    }

    /// Records a successful connection.
    pub fn record_success(&mut self, now_ms: u64) {
        self.consecutive_failures = 0;
        self.last_attempt_ms = Some(now_ms);
        self.last_success_ms = Some(now_ms);
    }

    /// Resets the backoff state.
    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.last_attempt_ms = None;
    }
}

/// Connection lifecycle state for agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Not connected, not attempting to connect.
    Disconnected,
    /// Waiting for backoff delay before reconnecting.
    Backoff,
    /// Actively attempting to connect.
    Connecting,
    /// Connected but not yet authenticated.
    Connected,
    /// Connected and authenticated, ready for normal operation.
    Authenticated,
    /// Connection is being gracefully closed.
    Closing,
}

// ============================================================================
// Backpressure & Flow Control
// ============================================================================

/// Flow control signal sent by the control plane to manage agent data rate.
///
/// When the control plane is overwhelmed with metrics or logs, it sends
/// flow control signals to slow down or pause data transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowControlSignal {
    /// Resume normal transmission rate.
    Resume,
    /// Reduce transmission rate (send less frequently).
    SlowDown {
        /// Suggested minimum interval between batches (milliseconds).
        min_interval_ms: u64,
    },
    /// Pause transmission of the specified data type.
    Pause,
}

/// Type of data stream for flow control targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataStreamType {
    /// Heartbeat messages.
    Heartbeats,
    /// Metrics batches.
    Metrics,
    /// Log batches.
    Logs,
    /// All data streams.
    All,
}

/// Agent-side buffer state for monitoring backpressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferState {
    /// Buffer is empty or nearly empty.
    Empty,
    /// Buffer has normal usage.
    Normal,
    /// Buffer is filling up, may need to slow down.
    High,
    /// Buffer is critically full, data may be dropped.
    Critical,
}

/// Buffer statistics reported by agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferStats {
    /// Type of data in this buffer.
    pub stream_type: DataStreamType,
    /// Current buffer state.
    pub state: BufferState,
    /// Number of items currently buffered.
    pub pending_items: u64,
    /// Maximum buffer capacity.
    pub capacity: u64,
    /// Number of items dropped due to buffer overflow (since last report).
    pub dropped_count: u64,
    /// Oldest item age in milliseconds (0 if empty).
    pub oldest_item_age_ms: u64,
}

// ============================================================================
// Authentication
// ============================================================================

/// Agent credentials for authentication.
///
/// Agents authenticate during connection establishment. Multiple credential
/// types are supported for different deployment scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentCredentials {
    /// Bearer token authentication (JWT or API key).
    Bearer {
        /// The token value.
        token: String,
    },
    /// Pre-shared key authentication.
    PreSharedKey {
        /// Key identifier.
        key_id: String,
        /// HMAC signature of the challenge.
        signature: String,
    },
    /// mTLS certificate authentication (certificate validated at transport layer).
    /// The fingerprint is sent for correlation with expected identity.
    Certificate {
        /// SHA-256 fingerprint of the client certificate.
        fingerprint: String,
    },
}

/// Authentication challenge sent by control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthChallenge {
    /// Random challenge value for PSK authentication.
    pub challenge: String,
    /// Supported authentication methods.
    pub supported_methods: Vec<AuthMethod>,
    /// Challenge expiration (milliseconds from now).
    pub expires_in_ms: u64,
}

/// Supported authentication methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// Bearer token (JWT or API key).
    Bearer,
    /// Pre-shared key with challenge-response.
    PreSharedKey,
    /// mTLS certificate.
    Certificate,
}

/// Authentication response from agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    /// The credentials used for authentication.
    pub credentials: AgentCredentials,
    /// Agent metadata for registration.
    pub agent_info: AgentInfo,
}

/// Agent metadata sent during authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent software version.
    pub version: String,
    /// Kimberlite protocol version supported.
    pub protocol_version: String,
    /// Optional agent capabilities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Authenticated agent identity (after successful auth).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// Authenticated cluster ID.
    pub cluster_id: ClusterId,
    /// Authenticated node ID.
    pub node_id: NodeId,
    /// Authentication method used.
    pub auth_method: AuthMethod,
    /// When authentication occurred (milliseconds since epoch).
    pub authenticated_at_ms: u64,
    /// When authentication expires (milliseconds since epoch, if applicable).
    pub expires_at_ms: Option<u64>,
}

// ============================================================================
// Health Monitoring
// ============================================================================

/// Health check request sent by control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckRequest {
    /// Correlation ID for matching response.
    pub request_id: MessageId,
    /// Which health checks to perform.
    pub checks: Vec<HealthCheckType>,
}

/// Types of health checks that can be requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    /// Basic liveness check.
    Liveness,
    /// Storage subsystem health.
    Storage,
    /// Replication health (if applicable).
    Replication,
    /// Resource availability (disk, memory).
    Resources,
    /// All available health checks.
    All,
}

/// Health check response from agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    /// Correlation ID from request.
    pub request_id: MessageId,
    /// Overall health status.
    pub status: HealthStatus,
    /// Individual check results.
    pub checks: Vec<HealthCheckResult>,
    /// Time taken to complete checks (milliseconds).
    pub duration_ms: u64,
}

/// Overall health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// All checks passed.
    Healthy,
    /// Some checks show warnings but system is operational.
    Degraded,
    /// Critical checks failed, system may not be operational.
    Unhealthy,
}

/// Result of an individual health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Type of check performed.
    pub check_type: HealthCheckType,
    /// Whether the check passed.
    pub passed: bool,
    /// Human-readable message.
    pub message: String,
    /// Optional details (e.g., specific metrics).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<(String, String)>,
}

/// Health monitoring thresholds configuration.
///
/// Control planes use these thresholds to determine when a node
/// transitions between health states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthThresholds {
    /// Maximum time without heartbeat before marking unhealthy (ms).
    pub heartbeat_timeout_ms: u64,
    /// Maximum replication lag before marking degraded (ms).
    pub replication_lag_warn_ms: u64,
    /// Maximum replication lag before marking unhealthy (ms).
    pub replication_lag_critical_ms: u64,
    /// Disk usage percentage for warning.
    pub disk_usage_warn_percent: f64,
    /// Disk usage percentage for critical.
    pub disk_usage_critical_percent: f64,
    /// Memory usage percentage for warning.
    pub memory_usage_warn_percent: f64,
    /// Memory usage percentage for critical.
    pub memory_usage_critical_percent: f64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            heartbeat_timeout_ms: 30_000,      // 30 seconds
            replication_lag_warn_ms: 5_000,    // 5 seconds
            replication_lag_critical_ms: 30_000, // 30 seconds
            disk_usage_warn_percent: 80.0,
            disk_usage_critical_percent: 95.0,
            memory_usage_warn_percent: 85.0,
            memory_usage_critical_percent: 95.0,
        }
    }
}

// ============================================================================
// Control Message Acknowledgment
// ============================================================================

/// Acknowledgment of a control message from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlAck {
    /// ID of the message being acknowledged.
    pub message_id: MessageId,
    /// Whether the command was successful.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// Command-specific result data (JSON-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Time taken to execute the command (milliseconds).
    pub duration_ms: u64,
}

/// Timeout configuration for control messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ControlMessageTimeout {
    /// Default timeout for control messages (milliseconds).
    pub default_ms: u64,
    /// Timeout for snapshot commands (milliseconds).
    pub snapshot_ms: u64,
    /// Timeout for compaction commands (milliseconds).
    pub compaction_ms: u64,
    /// Timeout for leadership transfer (milliseconds).
    pub leadership_transfer_ms: u64,
}

impl Default for ControlMessageTimeout {
    fn default() -> Self {
        Self {
            default_ms: 30_000,           // 30 seconds
            snapshot_ms: 300_000,         // 5 minutes
            compaction_ms: 600_000,       // 10 minutes
            leadership_transfer_ms: 60_000, // 1 minute
        }
    }
}

// ============================================================================
// Agent → Control Plane Messages
// ============================================================================

/// Messages sent from an agent to the control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    /// Periodic heartbeat with node status and resource usage.
    Heartbeat {
        /// The node sending this heartbeat.
        node_id: String,
        /// Current health status of the node.
        status: NodeStatus,
        /// Current role in the cluster.
        role: NodeRole,
        /// Resource utilization snapshot.
        resources: Resources,
        /// Replication status (if applicable).
        replication: Option<ReplicationStatus>,
        /// Buffer statistics for backpressure monitoring.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        buffer_stats: Vec<BufferStats>,
    },

    /// Batch of collected metrics samples.
    MetricsBatch {
        /// The node these metrics are from.
        node_id: String,
        /// Collected metric samples.
        metrics: Vec<MetricSample>,
    },

    /// Batch of log entries.
    LogsBatch {
        /// The node these logs are from.
        node_id: String,
        /// Log entries.
        entries: Vec<LogEntry>,
    },

    /// Acknowledgment of a configuration update.
    ConfigAck {
        /// The configuration version being acknowledged.
        version: ConfigVersion,
        /// Whether the configuration was applied successfully.
        success: bool,
        /// Error message if the configuration failed to apply.
        error: Option<String>,
    },

    /// Acknowledgment of a control message (`AdminCommand`, etc.).
    ControlAck(ControlAck),

    /// Response to authentication challenge.
    AuthResponse(AuthResponse),

    /// Response to health check request.
    HealthCheckResponse(HealthCheckResponse),
}

/// Health status of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    /// Node is operating normally.
    Healthy,
    /// Node is experiencing issues but still operational.
    Degraded,
    /// Node is not operational.
    Unhealthy,
    /// Node is starting up.
    Starting,
    /// Node is shutting down.
    Stopping,
}

/// Role of a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// Primary node handling writes.
    Leader,
    /// Secondary node replicating from the leader.
    Follower,
    /// Node participating in leader election.
    Candidate,
    /// Node is not yet part of the cluster.
    Learner,
}

/// Resource utilization snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    /// CPU utilization as a percentage (0.0 - 100.0).
    pub cpu_percent: f64,
    /// Memory currently used in bytes.
    pub memory_used_bytes: u64,
    /// Total memory available in bytes.
    pub memory_total_bytes: u64,
    /// Disk space used in bytes.
    pub disk_used_bytes: u64,
    /// Total disk space in bytes.
    pub disk_total_bytes: u64,
}

/// Replication status for a follower node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatus {
    /// ID of the leader being replicated from.
    pub leader_id: String,
    /// Replication lag in milliseconds.
    pub lag_ms: u64,
    /// Number of pending entries to replicate.
    pub pending_entries: u64,
}

/// A single metric sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    /// Metric name (e.g., "kimberlite.writes.total").
    pub name: String,
    /// Metric value.
    pub value: f64,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Optional labels for the metric.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<(String, String)>,
}

/// A log entry from a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Log level.
    pub level: LogLevel,
    /// Log message.
    pub message: String,
    /// Optional structured fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<(String, String)>,
}

/// Log severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

// ============================================================================
// Control Plane → Agent Messages
// ============================================================================

/// Messages sent from the control plane to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    /// Push a new configuration to the agent.
    ConfigUpdate {
        /// Message ID for correlation with `ConfigAck`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<MessageId>,
        /// Version of this configuration.
        version: ConfigVersion,
        /// Configuration content (JSON-encoded).
        config: String,
        /// Checksum for integrity verification.
        checksum: String,
    },

    /// Execute an administrative command.
    AdminCommand {
        /// Message ID for correlation with `ControlAck`.
        message_id: MessageId,
        /// The command to execute.
        command: AdminCommand,
    },

    /// Request an immediate heartbeat.
    HeartbeatRequest,

    /// Request graceful shutdown.
    Shutdown {
        /// Reason for the shutdown.
        reason: String,
    },

    /// Authentication challenge (sent after connection).
    AuthChallenge(AuthChallenge),

    /// Flow control signal for backpressure.
    FlowControl {
        /// Target data stream.
        stream_type: DataStreamType,
        /// Flow control action.
        signal: FlowControlSignal,
    },

    /// Health check request.
    HealthCheck(HealthCheckRequest),
}

/// Administrative commands that can be sent to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum AdminCommand {
    /// Trigger a snapshot of the current state.
    TakeSnapshot,
    /// Compact the log up to a given offset.
    CompactLog { up_to_offset: u64 },
    /// Step down from leader role (if leader).
    StepDown,
    /// Transfer leadership to a specific node.
    TransferLeadership { target_node_id: String },
    /// Pause replication (for maintenance).
    PauseReplication,
    /// Resume replication.
    ResumeReplication,
    /// Upgrade the node to a new version.
    UpgradeVersion {
        /// Target version to upgrade to.
        target_version: String,
    },
    /// Get the current version of the node.
    GetVersion,
    /// Prepare for a backup operation.
    PrepareBackup {
        /// Backup identifier for correlation.
        backup_id: String,
    },
    /// Stream backup data from a specific offset range.
    StreamBackupData {
        /// Backup identifier for correlation.
        backup_id: String,
        /// Starting offset.
        from_offset: u64,
        /// Ending offset (exclusive).
        to_offset: u64,
    },
    /// Restore data from a backup.
    RestoreData {
        /// Restore identifier for correlation.
        restore_id: String,
        /// Expected checksum for verification.
        expected_checksum: String,
    },
    /// Drain connections and traffic from this node.
    Drain {
        /// Timeout in seconds before force drain.
        timeout_secs: u32,
    },
    /// Undrain - resume accepting connections.
    Undrain,
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during protocol operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProtocolError {
    /// Failed to serialize a message.
    #[error("serialization failed: {0}")]
    Serialization(String),

    /// Failed to deserialize a message.
    #[error("deserialization failed: {0}")]
    Deserialization(String),

    /// Invalid message format.
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// Configuration checksum mismatch.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_roundtrip() {
        let msg = AgentMessage::Heartbeat {
            node_id: "node-001".to_string(),
            status: NodeStatus::Healthy,
            role: NodeRole::Leader,
            resources: Resources {
                cpu_percent: 45.2,
                memory_used_bytes: 1_073_741_824,
                memory_total_bytes: 8_589_934_592,
                disk_used_bytes: 10_737_418_240,
                disk_total_bytes: 107_374_182_400,
            },
            replication: None,
            buffer_stats: vec![],
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::Heartbeat {
                node_id, status, ..
            } => {
                assert_eq!(node_id, "node-001");
                assert_eq!(status, NodeStatus::Healthy);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn config_update_roundtrip() {
        let msg = ControlMessage::ConfigUpdate {
            message_id: Some(MessageId(1)),
            version: ConfigVersion(42),
            config: r#"{"max_connections": 100}"#.to_string(),
            checksum: "sha256:abc123".to_string(),
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: ControlMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            ControlMessage::ConfigUpdate { version, .. } => {
                assert_eq!(version, ConfigVersion(42));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn admin_command_variants() {
        let commands = vec![
            AdminCommand::TakeSnapshot,
            AdminCommand::CompactLog { up_to_offset: 1000 },
            AdminCommand::StepDown,
            AdminCommand::TransferLeadership {
                target_node_id: "node-002".to_string(),
            },
            AdminCommand::PauseReplication,
            AdminCommand::ResumeReplication,
            AdminCommand::UpgradeVersion {
                target_version: "0.2.0".to_string(),
            },
            AdminCommand::GetVersion,
            AdminCommand::PrepareBackup {
                backup_id: "backup-001".to_string(),
            },
            AdminCommand::StreamBackupData {
                backup_id: "backup-001".to_string(),
                from_offset: 0,
                to_offset: 1000,
            },
            AdminCommand::RestoreData {
                restore_id: "restore-001".to_string(),
                expected_checksum: "sha256:abc123".to_string(),
            },
            AdminCommand::Drain { timeout_secs: 30 },
            AdminCommand::Undrain,
        ];

        for (i, cmd) in commands.into_iter().enumerate() {
            let msg = ControlMessage::AdminCommand {
                message_id: MessageId(i as u64),
                command: cmd,
            };
            let json = serde_json::to_string(&msg).expect("serialize");
            let _decoded: ControlMessage = serde_json::from_str(&json).expect("deserialize");
        }
    }

    #[test]
    fn metrics_batch_roundtrip() {
        let msg = AgentMessage::MetricsBatch {
            node_id: "node-001".to_string(),
            metrics: vec![
                MetricSample {
                    name: "kimberlite.writes.total".to_string(),
                    value: 12345.0,
                    timestamp_ms: 1700000000000,
                    labels: vec![("tenant".to_string(), "acme".to_string())],
                },
                MetricSample {
                    name: "kimberlite.reads.total".to_string(),
                    value: 98765.0,
                    timestamp_ms: 1700000000000,
                    labels: vec![],
                },
            ],
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::MetricsBatch { metrics, .. } => {
                assert_eq!(metrics.len(), 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn backoff_config_default() {
        let config = BackoffConfig::default();
        assert_eq!(config.initial_delay_ms, 1_000);
        assert_eq!(config.max_delay_ms, 60_000);
        assert_eq!(config.multiplier, 2.0);
    }

    #[test]
    fn backoff_delay_calculation() {
        let config = BackoffConfig {
            initial_delay_ms: 1_000,
            max_delay_ms: 60_000,
            multiplier: 2.0,
            jitter_factor: 0.0, // No jitter for deterministic test
            max_attempts: 0,
        };

        // Attempt 0: 1s
        assert_eq!(
            config.delay_for_attempt(0),
            Some(Duration::from_millis(1_000))
        );

        // Attempt 1: 2s
        assert_eq!(
            config.delay_for_attempt(1),
            Some(Duration::from_millis(2_000))
        );

        // Attempt 2: 4s
        assert_eq!(
            config.delay_for_attempt(2),
            Some(Duration::from_millis(4_000))
        );

        // Attempt 6: would be 64s, capped to 60s
        assert_eq!(
            config.delay_for_attempt(6),
            Some(Duration::from_millis(60_000))
        );
    }

    #[test]
    fn backoff_max_attempts() {
        let config = BackoffConfig {
            max_attempts: 3,
            ..Default::default()
        };

        assert!(config.delay_for_attempt(0).is_some());
        assert!(config.delay_for_attempt(2).is_some());
        assert!(config.delay_for_attempt(3).is_none());
    }

    #[test]
    fn backoff_state_tracking() {
        let mut state = BackoffState::default();
        assert_eq!(state.consecutive_failures, 0);

        state.record_failure(1000);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_attempt_ms, Some(1000));

        state.record_failure(2000);
        assert_eq!(state.consecutive_failures, 2);

        state.record_success(3000);
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.last_success_ms, Some(3000));
    }

    #[test]
    fn connection_state_serialization() {
        let states = vec![
            ConnectionState::Disconnected,
            ConnectionState::Backoff,
            ConnectionState::Connecting,
            ConnectionState::Connected,
            ConnectionState::Authenticated,
            ConnectionState::Closing,
        ];

        for state in states {
            let json = serde_json::to_string(&state).expect("serialize");
            let decoded: ConnectionState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, state);
        }
    }

    #[test]
    fn flow_control_roundtrip() {
        let msg = ControlMessage::FlowControl {
            stream_type: DataStreamType::Metrics,
            signal: FlowControlSignal::SlowDown {
                min_interval_ms: 10_000,
            },
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: ControlMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            ControlMessage::FlowControl {
                stream_type,
                signal,
            } => {
                assert_eq!(stream_type, DataStreamType::Metrics);
                match signal {
                    FlowControlSignal::SlowDown { min_interval_ms } => {
                        assert_eq!(min_interval_ms, 10_000);
                    }
                    _ => panic!("wrong signal variant"),
                }
            }
            _ => panic!("wrong message variant"),
        }
    }

    #[test]
    fn auth_challenge_roundtrip() {
        let challenge = AuthChallenge {
            challenge: "abc123".to_string(),
            supported_methods: vec![AuthMethod::Bearer, AuthMethod::PreSharedKey],
            expires_in_ms: 30_000,
        };

        let msg = ControlMessage::AuthChallenge(challenge);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: ControlMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            ControlMessage::AuthChallenge(c) => {
                assert_eq!(c.challenge, "abc123");
                assert_eq!(c.supported_methods.len(), 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn auth_response_roundtrip() {
        let response = AuthResponse {
            credentials: AgentCredentials::Bearer {
                token: "jwt.token.here".to_string(),
            },
            agent_info: AgentInfo {
                version: "1.0.0".to_string(),
                protocol_version: "v1".to_string(),
                capabilities: vec!["snapshots".to_string()],
            },
        };

        let msg = AgentMessage::AuthResponse(response);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::AuthResponse(r) => match r.credentials {
                AgentCredentials::Bearer { token } => {
                    assert_eq!(token, "jwt.token.here");
                }
                _ => panic!("wrong credentials variant"),
            },
            _ => panic!("wrong message variant"),
        }
    }

    #[test]
    fn health_check_roundtrip() {
        let request = HealthCheckRequest {
            request_id: MessageId(42),
            checks: vec![HealthCheckType::Liveness, HealthCheckType::Storage],
        };

        let msg = ControlMessage::HealthCheck(request);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: ControlMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            ControlMessage::HealthCheck(r) => {
                assert_eq!(r.request_id, MessageId(42));
                assert_eq!(r.checks.len(), 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn health_check_response_roundtrip() {
        let response = HealthCheckResponse {
            request_id: MessageId(42),
            status: HealthStatus::Healthy,
            checks: vec![HealthCheckResult {
                check_type: HealthCheckType::Liveness,
                passed: true,
                message: "OK".to_string(),
                details: vec![],
            }],
            duration_ms: 5,
        };

        let msg = AgentMessage::HealthCheckResponse(response);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::HealthCheckResponse(r) => {
                assert_eq!(r.status, HealthStatus::Healthy);
                assert_eq!(r.checks.len(), 1);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn control_ack_roundtrip() {
        let ack = ControlAck {
            message_id: MessageId(99),
            success: true,
            error: None,
            result: Some(r#"{"snapshot_id": "snap-001"}"#.to_string()),
            duration_ms: 1234,
        };

        let msg = AgentMessage::ControlAck(ack);
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::ControlAck(a) => {
                assert_eq!(a.message_id, MessageId(99));
                assert!(a.success);
                assert!(a.result.is_some());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn buffer_stats_in_heartbeat() {
        let msg = AgentMessage::Heartbeat {
            node_id: "node-001".to_string(),
            status: NodeStatus::Healthy,
            role: NodeRole::Leader,
            resources: Resources {
                cpu_percent: 45.2,
                memory_used_bytes: 1_073_741_824,
                memory_total_bytes: 8_589_934_592,
                disk_used_bytes: 10_737_418_240,
                disk_total_bytes: 107_374_182_400,
            },
            replication: None,
            buffer_stats: vec![
                BufferStats {
                    stream_type: DataStreamType::Metrics,
                    state: BufferState::Normal,
                    pending_items: 50,
                    capacity: 1000,
                    dropped_count: 0,
                    oldest_item_age_ms: 500,
                },
                BufferStats {
                    stream_type: DataStreamType::Logs,
                    state: BufferState::High,
                    pending_items: 800,
                    capacity: 1000,
                    dropped_count: 5,
                    oldest_item_age_ms: 2000,
                },
            ],
        };

        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: AgentMessage = serde_json::from_str(&json).expect("deserialize");

        match decoded {
            AgentMessage::Heartbeat { buffer_stats, .. } => {
                assert_eq!(buffer_stats.len(), 2);
                assert_eq!(buffer_stats[0].stream_type, DataStreamType::Metrics);
                assert_eq!(buffer_stats[1].dropped_count, 5);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn health_thresholds_default() {
        let thresholds = HealthThresholds::default();
        assert_eq!(thresholds.heartbeat_timeout_ms, 30_000);
        assert_eq!(thresholds.replication_lag_warn_ms, 5_000);
        assert_eq!(thresholds.disk_usage_critical_percent, 95.0);
    }
}
