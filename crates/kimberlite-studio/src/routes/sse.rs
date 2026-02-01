//! Server-Sent Events (SSE) endpoints for real-time UI updates.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;

use crate::{broadcast::ProjectionEvent, state::StudioState, templates};

/// Streams projection update events to the UI.
///
/// GET /sse/projection-updates
///
/// This endpoint:
/// 1. Sends initial schema tree HTML
/// 2. Streams projection events (table created/updated/dropped)
/// 3. Updates max_offset signal when tables are updated
pub async fn projection_updates(
    State(state): State<StudioState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.projection_broadcast.subscribe();

    let stream = async_stream::stream! {
        // Send initial schema tree (empty until tenant selected)
        let initial_html = templates::render_schema_tree(
            0,
            "No tenant selected",
            &[],
        );

        let event = Event::default()
            .event("schema-update")
            .data(initial_html);

        yield Ok(event);

        // Stream projection events
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            match event {
                                ProjectionEvent::TableCreated { tenant_id, table_id, name } => {
                                    tracing::debug!(
                                        ?tenant_id,
                                        ?table_id,
                                        %name,
                                        "Table created event"
                                    );

                                    // TODO: Re-render schema tree with new table
                                    let data = format!(
                                        r#"{{"type":"table_created","tenant_id":{},"table_id":{},"name":"{}"}}"#,
                                        u64::from(tenant_id),
                                        table_id,
                                        name
                                    );

                                    let event = Event::default()
                                        .event("projection-event")
                                        .data(data);

                                    yield Ok(event);
                                }
                                ProjectionEvent::TableUpdated { tenant_id, table_id, from_offset, to_offset } => {
                                    tracing::debug!(
                                        ?tenant_id,
                                        ?table_id,
                                        ?from_offset,
                                        ?to_offset,
                                        "Table updated event"
                                    );

                                    // Update max_offset signal
                                    let data = format!(
                                        r#"{{"type":"table_updated","max_offset":{}}}"#,
                                        to_offset.as_u64()
                                    );

                                    let event = Event::default()
                                        .event("projection-event")
                                        .data(data);

                                    yield Ok(event);
                                }
                                ProjectionEvent::TableDropped { tenant_id, table_id } => {
                                    tracing::debug!(
                                        ?tenant_id,
                                        ?table_id,
                                        "Table dropped event"
                                    );

                                    let data = format!(
                                        r#"{{"type":"table_dropped","tenant_id":{},"table_id":{}}}"#,
                                        u64::from(tenant_id),
                                        table_id
                                    );

                                    let event = Event::default()
                                        .event("projection-event")
                                        .data(data);

                                    yield Ok(event);
                                }
                                ProjectionEvent::IndexCreated { tenant_id, table_id, index_id, name } => {
                                    tracing::debug!(
                                        ?tenant_id,
                                        ?table_id,
                                        ?index_id,
                                        %name,
                                        "Index created event"
                                    );

                                    let data = format!(
                                        r#"{{"type":"index_created","tenant_id":{},"table_id":{},"index_id":{},"name":"{}"}}"#,
                                        u64::from(tenant_id),
                                        table_id,
                                        index_id,
                                        name
                                    );

                                    let event = Event::default()
                                        .event("projection-event")
                                        .data(data);

                                    yield Ok(event);
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                lagged_count = n,
                                "SSE client lagged behind broadcast"
                            );
                            // Client will catch up on next event
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::info!("Broadcast channel closed, ending SSE stream");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    // Keep-alive ping (handled by KeepAlive below)
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// Streams query results to the UI.
///
/// GET /sse/query-results?tenant_id=1&query=SELECT...&offset=0
///
/// This endpoint:
/// 1. Sets loading signal to true
/// 2. Executes the query
/// 3. Renders results as HTML
/// 4. Sends HTML via SSE to update DOM
/// 5. Sets loading signal to false
pub async fn query_results(
    State(_state): State<StudioState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        // Set loading signal
        let event = Event::default()
            .event("signal-update")
            .data(r#"{"loading":true,"error":null}"#);
        yield Ok(event);

        // Simulate query execution
        tokio::time::sleep(Duration::from_millis(200)).await;

        // TODO: Execute actual query via kimberlite_client
        // For now, return mock data
        let columns = vec!["id".to_string(), "name".to_string(), "created_at".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string(), "2024-01-01".to_string()],
            vec!["2".to_string(), "Bob".to_string(), "2024-01-02".to_string()],
            vec!["3".to_string(), "Charlie".to_string(), "2024-01-03".to_string()],
        ];

        let html = templates::render_query_results(&columns, &rows);

        // Send results HTML
        let event = Event::default()
            .event("results-update")
            .data(html);
        yield Ok(event);

        // Clear loading signal
        let event = Event::default()
            .event("signal-update")
            .data(r#"{"loading":false}"#);
        yield Ok(event);
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(5)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::ProjectionBroadcast;
    use std::sync::Arc;

    fn mock_state() -> StudioState {
        StudioState::new(
            Arc::new(ProjectionBroadcast::default()),
            "127.0.0.1:5432".to_string(),
            Some(1),
            5555,
        )
    }

    #[tokio::test]
    async fn test_projection_updates_stream() {
        let state = mock_state();
        let sse = projection_updates(State(state)).await;

        // Stream should be created successfully
        // Actual streaming tested via integration tests
        drop(sse);
    }

    #[tokio::test]
    async fn test_query_results_stream() {
        let state = mock_state();
        let sse = query_results(State(state)).await;

        // Stream should be created successfully
        drop(sse);
    }
}
