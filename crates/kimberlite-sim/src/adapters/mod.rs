//! Reality adapters for deterministic simulation testing.
//!
//! This module provides trait-based abstractions for all simulation-vs-production
//! boundaries, enabling:
//! - **Zero-cost abstraction**: Hot-path traits (Clock, Rng) use generics for inlining
//! - **Pluggability**: Swap between simulation and production implementations
//! - **Per-node isolation**: Different adapters per replica (clock skew, forked RNGs)
//! - **FCIS compliance**: Clear functional core / imperative shell boundary
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Pure Functional Core (Kernel)              │
//! ├─────────────────────────────────────────────┤
//! │  Reality Adapters (Trait Boundary)          │
//! │  • Clock    (time source)                   │
//! │  • Rng      (randomness)                    │
//! │  • Network  (message passing)               │
//! │  • Storage  (I/O)                           │
//! │  • Scheduler (event ordering)               │
//! │  • Crash    (failure injection)             │
//! ├─────────────────────────────────────────────┤
//! │  Imperative Shell (Simulation or Production)│
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Performance Considerations
//!
//! - **Hot path** (Clock, Rng): Use generics for monomorphization → zero overhead
//! - **Cold path** (Network, Storage, Crash): Trait objects acceptable
//! - **Scheduler**: Generic or trait object depending on use case
//!
//! # Example: Per-Node Clock Skew
//!
//! ```rust,ignore
//! let replica0 = VsrReplicaWrapper::new(
//!     0,
//!     SimClock::new(),           // No skew
//!     SimRng::new(seed1),
//!     /* ... */
//! );
//!
//! let replica1 = VsrReplicaWrapper::new(
//!     1,
//!     SimClock::with_skew(-5_000_000),  // 5ms behind
//!     SimRng::new(seed2),
//!     /* ... */
//! );
//! ```

pub mod clock;
pub mod crash;
pub mod network;
pub mod rng;
pub mod scheduler;
pub mod storage;

// Re-export primary types
pub use clock::{Clock, SimClock};
pub use crash::{CrashController, CrashRecoveryEngine, CrashScenario, CrashState};
pub use network::{Network, NetworkConfig, SimNetwork};
pub use rng::{Rng, SimRng};
pub use scheduler::{EventQueue, Scheduler};
pub use storage::{SimStorage, Storage, StorageCheckpoint};
