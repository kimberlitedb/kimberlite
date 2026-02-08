//! # kmb-server: `Kimberlite` server daemon
//!
//! This crate provides the TCP server that exposes `Kimberlite` over the network
//! using the binary wire protocol defined in `kmb-wire`.
//!
//! ## Architecture
//!
//! The server uses `mio` for non-blocking I/O with a poll-based event loop.
//! This follows the project's design principle of explicit control flow
//! without async runtimes.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                      kmb-server                          │
//! │  ┌─────────────┐   ┌─────────────┐   ┌───────────────┐  │
//! │  │  Listener   │ → │ Connections │ → │  RequestRouter │  │
//! │  │  (TCP)      │   │ (mio poll)  │   │  (→ Kimberlite)   │  │
//! │  └─────────────┘   └─────────────┘   └───────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! use kimberlite_server::{Server, ServerConfig};
//! use kimberlite::Kimberlite;
//!
//! let db = Kimberlite::open("./data")?;
//! let config = ServerConfig::new("127.0.0.1:5432");
//! let server = Server::new(config, db)?;
//! server.run()?;
//! ```

#![allow(clippy::cast_precision_loss)] // Server metrics use f64 for stats

pub mod auth;
pub mod bounded_queue;
pub mod buffer_pool;
mod config;
mod connection;
pub mod core_runtime;
mod error;
mod handler;
pub mod health;
pub mod http;
pub mod metrics;
mod pem;
pub mod replication;
mod server;
#[cfg(test)]
mod tests;
pub mod tls;

#[cfg(feature = "otel")]
pub mod otel;

pub use auth::{ApiKeyConfig, AuthMode, AuthService, AuthenticatedIdentity, JwtConfig};
pub use buffer_pool::BytesMutPool;
pub use config::{ClusterConfigError, RateLimitConfig, ReplicationMode, ServerConfig};
pub use core_runtime::{CoreRequest, CoreRouter, CoreRuntime, CoreRuntimeConfig};
pub use error::{ServerError, ServerResult};
pub use health::{HealthChecker, HealthResponse, HealthStatus};
pub use replication::{CommandSubmitter, ReplicationStatus, SubmissionResult};
pub use server::{Server, ShutdownHandle};
pub use tls::TlsConfig;
