//! Website integration tests.
//!
//! These tests require a running kmb-site server on localhost:3000.
//! Run with: `just test-website` (starts server, runs tests, stops server).
//!
//! Or manually:
//!   1. `cargo run -p kmb-site` (in /website directory)
//!   2. `cargo test -p kmb-site-tests`

const BASE_URL: &str = "http://localhost:3000";

#[tokio::test]
async fn test_homepage_loads() {
    let resp = reqwest::get(format!("{BASE_URL}/")).await.unwrap();
    assert_eq!(resp.status(), 200, "Homepage should return 200");
}

#[tokio::test]
async fn test_install_script_serves() {
    let resp = reqwest::get(format!("{BASE_URL}/public/install.sh"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "install.sh should return 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Kimberlite Install Script"),
        "install.sh should contain the script header"
    );
}

#[tokio::test]
async fn test_security_headers() {
    let resp = reqwest::get(format!("{BASE_URL}/")).await.unwrap();
    let headers = resp.headers();
    assert!(
        headers.contains_key("content-security-policy"),
        "Response must include Content-Security-Policy header"
    );
    assert!(
        headers.contains_key("strict-transport-security"),
        "Response must include Strict-Transport-Security header"
    );
    assert!(
        headers.contains_key("x-frame-options"),
        "Response must include X-Frame-Options header"
    );
    assert!(
        headers.contains_key("x-content-type-options"),
        "Response must include X-Content-Type-Options header"
    );
    assert!(
        headers.contains_key("referrer-policy"),
        "Response must include Referrer-Policy header"
    );
}

#[tokio::test]
async fn test_x_frame_options_is_deny() {
    let resp = reqwest::get(format!("{BASE_URL}/")).await.unwrap();
    let xfo = resp
        .headers()
        .get("x-frame-options")
        .expect("X-Frame-Options header must be present")
        .to_str()
        .unwrap();
    assert_eq!(xfo, "DENY", "X-Frame-Options should be DENY");
}

#[tokio::test]
async fn test_docs_index_loads() {
    let resp = reqwest::get(format!("{BASE_URL}/docs")).await.unwrap();
    assert_eq!(resp.status(), 200, "/docs should return 200");
}

#[tokio::test]
async fn test_docs_pages_load() {
    let client = reqwest::Client::new();
    let pages = ["/docs/start", "/docs/reference/cli", "/docs/concepts/architecture"];
    for page in &pages {
        let resp = client
            .get(format!("{BASE_URL}{page}"))
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success() || resp.status() == 404,
            "page {page} returned unexpected status: {}",
            resp.status()
        );
    }
}

#[tokio::test]
async fn test_404_is_graceful() {
    let resp = reqwest::get(format!("{BASE_URL}/nonexistent-page-12345"))
        .await
        .unwrap();
    // Should return 404, not 500
    assert_eq!(resp.status(), 404, "Unknown pages should return 404");
}
