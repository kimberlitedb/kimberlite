//! Shared state for Studio server.

use crate::broadcast::ProjectionBroadcast;
use std::sync::Arc;

/// Shared state for all Studio HTTP handlers.
#[derive(Debug, Clone)]
pub struct StudioState {
    /// Broadcast channel for projection events
    pub projection_broadcast: Arc<ProjectionBroadcast>,

    /// Database connection string (for kimberlite-client)
    pub db_address: String,

    /// Default tenant ID (optional, for convenience)
    pub default_tenant: Option<u64>,

    /// Port the Studio server is running on
    pub port: u16,
}

impl StudioState {
    /// Creates new Studio state.
    pub fn new(
        projection_broadcast: Arc<ProjectionBroadcast>,
        db_address: String,
        default_tenant: Option<u64>,
        port: u16,
    ) -> Self {
        Self {
            projection_broadcast,
            db_address,
            default_tenant,
            port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_studio_state_creation() {
        let broadcast = Arc::new(ProjectionBroadcast::default());
        let state = StudioState::new(
            broadcast.clone(),
            "127.0.0.1:5432".to_string(),
            Some(1),
            5555,
        );

        assert_eq!(state.db_address, "127.0.0.1:5432");
        assert_eq!(state.default_tenant, Some(1));
        assert_eq!(state.port, 5555);
        assert_eq!(
            Arc::strong_count(&state.projection_broadcast),
            2
        ); // state + broadcast
    }

    #[test]
    fn test_studio_state_clone() {
        let broadcast = Arc::new(ProjectionBroadcast::default());
        let state1 = StudioState::new(
            broadcast.clone(),
            "127.0.0.1:5432".to_string(),
            None,
            5555,
        );

        let state2 = state1.clone();

        assert_eq!(state1.db_address, state2.db_address);
        assert_eq!(state1.default_tenant, state2.default_tenant);
        assert_eq!(state1.port, state2.port);
        assert_eq!(
            Arc::strong_count(&state1.projection_broadcast),
            3
        ); // broadcast + state1 + state2
    }
}
