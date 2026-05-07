//! Healthcare Vertical Page Handler

use axum::response::IntoResponse;

use crate::templates::HealthcareTemplate;

/// Handler for the Kimberlite-for-healthcare landing page.
pub async fn healthcare() -> impl IntoResponse {
    HealthcareTemplate::new("Kimberlite")
}
