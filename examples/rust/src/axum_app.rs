//! Axum framework integration example.
//!
//! Exercises: connect (pooled) + query + consent + audit.
//!
//! Run:
//!     # Start a server first: `just dev`
//!     cargo run --example axum_app -p kimberlite-examples
//!
//! Endpoints:
//!     GET  /health
//!     POST /patients       { "name": "...", "consent_purpose": "Analytics" }
//!     GET  /patients/:id
//!     GET  /info           (server info)

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use kimberlite_client::{ConsentPurpose, Pool, PoolConfig};
use kimberlite_types::TenantId;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    pool: Pool,
}

#[derive(Deserialize)]
struct CreatePatient {
    name: String,
    /// One of `"Marketing" | "Analytics" | "Contractual" | ...`
    consent_purpose: String,
}

#[derive(Serialize)]
struct PatientOk {
    id: String,
    consent_id: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let pool = Pool::new(
        "127.0.0.1:5432",
        TenantId::new(1),
        PoolConfig {
            max_size: 8,
            ..PoolConfig::default()
        },
    )?;
    let state = AppState { pool };

    let app = Router::new()
        .route("/health", get(health))
        .route("/patients", post(create_patient))
        .route("/patients/:id", get(get_patient))
        .route("/info", get(server_info))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    eprintln!("axum example listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn server_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Run the blocking call on a dedicated thread pool to respect axum's async runtime.
    let state = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut c = state.pool.acquire().map_err(|e| e.to_string())?;
        c.server_info().map_err(|e| e.to_string())
    })
    .await
    .unwrap();
    match result {
        Ok(info) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "version": info.build_version,
                "uptime_secs": info.uptime_secs,
                "capabilities": info.capabilities,
            })),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn create_patient(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreatePatient>,
) -> impl IntoResponse {
    let purpose = match body.consent_purpose.as_str() {
        "Marketing" => ConsentPurpose::Marketing,
        "Analytics" => ConsentPurpose::Analytics,
        "Contractual" => ConsentPurpose::Contractual,
        _ => return (StatusCode::BAD_REQUEST, "invalid consent_purpose".to_string()).into_response(),
    };
    let state = state.clone();
    let name = body.name.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<PatientOk, String> {
        let mut c = state.pool.acquire().map_err(|e| e.to_string())?;
        let grant = c
            .consent_grant(&name, purpose, None)
            .map_err(|e| e.to_string())?;
        Ok(PatientOk {
            id: name,
            consent_id: grant.consent_id,
        })
    })
    .await
    .unwrap();
    match result {
        Ok(ok) => (StatusCode::CREATED, Json(ok)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn get_patient(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let state = state.clone();
    let id_clone = id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<bool, String> {
        let mut c = state.pool.acquire().map_err(|e| e.to_string())?;
        c.consent_check(&id_clone, ConsentPurpose::Analytics)
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap();
    match result {
        Ok(has_consent) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": id,
                "analytics_consent": has_consent,
            })),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}
