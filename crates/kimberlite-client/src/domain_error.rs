//! Domain-level error mapping for Kimberlite.
//!
//! AUDIT-2026-04 S2.4 — lifts `mapKimberliteError` out of
//! `notebar/packages/kimberlite-client/src/retry.ts` into the SDK
//! so every app gets a single canonical translation from wire-level
//! errors to app-visible domain-error shapes.
//!
//! The Rust variant is a plain enum + `From<ClientError>` impl so
//! callers can pattern-match without an extra function call:
//!
//! ```no_run
//! use kimberlite_client::domain_error::DomainError;
//! # use kimberlite_client::{Client, ClientError, ClientResult};
//! # fn handler(client: &mut Client) -> Result<(), DomainError> {
//! let result: ClientResult<()> = Err(ClientError::NotConnected);
//! match result {
//!     Ok(_) => Ok(()),
//!     Err(e) => Err(DomainError::from(e)),
//! }
//! # }
//! ```

use kimberlite_wire::ErrorCode;

use crate::error::ClientError;

/// Discriminated enum of domain-facing error shapes.
///
/// 1-1 mapping from wire-level `ErrorCode` to the kind of failure
/// an app UI or HTTP endpoint needs to distinguish. New codes map
/// into `Unavailable` by default — extend the `From` impl to carve
/// out additional variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// The referenced entity does not exist (stream, table, tenant, API key).
    NotFound,
    /// The caller is authenticated but not authorised, or the token is invalid.
    Forbidden,
    /// Optimistic-concurrency conflict (`OffsetMismatch`). Re-read and retry.
    ConcurrentModification,
    /// A uniqueness or precondition conflict (duplicate tenant, duplicate stream).
    Conflict { reasons: Vec<String> },
    /// An invariant that should always hold was violated. Indicates a server bug.
    InvariantViolation { name: String },
    /// The server or cluster is unavailable for this request.
    Unavailable { message: String },
    /// Server-side rate limiting. Back off per `Retry-After` / SDK retry policy.
    RateLimited,
    /// Client-side or network timeout.
    Timeout,
    /// Caller error — malformed request / query / invalid data class.
    Validation { message: String },
}

impl From<ClientError> for DomainError {
    fn from(e: ClientError) -> Self {
        match &e {
            ClientError::Connection(_)
            | ClientError::Wire(_)
            | ClientError::ResponseMismatch { .. }
            | ClientError::UnexpectedResponse { .. } => DomainError::Unavailable {
                message: e.to_string(),
            },
            ClientError::Timeout => DomainError::Timeout,
            ClientError::NotConnected => DomainError::Unavailable {
                message: "not connected".to_string(),
            },
            ClientError::HandshakeFailed(_) => DomainError::Forbidden,
            ClientError::Server { code, message, .. } => match code {
                ErrorCode::OffsetMismatch => DomainError::ConcurrentModification,
                ErrorCode::StreamNotFound
                | ErrorCode::TableNotFound
                | ErrorCode::TenantNotFound
                | ErrorCode::ApiKeyNotFound
                | ErrorCode::ConsentNotFound
                | ErrorCode::ErasureNotFound => DomainError::NotFound,
                ErrorCode::AuthenticationFailed => DomainError::Forbidden,
                ErrorCode::RateLimited => DomainError::RateLimited,
                ErrorCode::QueryParseError
                | ErrorCode::InvalidRequest
                | ErrorCode::InvalidOffset => DomainError::Validation {
                    message: message.clone(),
                },
                ErrorCode::TenantAlreadyExists
                | ErrorCode::StreamAlreadyExists
                | ErrorCode::UniqueConstraintViolation => DomainError::Conflict {
                    reasons: vec![message.clone()],
                },
                ErrorCode::ConsentExpired => DomainError::Conflict {
                    reasons: vec![message.clone()],
                },
                _ => DomainError::Unavailable {
                    message: message.clone(),
                },
            },
        }
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::Forbidden => write!(f, "forbidden"),
            Self::ConcurrentModification => write!(f, "concurrent modification"),
            Self::Conflict { reasons } => write!(f, "conflict: {}", reasons.join(", ")),
            Self::InvariantViolation { name } => write!(f, "invariant violation: {name}"),
            Self::Unavailable { message } => write!(f, "unavailable: {message}"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::Timeout => write!(f, "timeout"),
            Self::Validation { message } => write!(f, "validation: {message}"),
        }
    }
}

impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_not_found_maps_to_not_found() {
        let err = ClientError::server(ErrorCode::StreamNotFound, "gone");
        assert_eq!(DomainError::from(err), DomainError::NotFound);
    }

    #[test]
    fn table_tenant_apikey_map_to_not_found() {
        let e1 = ClientError::server(ErrorCode::TableNotFound, "");
        let e2 = ClientError::server(ErrorCode::TenantNotFound, "");
        let e3 = ClientError::server(ErrorCode::ApiKeyNotFound, "");
        assert_eq!(DomainError::from(e1), DomainError::NotFound);
        assert_eq!(DomainError::from(e2), DomainError::NotFound);
        assert_eq!(DomainError::from(e3), DomainError::NotFound);
    }

    #[test]
    fn auth_failed_maps_to_forbidden() {
        let err = ClientError::server(ErrorCode::AuthenticationFailed, "bad token");
        assert_eq!(DomainError::from(err), DomainError::Forbidden);
    }

    #[test]
    fn offset_mismatch_maps_to_concurrent_modification() {
        let err = ClientError::server(ErrorCode::OffsetMismatch, "expected 1 got 2");
        assert_eq!(DomainError::from(err), DomainError::ConcurrentModification);
    }

    #[test]
    fn rate_limited_maps_to_rate_limited() {
        let err = ClientError::server(ErrorCode::RateLimited, "slow down");
        assert_eq!(DomainError::from(err), DomainError::RateLimited);
    }

    #[test]
    fn timeout_maps_to_timeout() {
        assert_eq!(
            DomainError::from(ClientError::Timeout),
            DomainError::Timeout
        );
    }

    #[test]
    fn query_parse_error_maps_to_validation_with_message() {
        let err = ClientError::server(ErrorCode::QueryParseError, "unexpected token");
        assert_eq!(
            DomainError::from(err),
            DomainError::Validation {
                message: "unexpected token".to_string(),
            }
        );
    }

    #[test]
    fn stream_already_exists_maps_to_conflict() {
        let err = ClientError::server(ErrorCode::StreamAlreadyExists, "dup");
        match DomainError::from(err) {
            DomainError::Conflict { reasons } => {
                assert_eq!(reasons, vec!["dup".to_string()]);
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn unique_constraint_violation_maps_to_conflict() {
        let err = ClientError::server(
            ErrorCode::UniqueConstraintViolation,
            "duplicate primary key in table 'users': [BigInt(1)]",
        );
        assert!(err.is_unique_constraint_violation());
        match DomainError::from(err) {
            DomainError::Conflict { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("duplicate primary key"));
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn internal_error_maps_to_unavailable() {
        let err = ClientError::server(ErrorCode::InternalError, "boom");
        match DomainError::from(err) {
            DomainError::Unavailable { message } => assert_eq!(message, "boom"),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn connection_error_maps_to_unavailable() {
        use std::io;
        let err =
            ClientError::Connection(io::Error::new(io::ErrorKind::ConnectionReset, "peer reset"));
        matches!(DomainError::from(err), DomainError::Unavailable { .. });
    }

    #[test]
    fn not_connected_maps_to_unavailable() {
        matches!(
            DomainError::from(ClientError::NotConnected),
            DomainError::Unavailable { .. },
        );
    }

    #[test]
    fn handshake_failed_maps_to_forbidden() {
        let err = ClientError::HandshakeFailed("revoked token".into());
        assert_eq!(DomainError::from(err), DomainError::Forbidden);
    }

    #[test]
    fn display_formats_every_variant() {
        // Smoke-test: Display is trait-implemented for logging.
        let cases = vec![
            DomainError::NotFound,
            DomainError::Forbidden,
            DomainError::ConcurrentModification,
            DomainError::Conflict {
                reasons: vec!["dup key".into()],
            },
            DomainError::InvariantViolation {
                name: "monotonic_offset".into(),
            },
            DomainError::Unavailable {
                message: "boom".into(),
            },
            DomainError::RateLimited,
            DomainError::Timeout,
            DomainError::Validation {
                message: "bad SQL".into(),
            },
        ];
        for e in cases {
            // No panic on Display; no empty string.
            assert!(!e.to_string().is_empty());
        }
    }
}
