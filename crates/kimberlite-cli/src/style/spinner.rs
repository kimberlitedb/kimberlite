//! Spinner helpers using indicatif.

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

/// Diamond-themed spinner characters.
const DIAMOND_SPINNER: &[&str] = &["◇ ", "◆ ", "◇ ", "◆ "];

/// Creates a new diamond-themed spinner with a message.
pub fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();

    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(DIAMOND_SPINNER)
            .template("{spinner:.cyan} {msg}")
            .expect("invalid spinner template"),
    );

    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));

    pb
}

/// Finishes a spinner with a success message.
pub fn finish_success(pb: &ProgressBar, msg: &str) {
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{prefix} {msg}")
            .expect("invalid spinner template"),
    );
    pb.set_prefix("✓");
    pb.finish_with_message(msg.to_string());
}

/// Finishes a spinner with an error message.
pub fn finish_error(pb: &ProgressBar, msg: &str) {
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{prefix} {msg}")
            .expect("invalid spinner template"),
    );
    pb.set_prefix("✗");
    pb.finish_with_message(msg.to_string());
}

/// Finishes a spinner and clears it from the terminal.
pub fn finish_and_clear(pb: &ProgressBar) {
    pb.finish_and_clear();
}
