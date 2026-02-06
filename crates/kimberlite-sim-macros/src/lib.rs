//! Procedural macros for zero-cost instrumentation in simulation mode.
//!
//! These macros compile to no-ops in production builds but enable:
//! - Fault injection points
//! - Phase markers for event-triggered assertions
//! - Coverage tracking
//! - "Sometimes assertions" (deterministic sampling)
//!
//! All macros are gated by `cfg(any(test, feature = "sim"))` to ensure
//! zero overhead in production builds.

use proc_macro::TokenStream;
use quote::quote;
use syn::{LitStr, parse_macro_input};

mod deferred;
mod fault_point;
mod phase;
mod sometimes;

/// Marks a fault injection point in the code.
///
/// In simulation mode, this registers the fault point with the global registry
/// and allows the SimFaultInjector to deterministically inject faults.
///
/// In production builds, this compiles to a no-op.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::fault_point;
///
/// fn write_to_disk(data: &[u8]) -> Result<()> {
///     fault_point!("storage.disk.write");
///     // actual write logic
///     Ok(())
/// }
/// ```
#[proc_macro]
pub fn fault_point(input: TokenStream) -> TokenStream {
    let key = parse_macro_input!(input as LitStr);
    let key_str = key.value();

    let expanded = quote! {
        #[cfg(any(test, feature = "sim"))]
        {
            kimberlite_sim::instrumentation::fault_registry::record_fault_point(#key_str);
        }
    };

    TokenStream::from(expanded)
}

/// Wraps a fallible operation with fault injection capability.
///
/// In simulation mode, the SimFaultInjector can intercept and fail this operation
/// deterministically based on the current seed and step count.
///
/// In production builds, this executes the operation directly without overhead.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::fault;
///
/// fault!("storage.fsync", { file_path: &path }, || {
///     file.sync_all()
/// })
/// ```
#[proc_macro]
pub fn fault(input: TokenStream) -> TokenStream {
    let fault_input = parse_macro_input!(input as fault_point::FaultInput);
    fault_point::expand_fault(fault_input)
}

/// Marks a system phase for event-triggered assertions.
///
/// Phases are tracked in the PhaseTracker and can trigger deferred assertions.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::phase;
///
/// phase!("vsr", "prepare_sent", { view: 1, op: 42 });
/// ```
#[proc_macro]
pub fn phase(input: TokenStream) -> TokenStream {
    let phase_input = parse_macro_input!(input as phase::PhaseInput);
    phase::expand_phase(phase_input)
}

/// Deterministically sampled assertion (expensive checks).
///
/// Runs the assertion with probability 1/rate, deterministically based on
/// the global seed and current step count.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::sometimes_assert;
///
/// sometimes_assert!(
///     rate = 1000,
///     key = "hash_chain_full_verify",
///     || self.verify_full_hash_chain().is_ok(),
///     "hash chain integrity violated"
/// );
/// ```
#[proc_macro]
pub fn sometimes_assert(input: TokenStream) -> TokenStream {
    let sometimes_input = parse_macro_input!(input as sometimes::SometimesInput);
    sometimes::expand_sometimes_assert(sometimes_input)
}

/// Records that an invariant check was executed.
///
/// Increments the run count for the named invariant in the global tracker.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::invariant_check;
///
/// fn check_linearizability(&self) -> InvariantResult {
///     invariant_check!("linearizability");
///     // ... actual check logic
///     InvariantResult::Ok
/// }
/// ```
#[proc_macro]
pub fn invariant_check(input: TokenStream) -> TokenStream {
    let key = parse_macro_input!(input as LitStr);
    let key_str = key.value();

    let expanded = quote! {
        #[cfg(any(test, feature = "sim"))]
        {
            kimberlite_sim::instrumentation::invariant_tracker::record_invariant_execution(#key_str);
        }
    };

    TokenStream::from(expanded)
}

/// Asserts a condition after a trigger event occurs, within a time window.
///
/// This macro registers a deferred assertion that will fire when the specified
/// trigger event occurs (or after within_steps, whichever comes first).
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::assert_after;
///
/// // After view change completes, assert no divergence within 50k steps
/// assert_after!(
///     trigger = "vsr:view_change_complete",
///     within_steps = 50_000,
///     key = "no_divergence_after_view_change",
///     || logs_prefix_consistent(),
///     "logs should be consistent after view change"
/// );
/// ```
#[proc_macro]
pub fn assert_after(input: TokenStream) -> TokenStream {
    let assert_input = parse_macro_input!(input as deferred::AssertAfterInput);
    deferred::expand_assert_after(assert_input)
}

/// Asserts a condition within a specified number of simulation steps.
///
/// This macro registers a deferred assertion that will fire N steps from now.
///
/// # Example
///
/// ```ignore
/// use kimberlite_sim_macros::assert_within_steps;
///
/// // Within 10k steps, projection should catch up
/// assert_within_steps!(
///     steps = 10_000,
///     key = "projection_catchup",
///     || projection.applied_position() >= commit_index,
///     "projection should catch up to commit within 10k steps"
/// );
/// ```
#[proc_macro]
pub fn assert_within_steps(input: TokenStream) -> TokenStream {
    let assert_input = parse_macro_input!(input as deferred::AssertWithinStepsInput);
    deferred::expand_assert_within_steps(assert_input)
}
