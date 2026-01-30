//! Version command implementation.

use crate::style::{banner::print_version_banner, print_info_table};

/// Version information for the CLI.
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() {
    print_version_banner(VERSION);

    let entries = [
        ("Rust version", rustc_version()),
        ("Target", std::env::consts::ARCH),
        ("OS", std::env::consts::OS),
    ];

    print_info_table(&entries);
}

fn rustc_version() -> &'static str {
    "1.88+"
}
