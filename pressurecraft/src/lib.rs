//! # Pressurecraft: Learn FCIS by Building
//!
//! This crate teaches the Functional Core, Imperative Shell (FCIS) pattern
//! by building a mini database kernel from scratch.
//!
//! ## Learning Path
//!
//! 1. [`step1_pure_functions`] - Pure vs impure functions
//! 2. [`step2_commands_effects`] - Command/Effect pattern
//! 3. [`step3_state_machine`] - State transitions
//! 4. [`step4_mini_kernel`] - The `apply()` function
//! 5. [`step5_full_kernel`] - Production-ready kernel
//!
//! ## Quick Start
//!
//! ```bash
//! # Run all tests
//! cargo test
//!
//! # Run specific step
//! cargo test step1
//!
//! # Run examples
//! cargo run --example counter
//! ```

pub mod step1_pure_functions;
pub mod step2_commands_effects;
pub mod step3_state_machine;
pub mod step4_mini_kernel;
pub mod step5_full_kernel;

#[cfg(test)]
mod tests;
