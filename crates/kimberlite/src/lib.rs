//! # Kimberlite
//!
//! Compliance-native database for regulated industries.
//!
//! Kimberlite is built on a replicated append-only log with deterministic
//! projection to a custom storage engine. This provides:
//!
//! - **Correctness by design** - Ordered log → deterministic apply → snapshot
//! - **Full audit trail** - Every mutation is captured in the immutable log
//! - **Point-in-time recovery** - Replay from any offset
//! - **Compliance by construction** - Built-in durability and encryption
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                          Kimberlite                             │
//! │  ┌─────────┐   ┌───────────┐   ┌──────────┐   ┌──────────┐ │
//! │  │   Log   │ → │  Kernel   │ → │  Store   │ → │  Query   │ │
//! │  │(append) │   │(pure FSM) │   │(B+tree)  │   │  (SQL)   │ │
//! │  └─────────┘   └───────────┘   └──────────┘   └──────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ```ignore
//! use kimberlite::{Kimberlite, TenantId, DataClass};
//!
//! // Open database
//! let db = Kimberlite::open("./data")?;
//!
//! // Get tenant handle
//! let tenant = db.tenant(TenantId::new(1));
//!
//! // Create a stream
//! let stream_id = tenant.create_stream("events", DataClass::NonPHI)?;
//!
//! // Append events
//! tenant.append(stream_id, vec![b"event1".to_vec(), b"event2".to_vec()])?;
//!
//! // Query (point-in-time support)
//! let results = tenant.query("SELECT * FROM events LIMIT 10", &[])?;
//! ```
//!
//! # Modules
//!
//! - **SDK Layer**: [`Kimberlite`], [`TenantHandle`] - Main API
//! - **Foundation**: Types, crypto, storage primitives
//! - **Query**: SQL subset for compliance lookups

mod error;
mod kimberlite;
mod tenant;

// SDK Layer - Main API
pub use error::{KimberliteError, Result};
pub use kimberlite::{Kimberlite, KimberliteConfig};
pub use tenant::{ExecuteResult, TenantHandle};

// Re-export core types from kmb-types
pub use kmb_types::{
    DataClass, GroupId, Offset, Placement, Region, StreamId, StreamMetadata, StreamName, TenantId,
};

// Re-export crypto primitives
pub use kmb_crypto::{ChainHash, chain_hash};

// Re-export field-level encryption
pub use kmb_crypto::{FieldKey, ReversibleToken, Token, decrypt_field, encrypt_field, tokenize};

// Re-export anonymization utilities
pub use kmb_crypto::{
    DatePrecision, GeoLevel, KAnonymityResult, MaskStyle, check_k_anonymity, generalize_age,
    generalize_numeric, generalize_zip, mask, redact, truncate_date,
};

// Re-export storage types
pub use kmb_storage::{Record, Storage, StorageError};

// Re-export kernel types
pub use kmb_kernel::{Command, Effect, KernelError, State, apply_committed};

// Re-export directory
pub use kmb_directory::{Directory, DirectoryError};

// Re-export query types for SQL operations
pub use kmb_query::{
    ColumnDef, ColumnName, DataType, QueryEngine, QueryError, QueryResult, Row, Schema,
    SchemaBuilder, TableDef, TableName, Value,
};

// Re-export store types for advanced usage
pub use kmb_store::{BTreeStore, Key, ProjectionStore, StoreError, TableId, WriteBatch, WriteOp};
