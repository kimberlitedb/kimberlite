//! ASCII art banner for Kimberlite.

use super::colors::SemanticStyle;

/// The full Kimberlite banner with diamond theme.
const BANNER: &str = r"
  ◆ K I M B E R L I T E
";

/// Prints the full banner with styling.
pub fn print_banner() {
    println!("{}", BANNER.info());
    println!("  {}", "The compliance-first database".muted());
    println!();
}

/// Prints a mini banner for use in subcommands.
pub fn print_mini_banner() {
    print!("{} {}", "◆".info(), "Kimberlite".header());
}

/// Prints the version banner.
pub fn print_version_banner(version: &str) {
    println!();
    println!(
        "  {} {} {}",
        "◆".info(),
        "Kimberlite".header(),
        format!("v{version}").muted()
    );
    println!("  {}", "The compliance-first database".muted());
    println!();
}
