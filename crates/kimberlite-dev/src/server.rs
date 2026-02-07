//! Development server process management.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread::JoinHandle;

/// Development server supervisor.
///
/// Manages the Kimberlite database server running on a dedicated thread
/// (mio-based event loop is synchronous).
pub struct DevServer {
    /// Server thread handle.
    server_thread: Option<JoinHandle<()>>,
    /// Shutdown handle for requesting graceful stop.
    shutdown_handle: Option<kimberlite_server::ShutdownHandle>,
    /// The Kimberlite instance (shared with the server).
    db: Option<kimberlite::Kimberlite>,
}

impl DevServer {
    /// Create a new dev server.
    pub fn new() -> Self {
        Self {
            server_thread: None,
            shutdown_handle: None,
            db: None,
        }
    }

    /// Start the database server on a dedicated thread.
    ///
    /// Creates a Kimberlite instance and spawns the mio-based server.
    pub async fn start(&mut self, data_dir: PathBuf, bind_addr: SocketAddr) -> Result<()> {
        // Create the Kimberlite database instance
        let db = kimberlite::Kimberlite::open(&data_dir)
            .with_context(|| format!("Failed to open database at {}", data_dir.display()))?;

        self.db = Some(db.clone());

        // Build server config
        let config = kimberlite_server::ServerConfig::new(bind_addr, data_dir);

        // Create the server to get a shutdown handle before spawning
        let mut server = kimberlite_server::Server::new(config, db)
            .map_err(|e| anyhow::anyhow!("Failed to create server: {e}"))?;

        self.shutdown_handle = Some(server.shutdown_handle());

        // Spawn the mio-based server on a dedicated thread
        let handle = std::thread::Builder::new()
            .name("kimberlite-server".to_string())
            .spawn(move || {
                if let Err(e) = server.run_with_shutdown() {
                    tracing::error!("Server error: {e}");
                }
            })
            .context("Failed to spawn server thread")?;

        self.server_thread = Some(handle);
        Ok(())
    }

    /// Returns a reference to the Kimberlite instance if the server is running.
    pub fn kimberlite(&self) -> Option<&kimberlite::Kimberlite> {
        self.db.as_ref()
    }

    /// Stop all services gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        // Signal shutdown via the handle
        if let Some(ref handle) = self.shutdown_handle {
            handle.shutdown();
        }

        // Wait for server thread to finish
        if let Some(handle) = self.server_thread.take() {
            handle.join().ok();
        }

        // Sync database before exit
        if let Some(ref db) = self.db {
            db.sync().ok(); // Best-effort sync
        }

        Ok(())
    }
}

impl Default for DevServer {
    fn default() -> Self {
        Self::new()
    }
}
