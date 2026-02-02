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

    // TODO: Execute query via kimberlite_client
    // For now, return mock data
    let response = QueryResponse {
        columns: vec![
            "id".to_string(),
            "name".to_string(),
            "created_at".to_string(),
        ],
        rows: vec![
            vec![
                "1".to_string(),
                "Alice".to_string(),
                "2024-01-01".to_string(),
            ],
            vec!["2".to_string(), "Bob".to_string(), "2024-01-02".to_string()],
        ],
        row_count: 2,
        execution_time_ms: 42,
    };

    tracing::info!(
        tenant_id = req.tenant_id,
        query = %req.query,
        offset = ?req.offset,
        row_count = response.row_count,
        "Query executed"
    );

    (StatusCode::OK, Json(response)).into_response()
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
    // TODO: Validate tenant exists and fetch schema from kernel
    // For now, return mock data
    let response = SelectTenantResponse {
        tenant_id: req.tenant_id,
        tenant_name: format!("tenant-{}", req.tenant_id),
        tables: vec![
            TableInfo {
                table_id: 1,
                table_name: "patients".to_string(),
                column_count: 5,
            },
            TableInfo {
                table_id: 2,
                table_name: "visits".to_string(),
                column_count: 3,
            },
        ],
    };

    tracing::info!(
        tenant_id = req.tenant_id,
        table_count = response.tables.len(),
        "Tenant selected"
    );

    (StatusCode::OK, Json(response)).into_response()
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
    async fn test_execute_query_success() {
        let state = mock_state();
        let req = QueryRequest {
            tenant_id: 1,
            query: "SELECT * FROM test".to_string(),
            offset: None,
        };

        let response = execute_query(State(state), Json(req)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_select_tenant() {
        let state = mock_state();
        let req = SelectTenantRequest { tenant_id: 1 };

        let response = select_tenant(State(state), Json(req)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}
