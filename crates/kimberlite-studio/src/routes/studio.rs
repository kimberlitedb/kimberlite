//! Studio query endpoint — Datastar SSE-driven query execution.
//!
//! One SSE endpoint:
//! - `POST /studio/query` — Execute a SQL query for the selected tenant

use axum::{
    extract::State,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use datastar::{
    axum::ReadSignals,
    prelude::{ElementPatchMode, PatchElements, PatchSignals},
};
use serde::Deserialize;
use std::convert::Infallible;
use std::time::Duration;

use crate::state::StudioState;
use crate::templates;

/// Row limit for Studio queries.
const MAX_ROWS: usize = 1000;
/// Query execution timeout.
const QUERY_TIMEOUT_SECS: u64 = 10;

/// Signals sent by the Studio UI when the user clicks Execute.
#[derive(Debug, Deserialize)]
pub struct StudioQuerySignals {
    /// Selected tenant ID. Can be a JSON number, a numeric string, or null.
    pub tenant_id: serde_json::Value,
    /// SQL query text.
    pub query: String,
    /// Optional log offset for time-travel queries.
    #[serde(default)]
    pub offset: Option<serde_json::Value>,
}

/// Execute a SQL query for the Studio, streaming results via Datastar SSE.
///
/// POST /studio/query
pub async fn execute_query(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<StudioQuerySignals>,
) -> impl IntoResponse {
    let query = signals.query.clone();
    let db_address = state.db_address.clone();

    // Parse tenant_id from signal (may be a number, a numeric string, or null)
    let tenant_id_result = parse_tenant_id(&signals.tenant_id);

    let stream = async_stream::stream! {
        // Signal loading state
        let patch = PatchSignals::new(r#"{"loading": true, "error": null}"#);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        // Validate tenant
        let tenant_id = match tenant_id_result {
            Some(id) => id,
            None => {
                let patch = PatchSignals::new(r#"{"loading": false, "error": "Please select a tenant before executing a query"}"#);
                yield Ok(patch.write_as_axum_sse_event());
                return;
            }
        };

        // Validate query is non-empty
        let query_trimmed = query.trim().to_string();
        if query_trimmed.is_empty() {
            let patch = PatchSignals::new(r#"{"loading": false, "error": "Query cannot be empty"}"#);
            yield Ok(patch.write_as_axum_sse_event());
            return;
        }

        // Execute query with timeout
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(QUERY_TIMEOUT_SECS),
            tokio::task::spawn_blocking(move || {
                use kimberlite_client::{Client, ClientConfig};
                use kimberlite_types::TenantId;

                let config = ClientConfig::default();
                let mut client = Client::connect(&db_address, TenantId::new(tenant_id), config)?;
                client.query(&query_trimmed, &[])
            }),
        )
        .await;

        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(Ok(query_response))) => {
                let columns: Vec<String> = query_response
                    .columns
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                let mut rows: Vec<Vec<String>> = query_response
                    .rows
                    .iter()
                    .map(|row| row.iter().map(format_value).collect())
                    .collect();

                let total_rows = rows.len();
                let truncated = total_rows > MAX_ROWS;
                if truncated {
                    rows.truncate(MAX_ROWS);
                }

                let mut html = templates::render_query_results(&columns, &rows);
                if truncated {
                    html.push_str(&format!(
                        "<div class=\"results-table__meta\" style=\"color: var(--text-warning);\">Results truncated: showing {MAX_ROWS} of {total_rows} rows</div>",
                    ));
                }

                let patch = PatchElements::new(html)
                    .selector("#results-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let row_count = if truncated { MAX_ROWS } else { total_rows };
                let patch = PatchSignals::new(format!(
                    r#"{{"loading": false, "error": null, "execution_time_ms": {elapsed_ms}, "row_count": {row_count}}}"#,
                ));
                yield Ok(patch.write_as_axum_sse_event());

                tracing::info!(
                    tenant_id,
                    elapsed_ms,
                    row_count = total_rows,
                    "Studio query executed"
                );
            }
            Ok(Ok(Err(e))) => {
                let error_msg = e.to_string();
                let html = templates::render_error("Query Error", &error_msg);
                let patch = PatchElements::new(html)
                    .selector("#results-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let escaped = serde_json::to_string(&error_msg).unwrap_or_default();
                let patch = PatchSignals::new(format!(r#"{{"loading": false, "error": {escaped}, "row_count": 0}}"#));
                yield Ok(patch.write_as_axum_sse_event());
            }
            Ok(Err(e)) => {
                tracing::error!(error = %e, "Studio query task panicked");
                let html = templates::render_error("Internal Error", "Query execution failed unexpectedly");
                let patch = PatchElements::new(html)
                    .selector("#results-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let patch = PatchSignals::new(r#"{"loading": false, "error": "Internal error", "row_count": 0}"#);
                yield Ok(patch.write_as_axum_sse_event());
            }
            Err(_timeout) => {
                let html = templates::render_error(
                    "Timeout",
                    &format!("Query exceeded the {QUERY_TIMEOUT_SECS}-second time limit"),
                );
                let patch = PatchElements::new(html)
                    .selector("#results-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let patch = PatchSignals::new(r#"{"loading": false, "error": "Query timed out", "row_count": 0}"#);
                yield Ok(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream)
}

/// Parses a tenant_id signal value (number, numeric string, or null) to a `u64`.
///
/// Returns `None` if the value is null, empty, or unparseable.
fn parse_tenant_id(val: &serde_json::Value) -> Option<u64> {
    match val {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<u64>().ok()
            }
        }
        _ => None,
    }
}

/// Formats a wire QueryValue to a display string.
fn format_value(val: &kimberlite_client::QueryValue) -> String {
    match val {
        kimberlite_client::QueryValue::Null => "NULL".to_string(),
        kimberlite_client::QueryValue::BigInt(i) => i.to_string(),
        kimberlite_client::QueryValue::Text(s) => s.clone(),
        kimberlite_client::QueryValue::Boolean(b) => b.to_string(),
        kimberlite_client::QueryValue::Timestamp(ts) => ts.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tenant_id_number() {
        assert_eq!(parse_tenant_id(&serde_json::json!(1)), Some(1));
        assert_eq!(parse_tenant_id(&serde_json::json!(42)), Some(42));
    }

    #[test]
    fn test_parse_tenant_id_string() {
        assert_eq!(parse_tenant_id(&serde_json::json!("1")), Some(1));
        assert_eq!(parse_tenant_id(&serde_json::json!("42")), Some(42));
        assert_eq!(parse_tenant_id(&serde_json::json!("")), None);
    }

    #[test]
    fn test_parse_tenant_id_null() {
        assert_eq!(parse_tenant_id(&serde_json::Value::Null), None);
    }
}
