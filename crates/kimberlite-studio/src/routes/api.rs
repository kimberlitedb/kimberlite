//! API endpoints for Studio queries and operations.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};

use crate::state::StudioState;

/// Request to execute a SQL query.
#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub tenant_id: u64,
    pub query: String,
    #[serde(default)]
    pub offset: Option<u64>,
}

/// Response from query execution.
#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub execution_time_ms: u64,
}

/// Error response.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

/// Executes a SQL query against the database.
///
/// POST /api/query
pub async fn execute_query(
    State(_state): State<StudioState>,
    Json(req): Json<QueryRequest>,
) -> Response {
    // Validate tenant_id is provided
    if req.tenant_id == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Tenant ID is required".to_string(),
                details: Some("Please select a tenant before executing queries".to_string()),
            }),
        )
            .into_response();
    }

    // Execute query via kimberlite-client
    let db_address = _state.db_address.clone();
    let tenant_id = req.tenant_id;
    let query = req.query.clone();

    let start = std::time::Instant::now();

    // Bridge sync client to async handler
    let result = tokio::task::spawn_blocking(move || {
        use kimberlite_client::{Client, ClientConfig};
        use kimberlite_types::TenantId;

        let config = ClientConfig::default();
        let mut client = Client::connect(&db_address, TenantId::new(tenant_id), config)?;
        client.query(&query, &[])
    })
    .await;

    let elapsed_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(query_response)) => {
            // Map wire QueryResponse to Studio QueryResponse
            let columns: Vec<String> = query_response
                .columns
                .iter()
                .map(|c| c.to_string())
                .collect();

            let rows: Vec<Vec<String>> = query_response
                .rows
                .iter()
                .map(|row| row.iter().map(format_query_value).collect())
                .collect();

            let row_count = rows.len();

            tracing::info!(
                tenant_id = req.tenant_id,
                query = %req.query,
                offset = ?req.offset,
                row_count,
                elapsed_ms,
                "Query executed"
            );

            let response = QueryResponse {
                columns,
                rows,
                row_count,
                execution_time_ms: elapsed_ms,
            };

            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!(
                tenant_id = req.tenant_id,
                query = %req.query,
                error = %e,
                "Query failed"
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Query execution failed".to_string(),
                    details: Some(e.to_string()),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Query task panicked");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal error".to_string(),
                    details: Some("Query execution task failed".to_string()),
                }),
            )
                .into_response()
        }
    }
}

/// Request to select a tenant.
#[derive(Debug, Deserialize)]
pub struct SelectTenantRequest {
    pub tenant_id: u64,
}

/// Response from tenant selection.
#[derive(Debug, Serialize)]
pub struct SelectTenantResponse {
    pub tenant_id: u64,
    pub tenant_name: String,
    pub tables: Vec<TableInfo>,
}

/// Table information.
#[derive(Debug, Serialize)]
pub struct TableInfo {
    pub table_id: u64,
    pub table_name: String,
    pub column_count: usize,
}

/// Selects a tenant and returns its schema.
///
/// POST /api/select-tenant
pub async fn select_tenant(
    State(_state): State<StudioState>,
    Json(req): Json<SelectTenantRequest>,
) -> Response {
    // Try to discover tables by querying the server
    let db_address = _state.db_address.clone();
    let tenant_id = req.tenant_id;

    let tables = tokio::task::spawn_blocking(move || -> Vec<TableInfo> {
        use kimberlite_client::{Client, ClientConfig};
        use kimberlite_types::TenantId;

        let config = ClientConfig::default();
        let mut client = match Client::connect(&db_address, TenantId::new(tenant_id), config) {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        // Query system tables if available - fall back to empty
        match client.query("SELECT table_name FROM information_schema.tables", &[]) {
            Ok(resp) => resp
                .rows
                .iter()
                .enumerate()
                .map(|(i, row)| TableInfo {
                    table_id: (i + 1) as u64,
                    table_name: row.first().map(format_query_value).unwrap_or_default(),
                    column_count: 0,
                })
                .collect(),
            Err(_) => vec![],
        }
    })
    .await
    .unwrap_or_default();

    let response = SelectTenantResponse {
        tenant_id: req.tenant_id,
        tenant_name: format!("tenant-{}", req.tenant_id),
        tables,
    };

    tracing::info!(
        tenant_id = req.tenant_id,
        table_count = response.tables.len(),
        "Tenant selected"
    );

    (StatusCode::OK, Json(response)).into_response()
}

/// Formats a wire QueryValue to a display string.
fn format_query_value(val: &kimberlite_client::QueryValue) -> String {
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
    use std::sync::Arc;

    fn mock_state() -> StudioState {
        use crate::broadcast::ProjectionBroadcast;

        StudioState::new(
            Arc::new(ProjectionBroadcast::default()),
            "127.0.0.1:5432".to_string(),
            Some(1),
            5555,
        )
    }

    #[tokio::test]
    async fn test_execute_query_requires_tenant() {
        let state = mock_state();
        let req = QueryRequest {
            tenant_id: 0,
            query: "SELECT * FROM test".to_string(),
            offset: None,
        };

        let response = execute_query(State(state), Json(req)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_execute_query_returns_error_without_server() {
        let state = mock_state();
        let req = QueryRequest {
            tenant_id: 1,
            query: "SELECT * FROM test".to_string(),
            offset: None,
        };

        let response = execute_query(State(state), Json(req)).await;
        // Without a running server, the query will fail with a connection error
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_select_tenant() {
        let state = mock_state();
        let req = SelectTenantRequest { tenant_id: 1 };

        let response = select_tenant(State(state), Json(req)).await;
        // Returns OK even without server (gracefully falls back to empty tables)
        assert_eq!(response.status(), StatusCode::OK);
    }
}
