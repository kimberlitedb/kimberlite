//! Download Handlers
//!
//! Platform detection and download redirects.

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Redirect},
};

use crate::templates::DownloadTemplate;

/// GitHub release base URL.
const GITHUB_RELEASE_BASE: &str =
    "https://github.com/kimberlite-db/kimberlite/releases/latest/download";

/// Handler for /download - auto-detects platform and redirects.
pub async fn download(headers: HeaderMap) -> impl IntoResponse {
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Detect platform from User-Agent
    let redirect_url = detect_platform_url(ua);

    match redirect_url {
        Some(url) => Redirect::temporary(&url).into_response(),
        None => {
            // Can't detect platform, show manual selection page
            DownloadTemplate::new("Download").into_response()
        }
    }
}

/// Handler for /download/manual - shows all platform options.
pub async fn download_manual() -> impl IntoResponse {
    DownloadTemplate::new("Download")
}

/// Detect the appropriate download URL based on User-Agent string.
fn detect_platform_url(ua: &str) -> Option<String> {
    let ua_lower = ua.to_lowercase();

    // Linux ARM64
    if ua_lower.contains("linux") && (ua_lower.contains("aarch64") || ua_lower.contains("arm64")) {
        return Some(format!("{GITHUB_RELEASE_BASE}/kimberlite-linux-aarch64.zip"));
    }

    // Linux x86_64
    if ua_lower.contains("linux") {
        return Some(format!("{GITHUB_RELEASE_BASE}/kimberlite-linux-x86_64.zip"));
    }

    // macOS ARM64 (Apple Silicon)
    // Note: Safari on Apple Silicon doesn't reliably indicate ARM in UA,
    // so we default macOS to ARM since most new Macs are Apple Silicon
    if ua_lower.contains("macintosh") || ua_lower.contains("mac os") {
        // Check for explicit Intel indicators
        if ua_lower.contains("intel") {
            return Some(format!("{GITHUB_RELEASE_BASE}/kimberlite-macos-x86_64.zip"));
        }
        // Default to ARM for modern Macs
        return Some(format!("{GITHUB_RELEASE_BASE}/kimberlite-macos-aarch64.zip"));
    }

    // Windows
    if ua_lower.contains("windows") {
        return Some(format!("{GITHUB_RELEASE_BASE}/kimberlite-windows-x86_64.zip"));
    }

    // Unknown platform
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linux_x86_64() {
        let ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";
        let url = detect_platform_url(ua).unwrap();
        assert!(url.contains("linux-x86_64"));
    }

    #[test]
    fn test_linux_arm64() {
        let ua = "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/537.36";
        let url = detect_platform_url(ua).unwrap();
        assert!(url.contains("linux-aarch64"));
    }

    #[test]
    fn test_macos_arm() {
        let ua = "Mozilla/5.0 (Macintosh; Apple M1) AppleWebKit/537.36";
        let url = detect_platform_url(ua).unwrap();
        assert!(url.contains("macos-aarch64"));
    }

    #[test]
    fn test_macos_intel() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";
        let url = detect_platform_url(ua).unwrap();
        assert!(url.contains("macos-x86_64"));
    }

    #[test]
    fn test_windows() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
        let url = detect_platform_url(ua).unwrap();
        assert!(url.contains("windows-x86_64"));
    }

    #[test]
    fn test_unknown() {
        let ua = "curl/7.68.0";
        assert!(detect_platform_url(ua).is_none());
    }
}
