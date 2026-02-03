//! Dashboard router and server implementation.

use crate::coverage_fuzzer::CoverageFuzzer;
use axum::{Router, routing::get};
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;

// ============================================================================
// Dashboard State
// ============================================================================

/// Shared state for the dashboard server.
#[derive(Clone)]
pub struct DashboardState {
    /// Coverage-guided fuzzer (contains coverage tracker).
    pub fuzzer: Arc<Mutex<CoverageFuzzer>>,
}

impl DashboardState {
    /// Creates a new dashboard state.
    pub fn new(fuzzer: Arc<Mutex<CoverageFuzzer>>) -> Self {
        Self { fuzzer }
    }
}

// ============================================================================
// Dashboard Server
// ============================================================================

/// Web server for the coverage dashboard.
pub struct DashboardServer {
    /// Server state.
    state: DashboardState,
    /// Server port.
    port: u16,
}

impl DashboardServer {
    /// Creates a new dashboard server.
    pub fn new(fuzzer: Arc<Mutex<CoverageFuzzer>>) -> Self {
        Self {
            state: DashboardState::new(fuzzer),
            port: 8080,
        }
    }

    /// Sets the server port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Creates the router with all routes.
    pub fn create_router(&self) -> Router {
        // Serve static files from website/public if it exists
        let static_dir = std::path::PathBuf::from("website/public");
        let static_service = if static_dir.exists() {
            ServeDir::new(static_dir)
        } else {
            // Fallback to embedded static files or minimal serving
            ServeDir::new("public")
        };

        Router::new()
            .route("/", get(super::handlers::dashboard))
            .route("/vopr/updates", get(super::handlers::coverage_updates_sse))
            .nest_service("/public", static_service)
            .with_state(self.state.clone())
    }

    /// Runs the dashboard server.
    ///
    /// This is an async function that starts the web server and blocks
    /// until the server is shut down.
    pub async fn run(self) -> Result<(), std::io::Error> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        let router = self.create_router();

        println!("VOPR Coverage Dashboard starting on http://{}", addr);
        println!("Press Ctrl+C to stop");

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage_fuzzer::SelectionStrategy;

    #[test]
    fn dashboard_state_creation() {
        let fuzzer = Arc::new(Mutex::new(CoverageFuzzer::new(
            SelectionStrategy::EnergyBased,
        )));

        let state = DashboardState::new(fuzzer.clone());
        assert_eq!(Arc::strong_count(&state.fuzzer), 2); // state + fuzzer variable
    }

    #[test]
    fn dashboard_server_creation() {
        let fuzzer = Arc::new(Mutex::new(CoverageFuzzer::new(
            SelectionStrategy::EnergyBased,
        )));

        let server = DashboardServer::new(fuzzer);
        assert_eq!(server.port, 8080);
    }

    #[test]
    fn dashboard_server_with_custom_port() {
        let fuzzer = Arc::new(Mutex::new(CoverageFuzzer::new(
            SelectionStrategy::EnergyBased,
        )));

        let server = DashboardServer::new(fuzzer).with_port(9090);
        assert_eq!(server.port, 9090);
    }
}
