//! Kimberlite Studio - Web UI for database exploration and queries.

use anyhow::Result;
use axum::{
    http::StatusCode,
    response::Html,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

mod assets;

/// Studio server configuration.
#[derive(Debug, Clone)]
pub struct StudioConfig {
    pub port: u16,
    pub db_address: String,
    pub default_tenant: Option<u64>,
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self {
            port: 5555,
            db_address: "127.0.0.1:5432".to_string(),
            default_tenant: Some(1),
        }
    }
}

/// Start the Studio server.
pub async fn run_studio(config: StudioConfig) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    info!("Starting Studio on http://{}", addr);

    let app = Router::new()
        .route("/", get(serve_index))
        .fallback(|| async { (StatusCode::NOT_FOUND, "Not found") });

    let listener = TcpListener::bind(addr).await?;
    info!("Studio ready on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_index() -> Html<&'static str> {
    Html(assets::INDEX_HTML)
}
