//! Client error types.

use kimberlite_wire::{ErrorCode, WireError};
use thiserror::Error;

/// Result type for client operations.
pub type ClientResult<T> = Result<T, ClientError>;

/// Errors that can occur during client operations.
#[derive(Debug, Error)]
pub enum ClientError {
    /// Connection error.
    #[error("connection error: {0}")]
    Connection(#[from] std::io::Error),

    /// Wire protocol error.
    #[error("wire protocol error: {0}")]
    Wire(#[from] WireError),

    /// Server returned an error response.
    #[error("server error ({code:?}): {message}")]
    Server { code: ErrorCode, message: String },

    /// Connection not established.
    #[error("not connected to server")]
    NotConnected,

    /// Response ID mismatch.
    #[error("response ID {received} does not match request ID {expected}")]
    ResponseMismatch { expected: u64, received: u64 },

    /// Unexpected response type.
    #[error("unexpected response type: expected {expected}, got {actual}")]
    UnexpectedResponse { expected: String, actual: String },

    /// Connection timeout.
    #[error("connection timeout")]
    Timeout,

    /// Handshake failed.
    #[error("handshake failed: {0}")]
    HandshakeFailed(String),
}

impl ClientError {
    /// Creates a server error from an error code and message.
    pub fn server(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Server {
            code,
            message: message.into(),
        }
    }

    /// Returns the wire error code if this is a server error.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Self::Server { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// True if the error is likely to succeed on retry (transient failure).
    ///
    /// Returns `true` for network errors, timeouts, and server-side transient
    /// states: `RateLimited`, `NotLeader`, `ProjectionLag`.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection(_) | Self::Timeout => true,
            Self::Server { code, .. } => matches!(
                code,
                ErrorCode::RateLimited | ErrorCode::NotLeader | ErrorCode::ProjectionLag
            ),
            _ => false,
        }
    }

    /// True if this is an optimistic-concurrency conflict on append.
    ///
    /// The caller should re-read the stream offset and retry.
    pub fn is_offset_mismatch(&self) -> bool {
        matches!(
            self,
            Self::Server {
                code: ErrorCode::OffsetMismatch,
                ..
            }
        )
    }

    /// True if authentication failed (bad token, expired JWT, revoked API key).
    pub fn is_auth_failed(&self) -> bool {
        matches!(
            self,
            Self::Server {
                code: ErrorCode::AuthenticationFailed,
                ..
            } | Self::HandshakeFailed(_)
        )
    }

    /// True if the request targeted a non-leader replica. The error message
    /// may include a leader hint; the caller should reconnect to that address.
    pub fn is_not_leader(&self) -> bool {
        matches!(
            self,
            Self::Server {
                code: ErrorCode::NotLeader,
                ..
            }
        )
    }

    /// True if a named resource (stream, table, tenant) was not found.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::Server {
                code: ErrorCode::StreamNotFound
                    | ErrorCode::TenantNotFound
                    | ErrorCode::TableNotFound,
                ..
            }
        )
    }

    /// True if the server rejected the request due to rate limiting.
    pub fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            Self::Server {
                code: ErrorCode::RateLimited,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        let timeout = ClientError::Timeout;
        assert!(timeout.is_retryable());

        let rate_limited = ClientError::server(ErrorCode::RateLimited, "slow down");
        assert!(rate_limited.is_retryable());
        assert!(rate_limited.is_rate_limited());

        let not_leader = ClientError::server(ErrorCode::NotLeader, "leader is elsewhere");
        assert!(not_leader.is_retryable());
        assert!(not_leader.is_not_leader());

        let auth_failed = ClientError::server(ErrorCode::AuthenticationFailed, "bad token");
        assert!(!auth_failed.is_retryable());
        assert!(auth_failed.is_auth_failed());

        let parse_error = ClientError::server(ErrorCode::QueryParseError, "bad SQL");
        assert!(!parse_error.is_retryable());
    }

    #[test]
    fn offset_mismatch_predicate() {
        let err = ClientError::server(ErrorCode::OffsetMismatch, "conflict");
        assert!(err.is_offset_mismatch());
        assert_eq!(err.code(), Some(ErrorCode::OffsetMismatch));

        let other = ClientError::server(ErrorCode::InvalidOffset, "bad offset");
        assert!(!other.is_offset_mismatch());
    }

    #[test]
    fn not_found_predicate() {
        assert!(ClientError::server(ErrorCode::StreamNotFound, "").is_not_found());
        assert!(ClientError::server(ErrorCode::TenantNotFound, "").is_not_found());
        assert!(ClientError::server(ErrorCode::TableNotFound, "").is_not_found());
        assert!(!ClientError::server(ErrorCode::InternalError, "").is_not_found());
    }

    #[test]
    fn handshake_failed_is_auth() {
        let err = ClientError::HandshakeFailed("bad version".to_string());
        assert!(err.is_auth_failed());
        assert_eq!(err.code(), None);
    }
}
