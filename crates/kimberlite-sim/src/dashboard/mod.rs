//! Coverage dashboard for VOPR simulation testing.
//!
//! Provides a web-based interface for viewing coverage metrics, corpus analysis,
//! and fuzzing progress in real-time.

pub mod handlers;
pub mod router;

pub use handlers::{DashboardTemplate, SeedInfo};
pub use router::{DashboardServer, DashboardState};
