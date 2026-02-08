//! # kimberlite-io: I/O Backend Abstraction for Kimberlite
//!
//! This crate provides a trait-based abstraction over file I/O operations,
//! enabling the storage layer to use different I/O strategies:
//!
//! - **`SyncBackend`** (default): Standard `std::fs` operations with optional
//!   `O_DIRECT` on Linux (via the `direct_io` feature)
//! - **Future**: `io_uring` backend for async I/O on Linux
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────┐
//! │     kimberlite-storage   │
//! │   (uses IoBackend trait) │
//! └────────────┬─────────────┘
//!              │
//! ┌────────────┴─────────────┐
//! │       kimberlite-io      │
//! │  ┌─────────┐  ┌────────┐ │
//! │  │  Sync   │  │ Future │ │
//! │  │ Backend │  │ io_uring│ │
//! │  └─────────┘  └────────┘ │
//! └──────────────────────────┘
//! ```
//!
//! # Features
//!
//! - `direct_io`: Enable `O_DIRECT` support on Linux (requires `libc`)

mod aligned;
mod backend;
mod error;
mod sync_backend;

pub use aligned::{AlignedBuffer, BLOCK_ALIGNMENT};
pub use backend::{FileHandle, IoBackend, OpenFlags};
pub use error::IoError;
pub use sync_backend::SyncBackend;

#[cfg(test)]
mod tests;
