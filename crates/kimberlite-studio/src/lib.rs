//! Kimberlite Studio - Web UI for database exploration and queries.

use anyhow::Result;
use axum::{Router, http::StatusCode, response::Html, routing::get};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

mod assets;
pub mod routes;
pub mod state;
pub mod templates;

// Re-export broadcast from kimberlite crate
pub use kimberlite::broadcast;

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
///
/// # Arguments
/// * `config` - Server configuration
/// * `projection_broadcast` - Optional broadcast channel for projection events
pub async fn run_studio(
    config: StudioConfig,
    projection_broadcast: Option<std::sync::Arc<broadcast::ProjectionBroadcast>>,
) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    info!("Starting Studio on http://{}", addr);

    // Create shared state
    let broadcast = projection_broadcast.unwrap_or_else(|| {
        std::sync::Arc::new(broadcast::ProjectionBroadcast::default())
    });

    let state = state::StudioState::new(
        broadcast,
        config.db_address.clone(),
        config.default_tenant,
        config.port,
    );

    let app = Router::new()
        // Main UI
        .route("/", get(serve_index))
        // Static assets
        .route("/css/*path", get(routes::assets::serve_css))
        .route("/fonts/*path", get(routes::assets::serve_font))
        .route("/vendor/*path", get(routes::assets::serve_vendor))
        .route("/icons/sustyicons.svg", get(routes::assets::serve_icons))
        // API endpoints
        .route("/api/query", axum::routing::post(routes::api::execute_query))
        .route("/api/select-tenant", axum::routing::post(routes::api::select_tenant))
        // SSE endpoints
        .route("/sse/projection-updates", get(routes::sse::projection_updates))
        .route("/sse/query-results", get(routes::sse::query_results))
        // Fallback
        .fallback(|| async { (StatusCode::NOT_FOUND, "Not found") })
        // Attach shared state
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    info!("Studio ready on http://{}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_index() -> Html<&'static str> {
    Html(assets::INDEX_HTML)
}
