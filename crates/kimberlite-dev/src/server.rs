//! Development server process management.

use anyhow::Result;

/// Development server supervisor.
pub struct DevServer {
    // TODO(v0.5.0): Add fields for managing server processes
}

impl DevServer {
    /// Create a new dev server.
    pub fn new() -> Self {
        Self {}
    }

    /// Start all services.
    pub async fn start(&mut self) -> Result<()> {
        // TODO(v0.5.0): Start database server
        // TODO(v0.5.0): Start Studio if enabled
        Ok(())
    }

    /// Stop all services gracefully.
    pub async fn stop(&mut self) -> Result<()> {
        // TODO(v0.5.0): Stop services
        Ok(())
    }
}

impl Default for DevServer {
    fn default() -> Self {
        Self::new()
    }
}
