//! FAQ Page Handler

use axum::response::IntoResponse;

use crate::templates::FaqTemplate;

/// Handler for the FAQ page.
pub async fn faq() -> impl IntoResponse {
    FaqTemplate::new("Kimberlite")
}
