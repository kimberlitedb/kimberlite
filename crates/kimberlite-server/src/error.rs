//! Server error types.

use std::net::SocketAddr;

use kimberlite::KimberliteError;
use kimberlite_wire::WireError;
use thiserror::Error;

/// Result type for server operations.
pub type ServerResult<T> = Result<T, ServerError>;

/// Errors that can occur during server operations.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Wire protocol error.
    #[error("wire protocol error: {0}")]
    Wire(#[from] WireError),

    /// Database error.
    #[error("database error: {0}")]
    Database(#[from] KimberliteError),

    /// I/O error.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Connection closed.
    #[error("connection closed")]
    ConnectionClosed,

    /// Maximum connections reached.
    #[error("maximum connections reached: {0}")]
    MaxConnectionsReached(usize),

    /// Invalid tenant ID.
    #[error("invalid tenant ID")]
    InvalidTenant,

    /// Bind failed.
    #[error("failed to bind to {addr}: {source}")]
    BindFailed {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },

    /// TLS error.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Authentication failed.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Server shutdown.
    #[error("server shutdown")]
    Shutdown,

    /// Replication error.
    #[error("replication error: {0}")]
    Replication(String),

    /// Not the leader - write requests should be redirected.
    ///
    /// This error includes an optional leader hint so clients can
    /// redirect their requests to the correct node.
    #[error("not the leader (leader hint: {leader_hint:?}, view: {view})")]
    NotLeader {
        /// The current view number.
        view: u64,
        /// Optional hint for the leader's address.
        leader_hint: Option<SocketAddr>,
    },

    /// Cluster configuration error.
    #[error("cluster configuration error: {0}")]
    ClusterConfig(String),
}

impl ServerError {
    /// Creates a `NotLeader` error with a leader hint.
    pub fn not_leader(view: u64, leader_hint: Option<SocketAddr>) -> Self {
        Self::NotLeader { view, leader_hint }
    }

    /// Returns true if this is a `NotLeader` error.
    pub fn is_not_leader(&self) -> bool {
        matches!(self, Self::NotLeader { .. })
    }

    /// Returns the leader hint if this is a `NotLeader` error.
    pub fn leader_hint(&self) -> Option<SocketAddr> {
        match self {
            Self::NotLeader { leader_hint, .. } => *leader_hint,
            _ => None,
        }
    }
}
