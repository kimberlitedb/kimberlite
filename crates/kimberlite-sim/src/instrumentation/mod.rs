//! Runtime support for zero-cost instrumentation macros.
//!
//! This module provides the runtime infrastructure that proc macros in
//! `kimberlite-sim-macros` call into. All functions are no-ops unless
//! compiled with `cfg(any(test, feature = "sim"))`.

pub mod coverage;
pub mod deferred_assertions;
pub mod fault_registry;
pub mod invariant_runtime;
pub mod invariant_tracker;
pub mod phase_tracker;

pub use coverage::CoverageReport;
pub use deferred_assertions::DeferredAssertion;
pub use fault_registry::FaultRegistry;
pub use invariant_tracker::InvariantTracker;
