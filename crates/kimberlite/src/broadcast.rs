//! Projection event broadcasting for real-time Studio UI updates.

use kimberlite_types::{Offset, TenantId};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Events emitted when projections are updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProjectionEvent {
    /// A table's projection was updated with new data.
    TableUpdated {
        tenant_id: TenantId,
        table_id: u64,
        from_offset: Offset,
        to_offset: Offset,
    },
    /// A new table was created.
    TableCreated {
        tenant_id: TenantId,
        table_id: u64,
        name: String,
    },
    /// A table was dropped.
    TableDropped { tenant_id: TenantId, table_id: u64 },
    /// An index was created on a table.
    IndexCreated {
        tenant_id: TenantId,
        table_id: u64,
        index_id: u64,
        name: String,
    },
}

/// Broadcasts projection events to connected Studio UI clients.
#[derive(Debug, Clone)]
pub struct ProjectionBroadcast {
    tx: broadcast::Sender<ProjectionEvent>,
}

impl ProjectionBroadcast {
    /// Creates a new projection broadcaster with the given buffer size.
    ///
    /// # Arguments
    /// * `buffer_size` - Number of events to buffer for slow consumers (default: 1024)
    pub fn new(buffer_size: usize) -> Self {
        let (tx, _rx) = broadcast::channel(buffer_size);
        Self { tx }
    }

    /// Sends a projection event to all subscribers.
    ///
    /// Returns the number of active subscribers who received the event.
    /// Slow subscribers who fall behind will receive a `RecvError::Lagged` error.
    pub fn send(&self, event: ProjectionEvent) -> usize {
        self.tx.send(event).unwrap_or_default()
    }

    /// Subscribes to projection events.
    ///
    /// Returns a receiver that will receive all future events.
    /// Events sent before subscription are not included.
    pub fn subscribe(&self) -> broadcast::Receiver<ProjectionEvent> {
        self.tx.subscribe()
    }

    /// Returns the number of active subscribers.
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for ProjectionBroadcast {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_basic() {
        let broadcast = ProjectionBroadcast::new(16);
        let mut rx = broadcast.subscribe();

        let event = ProjectionEvent::TableCreated {
            tenant_id: TenantId::from(1),
            table_id: 10,
            name: "patients".to_string(),
        };

        broadcast.send(event.clone());

        let received = rx.try_recv().expect("should receive event");
        matches!(received, ProjectionEvent::TableCreated { .. });
    }

    #[test]
    fn test_multiple_subscribers() {
        let broadcast = ProjectionBroadcast::new(16);
        let mut rx1 = broadcast.subscribe();
        let mut rx2 = broadcast.subscribe();

        assert_eq!(broadcast.receiver_count(), 2);

        let event = ProjectionEvent::TableUpdated {
            tenant_id: TenantId::from(1),
            table_id: 10,
            from_offset: Offset::from(0),
            to_offset: Offset::from(5),
        };

        let sent_count = broadcast.send(event);
        assert_eq!(sent_count, 2);

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn test_lagging_subscriber() {
        let broadcast = ProjectionBroadcast::new(2); // Small buffer
        let mut rx = broadcast.subscribe();

        // Send more events than the buffer can hold
        for i in 0..5 {
            broadcast.send(ProjectionEvent::TableUpdated {
                tenant_id: TenantId::from(1),
                table_id: 10,
                from_offset: Offset::from(i),
                to_offset: Offset::from(i + 1),
            });
        }

        // First receive should indicate lag
        match rx.try_recv() {
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                assert!(n > 0, "should have lagged messages");
            }
            other => panic!("expected lagged error, got {other:?}"),
        }
    }
}
