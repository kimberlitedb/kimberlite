//! Documentation Handlers

use axum::response::{IntoResponse, Redirect};

use crate::templates::{DocsCliTemplate, DocsQuickStartTemplate, DocsSqlTemplate};

/// Handler for /docs - redirects to quick-start.
pub async fn docs_index() -> impl IntoResponse {
    Redirect::to("/docs/quick-start")
}

/// Handler for /docs/quick-start.
pub async fn quick_start() -> impl IntoResponse {
    DocsQuickStartTemplate::new("Quick Start")
}

/// Handler for /docs/reference/cli.
pub async fn cli_reference() -> impl IntoResponse {
    DocsCliTemplate::new("CLI Reference")
}

/// Handler for /docs/reference/sql.
pub async fn sql_reference() -> impl IntoResponse {
    DocsSqlTemplate::new("SQL Reference")
}
