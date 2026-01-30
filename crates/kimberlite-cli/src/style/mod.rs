//! CLI styling utilities for beautiful terminal output.
//!
//! Provides semantic colors, output helpers, ASCII art banners,
//! formatted tables, and animated spinners.

use std::sync::atomic::{AtomicBool, Ordering};

pub mod banner;
pub mod colors;
pub mod output;
pub mod spinner;
pub mod table;

pub use output::*;
pub use spinner::*;
pub use table::*;

/// Global flag to track if colors are disabled.
static NO_COLOR: AtomicBool = AtomicBool::new(false);

/// Sets the global no-color flag.
pub fn set_no_color(value: bool) {
    NO_COLOR.store(value, Ordering::SeqCst);
}

/// Checks if colors are disabled.
pub fn no_color() -> bool {
    NO_COLOR.load(Ordering::SeqCst)
}
