//! # kmb-client: RPC client for `Kimberlite`
//!
//! This crate provides a synchronous RPC client for communicating with
//! a `Kimberlite` server using the binary wire protocol defined in `kmb-wire`.
//!
//! ## Usage
//!
//! ```ignore
//! use kmb_client::{Client, ClientConfig};
//! use kmb_types::{DataClass, TenantId};
//!
//! // Connect to server
//! let mut client = Client::connect(
//!     "127.0.0.1:5432",
//!     TenantId::new(1),
//!     ClientConfig::default(),
//! )?;
//!
//! // Create a stream
//! let stream_id = client.create_stream("events", DataClass::NonPHI)?;
//!
//! // Append events
//! let offset = client.append(stream_id, vec![
//!     b"event1".to_vec(),
//!     b"event2".to_vec(),
//! ])?;
//!
//! // Read events back
//! let events = client.read_events(stream_id, kmb_types::Offset::new(0), 1024)?;
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
//! use kmb_client::ClientConfig;
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

pub use client::{Client, ClientConfig};
pub use error::{ClientError, ClientResult};

// Re-export useful types from dependencies
pub use kmb_wire::{QueryParam, QueryResponse, QueryValue, ReadEventsResponse};
