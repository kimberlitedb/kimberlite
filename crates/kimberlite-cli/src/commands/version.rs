//! Version command implementation.

/// Version information for the CLI.
const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = env!("CARGO_PKG_NAME");

pub fn run() {
    println!("{NAME} {VERSION}");
    println!();
    println!("The compliance-first database for regulated industries.");
    println!();
    println!("Build info:");
    println!("  Rust version: {}", rustc_version());
    println!("  Target:       {}", std::env::consts::ARCH);
    println!("  OS:           {}", std::env::consts::OS);
}

fn rustc_version() -> &'static str {
    // Fallback to a static string since we can't easily get rustc version at runtime
    "1.88+"
}
