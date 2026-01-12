//! vdb-projections: SQLCipher projection runtime for VerityDB
//!
//! Projections transform the event log into queryable SQLite views.
//! Each projection maintains its own encrypted SQLite database.
//!
//! Features:
//! - SQLCipher encryption (via sqlx + libsqlite3-sys)
//! - Optimized read/write connection pools
//! - Checkpoint tracking (last_offset, checksum)
//! - Snapshot/restore for fast recovery
//! - ProjectionRunner for continuous event application

pub mod checkpoint;
pub mod error;
pub mod pool;

pub use error::ProjectionError;
pub use pool::{PoolConfig, ProjectionDb};
