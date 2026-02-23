//! Wizard-style output for interactive CLI flows.
//!
//! Provides box-drawing character output matching the diamond theme
//! used throughout Kimberlite's CLI.

use super::colors::SemanticStyle;

const BAR: &str = "\u{2502}";   // │
const CORNER: &str = "\u{2514}"; // └
const DIAMOND: &str = "\u{25c6}"; // ◆

/// Prints the branded wizard welcome header.
pub fn print_wizard_welcome(version: &str) {
    println!();
    println!("  {}  {}  {}", DIAMOND.info(), "Kimberlite".header(), format!("v{version}").muted());
    println!("  {}", BAR.muted());
    println!("  {}  {}", BAR.muted(), "The compliance-first database".muted());
    println!("  {}", BAR.muted());
}

/// Prints a step header with diamond prefix.
pub fn print_step(label: &str) {
    println!("  {}  {}", DIAMOND.info(), label.header());
}

/// Prints a vertical bar connector line.
pub fn print_bar() {
    println!("  {}", BAR.muted());
}

/// Prints a bar-indented content line.
pub fn print_bar_line(text: &str) {
    println!("  {}  {}", BAR.muted(), text);
}

/// Prints a success item under the current step.
pub fn print_wizard_check(msg: &str) {
    println!("  {}  {} {}", BAR.muted(), "\u{2713}".success(), msg);
}

/// Prints the wizard completion summary.
pub fn print_wizard_summary(location: &str, template: &str, config: &str) {
    println!();
    print_step("Your project is ready!");
    print_bar();
    print_bar_line(&format!("{}    {}", "Location".muted(), location));
    print_bar_line(&format!("{}    {}", "Template".muted(), template));
    print_bar_line(&format!("{}      {}", "Config".muted(), config));
    println!();
    println!("  {}  {}", CORNER.muted(), "Next steps".header());
}

/// Prints a next-step command under the corner.
pub fn print_wizard_next(cmd: &str) {
    println!("     {}", cmd.code());
}

/// Prints a clean cancellation message.
pub fn print_wizard_canceled() {
    println!();
    println!("  {}  {}", BAR.muted(), "Operation cancelled.".muted());
    println!();
}

/// Returns `true` if stdout is an interactive terminal.
pub fn is_interactive() -> bool {
    console::Term::stdout().is_term()
}
