//! Finance Vertical Page Handler

use axum::response::IntoResponse;

use crate::templates::FinanceTemplate;

/// Handler for the Kimberlite-for-finance landing page.
pub async fn finance() -> impl IntoResponse {
    FinanceTemplate::new("Kimberlite")
}
