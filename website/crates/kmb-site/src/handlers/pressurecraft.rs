//! Pressurecraft page handlers.
//!
//! Interactive teaching workspace for understanding FCIS and the Kimberlite kernel.

use axum::response::IntoResponse;

use crate::templates::{PressurecraftDeterminismTemplate, PressurecraftFcisFlowTemplate};

/// FCIS Flow diagram - interactive visualization of commands -> kernel -> effects
pub async fn fcis_flow() -> impl IntoResponse {
    PressurecraftFcisFlowTemplate::new("FCIS Flow | Pressurecraft")
}

/// Determinism demo - proof that same input produces same output
pub async fn determinism_demo() -> impl IntoResponse {
    PressurecraftDeterminismTemplate::new("Determinism Demo | Pressurecraft")
}
