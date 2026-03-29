//! Studio endpoints — Datastar SSE-driven query execution and schema discovery.
//!
//! SSE endpoints:
//! - `POST /studio/init` — Auto-load default tenant schema on page open
//! - `POST /studio/select-tenant` — Load schema when tenant changes
//! - `POST /studio/query` — Execute a SQL query for the selected tenant
//! - `POST /studio/browse` — Browse table data with pagination

use axum::{
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use datastar::{
    axum::ReadSignals,
    prelude::{ElementPatchMode, ExecuteScript, PatchElements, PatchSignals},
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

    // Parse offset from slider signal (may be number, string, or null)
    let slider_offset = signals.offset.as_ref().and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    });

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

        // Execute query with timeout.
        // Time-travel priority: SQL-inline AT OFFSET > slider offset > latest.
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(QUERY_TIMEOUT_SECS),
            tokio::task::spawn_blocking(move || {
                use kimberlite_client::{Client, ClientConfig};
                use kimberlite_query::extract_at_offset;
                use kimberlite_types::{Offset, TenantId};

                let config = ClientConfig::default();
                let mut client = Client::connect(&db_address, TenantId::new(tenant_id), config)?;

                // Check for SQL-inline AT OFFSET first
                let (cleaned_sql, at_offset) = extract_at_offset(&query_trimmed);
                if let Some(offset) = at_offset {
                    client.query_at(&cleaned_sql, &[], Offset::new(offset))
                } else if let Some(offset) = slider_offset {
                    // Use slider offset for time-travel
                    client.query_at(&query_trimmed, &[], Offset::new(offset))
                } else {
                    client.query(&query_trimmed, &[])
                }
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
        // Always clear loading/error state on init
        let patch = PatchSignals::new(r#"{"loading": false, "error": null}"#);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        if let Some(tenant_id) = default_tenant {
            // Set tenant_id signal
            let patch = PatchSignals::new(format!(r#"{{"tenant_id": {tenant_id}}}"#));
            yield Ok(patch.write_as_axum_sse_event());

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

            // Populate SQL completion with table names and columns
            let schema_js = schema_to_js_object(&schema);
            let script = ExecuteScript::new(format!(
                "window.STUDIO_SCHEMA = {schema_js}; window.STUDIO_TABLES = Object.keys(window.STUDIO_SCHEMA);"
            ));
            yield Ok(script.write_as_axum_sse_event());

            // Query max offset for time-travel slider
            let max_offset = query_max_offset(&db_address, tenant_id).await;
            if max_offset > 0 {
                let patch = PatchSignals::new(format!(r#"{{"max_offset": {max_offset}}}"#));
                yield Ok(patch.write_as_axum_sse_event());
            }

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

        // Populate SQL completion with table names and columns
        let schema_js = schema_to_js_object(&schema);
        let script = ExecuteScript::new(format!(
            "window.STUDIO_SCHEMA = {schema_js}; window.STUDIO_TABLES = Object.keys(window.STUDIO_SCHEMA);"
        ));
        yield Ok(script.write_as_axum_sse_event());

        // Query max offset for time-travel slider
        let max_offset = query_max_offset(&db_address, tenant_id).await;
        let patch = PatchSignals::new(format!(r#"{{"max_offset": {max_offset}}}"#));
        yield Ok(patch.write_as_axum_sse_event());

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

/// Converts a schema list to a JS object with table names as keys and column name arrays as values.
///
/// Example: `{"patients": ["id", "name", "dob"], "visits": ["id", "date"]}`
fn schema_to_js_object(schema: &[(String, Vec<String>)]) -> String {
    let entries: Vec<String> = schema
        .iter()
        .map(|(table_name, columns)| {
            let escaped_table = table_name.replace('\\', "\\\\").replace('"', "\\\"");
            let col_names: Vec<String> = columns
                .iter()
                .map(|col| {
                    // Parse "name (TYPE)" or "name (TYPE) [CLASS]" format to extract just the name
                    let col_name = if let Some(paren_pos) = col.find(" (") {
                        &col[..paren_pos]
                    } else {
                        col.as_str()
                    };
                    format!("\"{}\"", col_name.replace('\\', "\\\\").replace('"', "\\\""))
                })
                .collect();
            format!("\"{}\":[{}]", escaped_table, col_names.join(","))
        })
        .collect();
    format!("{{{}}}", entries.join(","))
}

/// Discover schema (tables + columns + classifications) for a tenant.
///
/// Returns `Vec<(table_name, Vec<"col_name (TYPE) [CLASSIFICATION]">)>`.
async fn discover_schema(db_address: &str, tenant_id: u64) -> Vec<(String, Vec<String>)> {
    let db = db_address.to_string();

    tokio::task::spawn_blocking(move || {
        use kimberlite_client::{Client, ClientConfig};
        use kimberlite_types::TenantId;
        use std::collections::HashMap;

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
                // Fetch column classifications for this table
                let classifications: HashMap<String, String> =
                    match client.query(&format!("SHOW CLASSIFICATIONS FOR {name}"), &[]) {
                        Ok(resp) => resp
                            .rows
                            .iter()
                            .filter_map(|row| {
                                let col = row.first().map(format_value)?;
                                let class = row.get(1).map(format_value)?;
                                Some((col, class))
                            })
                            .collect(),
                        Err(_) => HashMap::new(),
                    };

                let columns = match client.query(&format!("SHOW COLUMNS FROM {name}"), &[]) {
                    Ok(resp) => resp
                        .rows
                        .iter()
                        .filter_map(|row| {
                            let col_name = row.first().map(format_value)?;
                            let col_type = row.get(1).map(format_value).unwrap_or_default();
                            let class = classifications.get(&col_name);
                            if let Some(c) = class {
                                Some(format!("{col_name} ({col_type}) [{c}]"))
                            } else {
                                Some(format!("{col_name} ({col_type})"))
                            }
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

/// Query max offset for the time-travel slider.
///
/// Sums COUNT(*) across all tables as a proxy for total log offset.
async fn query_max_offset(db_address: &str, tenant_id: u64) -> u64 {
    let db = db_address.to_string();

    tokio::task::spawn_blocking(move || {
        use kimberlite_client::{Client, ClientConfig};
        use kimberlite_types::TenantId;

        let config = ClientConfig::default();
        let mut client = match Client::connect(&db, TenantId::new(tenant_id), config) {
            Ok(c) => c,
            Err(_) => return 0,
        };

        let table_names: Vec<String> = match client.query("SHOW TABLES", &[]) {
            Ok(resp) => resp
                .rows
                .iter()
                .filter_map(|row| row.first().map(format_value))
                .collect(),
            Err(_) => return 0,
        };

        let mut total: u64 = 0;
        for table in &table_names {
            if let Ok(resp) = client.query(&format!("SELECT COUNT(*) FROM {table}"), &[]) {
                if let Some(row) = resp.rows.first() {
                    if let Some(kimberlite_client::QueryValue::BigInt(n)) = row.first() {
                        total += *n as u64;
                    }
                }
            }
        }
        total
    })
    .await
    .unwrap_or(0)
}

/// Query parameters for the export endpoint.
#[derive(Debug, Deserialize)]
pub struct ExportParams {
    pub tenant_id: u64,
    pub query: String,
    #[serde(default = "default_export_format")]
    pub format: String,
}

fn default_export_format() -> String {
    "csv".to_string()
}

/// Export query results as CSV or JSON file download.
///
/// GET /studio/export?tenant_id=1&query=SELECT...&format=csv
pub async fn export(
    State(state): State<StudioState>,
    Query(params): Query<ExportParams>,
) -> Response {
    let db_address = state.db_address.clone();
    let tenant_id = params.tenant_id;
    let query = params.query.clone();
    let format = params.format.to_lowercase();

    if tenant_id == 0 {
        return (StatusCode::BAD_REQUEST, "Tenant ID is required").into_response();
    }
    if query.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "Query is required").into_response();
    }

    let result = tokio::time::timeout(
        Duration::from_secs(QUERY_TIMEOUT_SECS),
        tokio::task::spawn_blocking(move || {
            use kimberlite_client::{Client, ClientConfig};
            use kimberlite_types::TenantId;

            let config = ClientConfig::default();
            let mut client = Client::connect(&db_address, TenantId::new(tenant_id), config)?;
            client.query(&query, &[])
        }),
    )
    .await;

    match result {
        Ok(Ok(Ok(query_response))) => {
            let columns: Vec<String> = query_response
                .columns
                .iter()
                .map(|c| c.to_string())
                .collect();
            let rows: Vec<Vec<String>> = query_response
                .rows
                .iter()
                .map(|row| row.iter().map(format_value).collect())
                .collect();

            match format.as_str() {
                "json" => {
                    let json_rows: Vec<serde_json::Value> = rows
                        .iter()
                        .map(|row| {
                            let obj: serde_json::Map<String, serde_json::Value> = columns
                                .iter()
                                .zip(row.iter())
                                .map(|(col, val)| {
                                    (col.clone(), serde_json::Value::String(val.clone()))
                                })
                                .collect();
                            serde_json::Value::Object(obj)
                        })
                        .collect();

                    let body = serde_json::to_string_pretty(&json_rows).unwrap_or_default();

                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(
                            header::CONTENT_DISPOSITION,
                            HeaderValue::from_static("attachment; filename=\"export.json\""),
                        )
                        .body(body.into())
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                }
                _ => {
                    // CSV format
                    let mut csv = String::new();

                    // Header row
                    for (i, col) in columns.iter().enumerate() {
                        if i > 0 {
                            csv.push(',');
                        }
                        csv_escape_field(&mut csv, col);
                    }
                    csv.push('\n');

                    // Data rows
                    for row in &rows {
                        for (i, cell) in row.iter().enumerate() {
                            if i > 0 {
                                csv.push(',');
                            }
                            csv_escape_field(&mut csv, cell);
                        }
                        csv.push('\n');
                    }

                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
                        .header(
                            header::CONTENT_DISPOSITION,
                            HeaderValue::from_static("attachment; filename=\"export.csv\""),
                        )
                        .body(csv.into())
                        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                }
            }
        }
        Ok(Ok(Err(e))) => {
            (StatusCode::BAD_REQUEST, format!("Query error: {e}")).into_response()
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "Export failed").into_response(),
    }
}

/// Escape a field for CSV output (RFC 4180).
fn csv_escape_field(out: &mut String, field: &str) {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

/// Signals for the audit log viewer.
#[derive(Debug, Deserialize)]
pub struct AuditSignals {
    pub tenant_id: serde_json::Value,
    #[serde(default)]
    pub audit_action: Option<String>,
    #[serde(default)]
    pub audit_actor: Option<String>,
}

/// Query audit log for a tenant.
///
/// POST /studio/audit
pub async fn audit_log(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<AuditSignals>,
) -> impl IntoResponse {
    let db_address = state.db_address.clone();
    let tenant_id_result = parse_tenant_id(&signals.tenant_id);
    let action_filter = signals.audit_action.clone();
    let actor_filter = signals.audit_actor.clone();

    let stream = async_stream::stream! {
        let tenant_id = match tenant_id_result {
            Some(id) if id > 0 => id,
            _ => {
                let html = templates::render_error("Audit Log", "Please select a tenant to view audit events");
                let patch = PatchElements::new(html)
                    .selector("#audit-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());
                return;
            }
        };

        // Show loading
        let patch = PatchSignals::new(r#"{"loading": true}"#);
        yield Ok(patch.write_as_axum_sse_event());

        // Build a query that filters audit-like tables if they exist
        // We query through the SQL interface for portability
        let result = tokio::time::timeout(
            Duration::from_secs(QUERY_TIMEOUT_SECS),
            tokio::task::spawn_blocking(move || {
                use kimberlite_client::{Client, ClientConfig};
                use kimberlite_types::TenantId;

                let config = ClientConfig::default();
                let mut client = Client::connect(&db_address, TenantId::new(tenant_id), config)?;

                // Check if an audit_log table exists — if so, query it
                let tables = client.query("SHOW TABLES", &[])?;
                let has_audit = tables
                    .rows
                    .iter()
                    .any(|row| row.first().map(format_value).is_some_and(|n| n == "audit_log"));

                if has_audit {
                    let mut sql = String::from("SELECT * FROM audit_log");
                    let mut conditions = Vec::new();
                    if let Some(ref action) = action_filter {
                        if !action.is_empty() {
                            conditions.push(format!("action = '{}'", action.replace('\'', "''")));
                        }
                    }
                    if let Some(ref actor) = actor_filter {
                        if !actor.is_empty() {
                            conditions.push(format!("actor LIKE '%{}%'", actor.replace('\'', "''")));
                        }
                    }
                    if !conditions.is_empty() {
                        sql.push_str(" WHERE ");
                        sql.push_str(&conditions.join(" AND "));
                    }
                    sql.push_str(" ORDER BY timestamp DESC LIMIT 200");
                    client.query(&sql, &[])
                } else {
                    // Return a synthetic result showing that no audit table exists yet
                    Ok(kimberlite_client::QueryResponse {
                        columns: vec!["info".to_string()],
                        rows: vec![vec![kimberlite_client::QueryValue::Text(
                            "No audit_log table found. Audit events are tracked automatically when compliance features are enabled.".to_string(),
                        )]],
                    })
                }
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(query_response))) => {
                let columns: Vec<String> = query_response
                    .columns
                    .iter()
                    .map(|c| c.to_string())
                    .collect();
                let rows: Vec<Vec<String>> = query_response
                    .rows
                    .iter()
                    .map(|row| row.iter().map(format_value).collect())
                    .collect();

                let html = templates::render_audit_log(&columns, &rows);
                let patch = PatchElements::new(html)
                    .selector("#audit-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
            Ok(Ok(Err(e))) => {
                let html = templates::render_error("Audit Error", &e.to_string());
                let patch = PatchElements::new(html)
                    .selector("#audit-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
            _ => {
                let html = templates::render_error("Error", "Failed to load audit log");
                let patch = PatchElements::new(html)
                    .selector("#audit-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
        }

        let patch = PatchSignals::new(r#"{"loading": false}"#);
        yield Ok(patch.write_as_axum_sse_event());
    };

    Sse::new(stream)
}

/// Signals for browse table filtering.
#[derive(Debug, Deserialize)]
pub struct FilteredBrowseSignals {
    pub tenant_id: serde_json::Value,
    pub active_table: Option<String>,
    #[serde(default)]
    pub browse_page: u64,
    #[serde(default = "default_page_size")]
    pub browse_page_size: u64,
    pub sort_column: Option<String>,
    #[serde(default = "default_sort_dir")]
    pub sort_dir: String,
    #[serde(default)]
    pub filters: Vec<BrowseFilter>,
}

/// A single filter condition for the data browser.
#[derive(Debug, Deserialize, Clone)]
pub struct BrowseFilter {
    pub column: String,
    pub operator: String,
    pub value: String,
}

/// Browse table with advanced filtering.
///
/// POST /studio/browse-filtered
pub async fn browse_filtered(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<FilteredBrowseSignals>,
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
    let filters = signals.filters.clone();

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
        let filter_list = filters.clone();

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || {
                use kimberlite_client::{Client, ClientConfig};
                use kimberlite_types::TenantId;

                let config = ClientConfig::default();
                let mut client = Client::connect(&db, TenantId::new(tenant_id), config)?;

                // Build WHERE clause from filters
                let where_clause = build_where_clause(&filter_list);

                // Get total row count
                let count_sql = format!("SELECT COUNT(*) FROM {tbl}{where_clause}");
                let count_result = client.query(&count_sql, &[])?;
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

                let query = format!("SELECT * FROM {tbl}{where_clause}{order_clause} LIMIT {ps} OFFSET {offset}");
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
                    r#"{{"total_rows": {total}, "row_count": {}}}"#,
                    rows.len()
                ));
                yield Ok(patch.write_as_axum_sse_event());
            }
            Ok(Ok(Err(e))) => {
                let html = templates::render_error("Filter Error", &e.to_string());
                let patch = PatchElements::new(html)
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
            _ => {
                let html = templates::render_error("Error", "Failed to load filtered data");
                let patch = PatchElements::new(html)
                    .selector("#browse-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream)
}

/// Build a WHERE clause from filter conditions.
///
/// Sanitizes column names (alphanumeric + underscore only) and escapes values.
fn build_where_clause(filters: &[BrowseFilter]) -> String {
    let valid_filters: Vec<String> = filters
        .iter()
        .filter_map(|f| {
            // Validate column name (alphanumeric + underscore only)
            if f.column.is_empty() || !f.column.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }

            let op = match f.operator.as_str() {
                "=" | "!=" | ">" | "<" | ">=" | "<=" => f.operator.as_str(),
                "LIKE" | "like" => "LIKE",
                "IS NULL" | "is null" => return Some(format!("{} IS NULL", f.column)),
                "IS NOT NULL" | "is not null" => return Some(format!("{} IS NOT NULL", f.column)),
                _ => return None,
            };

            // Escape single quotes in value
            let escaped_value = f.value.replace('\'', "''");
            Some(format!("{} {} '{}'", f.column, op, escaped_value))
        })
        .collect();

    if valid_filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", valid_filters.join(" AND "))
    }
}

/// Load compliance status for a tenant.
///
/// POST /studio/compliance
pub async fn compliance_status(
    State(state): State<StudioState>,
    ReadSignals(signals): ReadSignals<TenantSignals>,
) -> impl IntoResponse {
    let db_address = state.db_address.clone();
    let tenant_id_result = parse_tenant_id(&signals.tenant_id);

    let stream = async_stream::stream! {
        let tenant_id = match tenant_id_result {
            Some(id) if id > 0 => id,
            _ => {
                let html = templates::render_error("Compliance", "Please select a tenant to view compliance status");
                let patch = PatchElements::new(html)
                    .selector("#compliance-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());
                return;
            }
        };

        // Query compliance-related metadata from the database
        let result = tokio::task::spawn_blocking(move || {
            use kimberlite_client::{Client, ClientConfig};
            use kimberlite_types::TenantId;

            let config = ClientConfig::default();
            let mut client = match Client::connect(&db_address, TenantId::new(tenant_id), config) {
                Ok(c) => c,
                Err(e) => return Err(e.to_string()),
            };

            // Count tables, total rows, and classified columns
            let tables = match client.query("SHOW TABLES", &[]) {
                Ok(r) => r.rows.iter().filter_map(|row| row.first().map(format_value)).collect::<Vec<_>>(),
                Err(_) => vec![],
            };

            let mut total_rows: u64 = 0;
            let mut classified_columns: u64 = 0;
            let mut total_columns: u64 = 0;

            for table in &tables {
                // Count rows
                if let Ok(r) = client.query(&format!("SELECT COUNT(*) FROM {table}"), &[]) {
                    if let Some(kimberlite_client::QueryValue::BigInt(n)) = r.rows.first().and_then(|r| r.first()) {
                        total_rows += *n as u64;
                    }
                }
                // Count classified columns
                if let Ok(r) = client.query(&format!("SHOW CLASSIFICATIONS FOR {table}"), &[]) {
                    classified_columns += r.rows.len() as u64;
                }
                // Count total columns
                if let Ok(r) = client.query(&format!("SHOW COLUMNS FROM {table}"), &[]) {
                    total_columns += r.rows.len() as u64;
                }
            }

            Ok((tables.len() as u64, total_rows, classified_columns, total_columns))
        })
        .await;

        match result {
            Ok(Ok((table_count, total_rows, classified_columns, total_columns))) => {
                let html = templates::render_compliance_dashboard(
                    tenant_id,
                    table_count,
                    total_rows,
                    classified_columns,
                    total_columns,
                );
                let patch = PatchElements::new(html)
                    .selector("#compliance-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
            _ => {
                let html = templates::render_error("Error", "Failed to load compliance status");
                let patch = PatchElements::new(html)
                    .selector("#compliance-container")
                    .mode(ElementPatchMode::Inner);
                yield Ok(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream)
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

    #[test]
    fn test_csv_escape_field_simple() {
        let mut out = String::new();
        csv_escape_field(&mut out, "hello");
        assert_eq!(out, "hello");
    }

    #[test]
    fn test_csv_escape_field_with_comma() {
        let mut out = String::new();
        csv_escape_field(&mut out, "hello, world");
        assert_eq!(out, "\"hello, world\"");
    }

    #[test]
    fn test_csv_escape_field_with_quotes() {
        let mut out = String::new();
        csv_escape_field(&mut out, r#"say "hi""#);
        assert_eq!(out, r#""say ""hi""""#);
    }

    #[test]
    fn test_schema_to_js_object() {
        let schema = vec![
            ("patients".to_string(), vec!["id (BIGINT)".to_string(), "name (TEXT)".to_string()]),
            ("visits".to_string(), vec!["id (BIGINT)".to_string()]),
        ];
        assert_eq!(
            schema_to_js_object(&schema),
            r#"{"patients":["id","name"],"visits":["id"]}"#
        );
    }

    #[test]
    fn test_schema_to_js_object_empty() {
        let schema: Vec<(String, Vec<String>)> = vec![];
        assert_eq!(schema_to_js_object(&schema), "{}");
    }

    #[test]
    fn test_build_where_clause_empty() {
        assert_eq!(build_where_clause(&[]), "");
    }

    #[test]
    fn test_build_where_clause_single() {
        let filters = vec![BrowseFilter {
            column: "name".to_string(),
            operator: "=".to_string(),
            value: "Alice".to_string(),
        }];
        assert_eq!(build_where_clause(&filters), " WHERE name = 'Alice'");
    }

    #[test]
    fn test_build_where_clause_multiple() {
        let filters = vec![
            BrowseFilter {
                column: "age".to_string(),
                operator: ">".to_string(),
                value: "18".to_string(),
            },
            BrowseFilter {
                column: "status".to_string(),
                operator: "=".to_string(),
                value: "active".to_string(),
            },
        ];
        assert_eq!(
            build_where_clause(&filters),
            " WHERE age > '18' AND status = 'active'"
        );
    }

    #[test]
    fn test_build_where_clause_is_null() {
        let filters = vec![BrowseFilter {
            column: "email".to_string(),
            operator: "IS NULL".to_string(),
            value: String::new(),
        }];
        assert_eq!(build_where_clause(&filters), " WHERE email IS NULL");
    }

    #[test]
    fn test_build_where_clause_rejects_invalid_column() {
        let filters = vec![BrowseFilter {
            column: "name; DROP TABLE--".to_string(),
            operator: "=".to_string(),
            value: "test".to_string(),
        }];
        assert_eq!(build_where_clause(&filters), "");
    }

    #[test]
    fn test_build_where_clause_escapes_quotes() {
        let filters = vec![BrowseFilter {
            column: "name".to_string(),
            operator: "=".to_string(),
            value: "O'Brien".to_string(),
        }];
        assert_eq!(
            build_where_clause(&filters),
            " WHERE name = 'O''Brien'"
        );
    }
}
