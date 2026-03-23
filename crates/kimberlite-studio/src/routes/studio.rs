//! Studio endpoints — Datastar SSE-driven query execution and schema discovery.
//!
//! SSE endpoints:
//! - `POST /studio/init` — Auto-load default tenant schema on page open
//! - `POST /studio/select-tenant` — Load schema when tenant changes
//! - `POST /studio/query` — Execute a SQL query for the selected tenant
//! - `POST /studio/browse` — Browse table data with pagination

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

/// Signals sent by the Studio UI on tenant change.
#[derive(Debug, Deserialize)]
pub struct TenantSignals {
    pub tenant_id: serde_json::Value,
}

/// Signals sent by the Studio UI when browsing a table.
#[derive(Debug, Deserialize)]
pub struct BrowseSignals {
    pub tenant_id: serde_json::Value,
    pub active_table: Option<String>,
    #[serde(default)]
    pub browse_page: u64,
    #[serde(default = "default_page_size")]
    pub browse_page_size: u64,
    pub sort_column: Option<String>,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
}

fn default_page_size() -> u64 {
    50
}
fn default_sort_dir() -> String {
    "ASC".to_string()
}

/// Initialize Studio on page load — auto-select default tenant and load schema.
///
/// POST /studio/init
pub async fn init(State(state): State<StudioState>) -> impl IntoResponse {
    let default_tenant = state.default_tenant;
    let db_address = state.db_address.clone();

    let stream = async_stream::stream! {
        if let Some(tenant_id) = default_tenant {
            // Set tenant_id signal
            let patch = PatchSignals::new(format!(r#"{{"tenant_id": {tenant_id}}}"#));
            yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

            // Populate tenant dropdown
            let html = format!(
                r#"<option value="">Select tenant...</option><option value="{tenant_id}" selected>tenant-{tenant_id} (ID: {tenant_id})</option>"#,
            );
            let patch = PatchElements::new(html)
                .selector(".tenant-selector__select")
                .mode(ElementPatchMode::Inner);
            yield Ok(patch.write_as_axum_sse_event());

            // Load schema
            let schema = discover_schema(&db_address, tenant_id).await;
            let tree_html = templates::render_schema_tree(tenant_id, &format!("tenant-{tenant_id}"), &schema);
            let patch = PatchElements::new(tree_html)
                .selector("#schema-tree")
                .mode(ElementPatchMode::Inner);
            yield Ok(patch.write_as_axum_sse_event());

            tracing::info!(tenant_id, tables = schema.len(), "Studio init: loaded schema");
        }
    };

    Sse::new(stream)
}

/// Load schema when tenant selection changes.
///
/// POST /studio/select-tenant
pub async fn select_tenant(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<TenantSignals>,
) -> impl IntoResponse {
    let db_address = state.db_address.clone();
    let tenant_id_result = parse_tenant_id(&signals.tenant_id);

    let stream = async_stream::stream! {
        let tenant_id = match tenant_id_result {
            Some(id) if id > 0 => id,
            _ => {
                // Clear schema tree
                let html = r#"<div class="schema-tree"><div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">Select a tenant to view schema</div></div>"#;
                let patch = PatchElements::new(html)
                    .selector("#schema-tree")
                    .mode(ElementPatchMode::Inner);
                yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

                // Clear browse container
                let patch = PatchElements::new("")
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let patch = PatchSignals::new(r#"{"active_table": null, "total_rows": 0}"#);
                yield Ok(patch.write_as_axum_sse_event());
                return;
            }
        };

        // Load schema
        let schema = discover_schema(&db_address, tenant_id).await;
        let tree_html = templates::render_schema_tree(tenant_id, &format!("tenant-{tenant_id}"), &schema);
        let patch = PatchElements::new(tree_html)
            .selector("#schema-tree")
            .mode(ElementPatchMode::Inner);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        // Clear browse state
        let patch = PatchElements::new("")
            .selector("#browse-container")
            .mode(ElementPatchMode::Inner);
        yield Ok(patch.write_as_axum_sse_event());

        let patch = PatchSignals::new(r#"{"active_table": null, "total_rows": 0}"#);
        yield Ok(patch.write_as_axum_sse_event());

        tracing::info!(tenant_id, tables = schema.len(), "Tenant selected, schema loaded");
    };

    Sse::new(stream)
}

/// Browse table data with pagination.
///
/// POST /studio/browse
pub async fn browse_table(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<BrowseSignals>,
) -> impl IntoResponse {
    let db_address = state.db_address.clone();
    let tenant_id_result = parse_tenant_id(&signals.tenant_id);
    let table = signals.active_table.clone();
    let page = signals.browse_page;
    let page_size = signals.browse_page_size.clamp(1, 500);
    let sort_column = signals.sort_column.clone();
    let sort_dir = if signals.sort_dir.eq_ignore_ascii_case("DESC") {
        "DESC"
    } else {
        "ASC"
    };

    let stream = async_stream::stream! {
        let tenant_id = match tenant_id_result {
            Some(id) if id > 0 => id,
            _ => return,
        };

        let table_name = match table {
            Some(ref t) if !t.is_empty() => t.clone(),
            _ => return,
        };

        // Show loading skeleton
        let skeleton = templates::render_skeleton_table();
        let patch = PatchElements::new(skeleton)
            .selector("#browse-container")
            .mode(ElementPatchMode::Inner);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        let db = db_address.clone();
        let tbl = table_name.clone();
        let offset = page * page_size;
        let sort_col = sort_column.clone();
        let sort_col_outer = sort_column.clone();
        let sort_d = sort_dir.to_string();
        let ps = page_size;

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || {
                use kimberlite_client::{Client, ClientConfig};
                use kimberlite_types::TenantId;

                let config = ClientConfig::default();
                let mut client = Client::connect(&db, TenantId::new(tenant_id), config)?;

                // Get total row count
                let count_result = client.query(&format!("SELECT COUNT(*) FROM {tbl}"), &[])?;
                let total: u64 = count_result
                    .rows
                    .first()
                    .and_then(|r| r.first())
                    .map(|v| match v {
                        kimberlite_client::QueryValue::BigInt(n) => *n as u64,
                        _ => 0,
                    })
                    .unwrap_or(0);

                // Build query with optional sort
                let order_clause = if let Some(ref col) = sort_col {
                    format!(" ORDER BY {col} {sort_d}")
                } else {
                    String::new()
                };

                let query = format!("SELECT * FROM {tbl}{order_clause} LIMIT {ps} OFFSET {offset}");
                let data_result = client.query(&query, &[])?;

                let columns: Vec<String> = data_result
                    .columns
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                let rows: Vec<Vec<String>> = data_result
                    .rows
                    .iter()
                    .map(|row| row.iter().map(format_value).collect())
                    .collect();

                Ok::<_, anyhow::Error>((columns, rows, total))
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok((columns, rows, total)))) => {
                let mut html = templates::render_browse_results(&table_name, &columns, &rows, sort_col_outer.as_deref(), sort_dir);
                html.push_str(&templates::render_pagination(page, total, page_size));

                let patch = PatchElements::new(html)
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());

                let patch = PatchSignals::new(format!(
                    r#"{{"total_rows": {total}, "row_count": {}, "execution_time_ms": 0}}"#,
                    rows.len()
                ));
                yield Ok(patch.write_as_axum_sse_event());
            }
            Ok(Ok(Err(e))) => {
                let html = templates::render_error("Browse Error", &e.to_string());
                let patch = PatchElements::new(html)
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
            _ => {
                let html = templates::render_error("Error", "Failed to load table data");
                let patch = PatchElements::new(html)
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream)
}

/// Discover schema (tables + columns) for a tenant.
async fn discover_schema(db_address: &str, tenant_id: u64) -> Vec<(String, Vec<String>)> {
    let db = db_address.to_string();

    tokio::task::spawn_blocking(move || {
        use kimberlite_client::{Client, ClientConfig};
        use kimberlite_types::TenantId;

        let config = ClientConfig::default();
        let mut client = match Client::connect(&db, TenantId::new(tenant_id), config) {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let table_names: Vec<String> = match client.query("SHOW TABLES", &[]) {
            Ok(resp) => resp
                .rows
                .iter()
                .filter_map(|row| row.first().map(format_value))
                .collect(),
            Err(_) => return vec![],
        };

        table_names
            .into_iter()
            .map(|name| {
                let columns = match client.query(&format!("SHOW COLUMNS FROM {name}"), &[]) {
                    Ok(resp) => resp
                        .rows
                        .iter()
                        .filter_map(|row| {
                            let col_name = row.first().map(format_value)?;
                            let col_type = row.get(1).map(format_value).unwrap_or_default();
                            Some(format!("{col_name} ({col_type})"))
                        })
                        .collect(),
                    Err(_) => vec![],
                };
                (name, columns)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
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
