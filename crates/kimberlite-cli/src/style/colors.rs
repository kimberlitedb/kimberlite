//! Semantic color palette for terminal output.
//!
//! Uses owo-colors for zero-allocation terminal coloring.

use owo_colors::{OwoColorize, Style};

/// Returns the style for success messages (green bold).
pub fn success_style() -> Style {
    Style::new().green().bold()
}

/// Returns the style for error messages (red bold).
pub fn error_style() -> Style {
    Style::new().red().bold()
}

/// Returns the style for warning messages (yellow).
pub fn warning_style() -> Style {
    Style::new().yellow()
}

/// Returns the style for informational messages (cyan).
pub fn info_style() -> Style {
    Style::new().cyan()
}

/// Returns the style for muted/secondary text (dimmed).
pub fn muted_style() -> Style {
    Style::new().dimmed()
}

/// Returns the style for headers (bold).
pub fn header_style() -> Style {
    Style::new().bold()
}

/// Returns the style for code/paths (blue).
pub fn code_style() -> Style {
    Style::new().blue()
}

/// Trait extension to apply semantic styles.
pub trait SemanticStyle: Sized {
    /// Apply success styling (green bold).
    fn success(&self) -> String;
    /// Apply error styling (red bold).
    fn error(&self) -> String;
    /// Apply warning styling (yellow).
    fn warning(&self) -> String;
    /// Apply info styling (cyan).
    fn info(&self) -> String;
    /// Apply muted styling (dimmed).
    fn muted(&self) -> String;
    /// Apply header styling (bold).
    fn header(&self) -> String;
    /// Apply code styling (blue).
    fn code(&self) -> String;
}

impl<T: std::fmt::Display> SemanticStyle for T {
    fn success(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(success_style()).to_string()
        }
    }

    fn error(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(error_style()).to_string()
        }
    }

    fn warning(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(warning_style()).to_string()
        }
    }

    fn info(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(info_style()).to_string()
        }
    }

    fn muted(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(muted_style()).to_string()
        }
    }

    fn header(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(header_style()).to_string()
        }
    }

    fn code(&self) -> String {
        if super::no_color() {
            self.to_string()
        } else {
            self.style(code_style()).to_string()
        }
    }
}

/// Format text with success styling (green bold).
pub fn success(text: &str) -> String {
    text.success()
}

/// Format text with error styling (red bold).
pub fn error(text: &str) -> String {
    text.error()
}

/// Format text with warning styling (yellow).
pub fn warning(text: &str) -> String {
    text.warning()
}

/// Format text with info styling (cyan).
pub fn info(text: &str) -> String {
    text.info()
}

/// Format text with muted styling (dimmed).
pub fn muted(text: &str) -> String {
    text.muted()
}

/// Format text with tenant styling (magenta).
pub fn tenant(text: &str) -> String {
    if super::no_color() {
        text.to_string()
    } else {
        text.magenta().to_string()
    }
}

/// Format text with code/path styling (blue).
pub fn code(text: &str) -> String {
    text.code()
}
