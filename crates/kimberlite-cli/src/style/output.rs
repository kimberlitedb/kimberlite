//! Output helper functions for consistent styled messages.

use super::colors::SemanticStyle;

/// Prints a success message with a checkmark.
pub fn print_success(msg: &str) {
    println!("{} {}", "✓".success(), msg);
}

/// Prints an error message with an X mark.
pub fn print_error(msg: &str) {
    eprintln!("{} {}", "✗".error(), msg);
}

/// Prints a warning message with a warning symbol.
pub fn print_warn(msg: &str) {
    println!("{} {}", "⚠".warning(), msg);
}

/// Prints a hint/suggestion with an arrow.
pub fn print_hint(msg: &str) {
    println!("{} {}", "→".muted(), msg.muted());
}

/// Prints a labeled key-value pair with proper indentation.
pub fn print_labeled(key: &str, value: &str) {
    println!("  {}: {}", key.muted(), value);
}

/// Prints a code example with indentation and styling.
pub fn print_code_example(cmd: &str) {
    println!("  {}", cmd.code());
}

/// Prints an empty line for spacing.
pub fn print_spacer() {
    println!();
}
