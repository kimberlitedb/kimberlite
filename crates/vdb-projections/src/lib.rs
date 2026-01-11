//! vdb-projections: SQLCipher projection runtime for VerityDB
//!
//! Projections transform the event log into queryable SQLite views.
//! Each projection maintains its own encrypted SQLite database.
//!
//! Features:
//! - SQLCipher encryption (via sqlx)
//! - Checkpoint tracking (last_offset, checksum)
//! - Snapshot/restore for fast recovery
//! - ProjectionRunner for continuous event application

// TODO: Implement projection runtime
