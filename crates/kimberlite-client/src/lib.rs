//! # kmb-client: RPC client for `Kimberlite`
//!
//! This crate provides a synchronous RPC client for communicating with
//! a `Kimberlite` server using the binary wire protocol defined in `kmb-wire`.
//!
//! ## Usage
//!
//! ```ignore
//! use kimberlite_client::{Client, ClientConfig};
//! use kimberlite_types::{DataClass, TenantId};
//!
//! // Connect to server
//! let mut client = Client::connect(
//!     "127.0.0.1:5432",
//!     TenantId::new(1),
//!     ClientConfig::default(),
//! )?;
//!
//! // Create a stream
//! let stream_id = client.create_stream("events", DataClass::Public)?;
//!
//! // Append events
//! let offset = client.append(stream_id, vec![
//!     b"event1".to_vec(),
//!     b"event2".to_vec(),
//! ], kimberlite_types::Offset::ZERO)?;
//!
//! // Read events back
//! let events = client.read_events(stream_id, kimberlite_types::Offset::new(0), 1024)?;
//!
//! // Execute a query
//! let result = client.query("SELECT * FROM streams", &[])?;
//! ```
//!
//! ## Configuration
//!
//! The client can be configured with timeouts and buffer sizes:
//!
//! ```ignore
//! use kimberlite_client::ClientConfig;
//! use std::time::Duration;
//!
//! let config = ClientConfig {
//!     read_timeout: Some(Duration::from_secs(60)),
//!     write_timeout: Some(Duration::from_secs(30)),
//!     buffer_size: 128 * 1024,
//!     auth_token: Some("secret-token".to_string()),
//! };
//! ```

mod client;
mod error;
mod pool;
mod query_builder;
mod subscription;

#[cfg(feature = "typed-rows")]
mod typed_row;

pub use client::{Client, ClientConfig};
pub use error::{ClientError, ClientResult};
pub use pool::{Pool, PoolConfig, PoolStats, PooledClient};
pub use query_builder::Query;
pub use subscription::{Subscription, SubscriptionEvent};

#[cfg(feature = "typed-rows")]
pub use typed_row::{FromRow, RowDeserializeError, map_rows, rows_as_maps};

// Re-export useful types from dependencies
pub use kimberlite_wire::{
    ApiKeyInfo, ApiKeyRegisterResponse, ApiKeyRotateResponse, AuditEventInfo, BreachEventInfo,
    BreachIndicatorPayload, BreachReportInfo, BreachSeverity, BreachStatusTag, ClusterMode,
    ColumnInfo, ConsentGrantResponse, ConsentPurpose, ConsentRecord, ConsentScope,
    ConsentWithdrawResponse, DescribeTableResponse, ErasureAuditInfo, ErasureExemptionBasis,
    ErasureRequestInfo, ErasureStatusTag, ErrorCode, ExportFormat, IndexInfo,
    PortabilityExportInfo, PushPayload, QueryParam, QueryResponse, QueryValue, ReadEventsResponse,
    ServerInfoResponse, SubscribeResponse, SubscriptionCloseReason, TableInfo, TenantCreateResponse,
    TenantDeleteResponse, TenantInfo, VerifyExportResponse,
};
