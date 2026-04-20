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
    ///
    /// `request_id` carries the wire-request ID the server was
    /// processing when the error occurred, enabling log
    /// correlation with server-side tracing (AUDIT-2026-04 S3.8).
    /// `None` on errors synthesised client-side (e.g. decoding
    /// a response that already failed classification).
    #[error("server error ({code:?}): {message}")]
    Server {
        code: ErrorCode,
        message: String,
        request_id: Option<u64>,
    },

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
    /// Back-compat shim — the caller does not have a
    /// `request_id` to thread through. Prefer
    /// [`Self::server_with_request`] when the wire layer has
    /// the ID available.
    pub fn server(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Server {
            code,
            message: message.into(),
            request_id: None,
        }
    }

    /// Creates a server error tagged with the wire request ID
    /// the server was responding to (AUDIT-2026-04 S3.8).
    pub fn server_with_request(
        code: ErrorCode,
        message: impl Into<String>,
        request_id: u64,
    ) -> Self {
        Self::Server {
            code,
            message: message.into(),
            request_id: Some(request_id),
        }
    }

    /// Returns the wire request ID the server was processing
    /// when the error occurred, or `None` for client-side
    /// errors / server errors that pre-date S3.8.
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Server { request_id, .. } => *request_id,
            _ => None,
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

    // AUDIT-2026-04 S3.8 — request_id correlation on errors.

    #[test]
    fn server_error_without_request_id_returns_none() {
        let err = ClientError::server(ErrorCode::RateLimited, "slow down");
        assert_eq!(err.request_id(), None);
    }

    #[test]
    fn server_error_with_request_id_round_trips() {
        let err = ClientError::server_with_request(ErrorCode::QueryParseError, "bad SQL", 42);
        assert_eq!(err.request_id(), Some(42));
        assert_eq!(err.code(), Some(ErrorCode::QueryParseError));
    }

    #[test]
    fn non_server_errors_return_none_request_id() {
        let ts = ClientError::Timeout;
        assert_eq!(ts.request_id(), None);

        let nc = ClientError::NotConnected;
        assert_eq!(nc.request_id(), None);

        let hs = ClientError::HandshakeFailed("token expired".into());
        assert_eq!(hs.request_id(), None);
    }

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
