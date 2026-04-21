//! actix-web framework integration example.
//!
//! Exercises: connect (pooled) + query + consent.
//!
//! Run:
//!     cargo run --example actix_app -p kimberlite-examples
//!
//! Endpoints:
//!     GET  /health
//!     POST /patients       { "name": "...", "consent_purpose": "Analytics" }
//!     GET  /patients/{id}

use std::sync::Arc;

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use anyhow::Result;
use kimberlite_client::{ConsentPurpose, Pool, PoolConfig};
use kimberlite_types::TenantId;
use serde::{Deserialize, Serialize};

struct AppState {
    pool: Pool,
}

#[derive(Deserialize)]
struct CreatePatient {
    name: String,
    consent_purpose: String,
}

#[derive(Serialize)]
struct PatientOk {
    id: String,
    consent_id: String,
}

#[get("/health")]
async fn health() -> impl Responder {
    "ok"
}

#[post("/patients")]
async fn create_patient(
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreatePatient>,
) -> impl Responder {
    let purpose = match body.consent_purpose.as_str() {
        "Marketing" => ConsentPurpose::Marketing,
        "Analytics" => ConsentPurpose::Analytics,
        "Contractual" => ConsentPurpose::Contractual,
        _ => return HttpResponse::BadRequest().body("invalid consent_purpose"),
    };
    let state = state.clone();
    let name = body.name.clone();
    let result = web::block(move || -> Result<PatientOk, String> {
        let mut c = state.pool.acquire().map_err(|e| e.to_string())?;
        let grant = c
            .consent_grant(&name, purpose, None, None)
            .map_err(|e| e.to_string())?;
        Ok(PatientOk {
            id: name,
            consent_id: grant.consent_id,
        })
    })
    .await
    .unwrap();

    match result {
        Ok(ok) => HttpResponse::Created().json(ok),
        Err(e) => HttpResponse::InternalServerError().body(e),
    }
}

#[get("/patients/{id}")]
async fn get_patient(state: web::Data<Arc<AppState>>, path: web::Path<String>) -> impl Responder {
    let id = path.into_inner();
    let state = state.clone();
    let id_clone = id.clone();
    let result = web::block(move || -> Result<bool, String> {
        let mut c = state.pool.acquire().map_err(|e| e.to_string())?;
        c.consent_check(&id_clone, ConsentPurpose::Analytics)
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap();

    match result {
        Ok(has) => HttpResponse::Ok().json(serde_json::json!({
            "id": id,
            "analytics_consent": has,
        })),
        Err(e) => HttpResponse::InternalServerError().body(e),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let pool = Pool::new(
        "127.0.0.1:5432",
        TenantId::new(1),
        PoolConfig {
            max_size: 8,
            ..PoolConfig::default()
        },
    )
    .expect("pool init");
    let state = Arc::new(AppState { pool });

    eprintln!("actix example listening on http://0.0.0.0:3001");
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .service(health)
            .service(create_patient)
            .service(get_patient)
    })
    .bind("0.0.0.0:3001")?
    .run()
    .await
}
