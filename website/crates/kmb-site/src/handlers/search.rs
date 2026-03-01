//! Search API Handler
//!
//! Returns an HTML fragment for Datastar to morph into the search dialog.

use askama::Template;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{state::AppState, templates::SearchResultsTemplate};

#[derive(Deserialize)]
pub struct SearchParams {
    q: Option<String>,
}

/// `GET /api/search?q=...` — returns an HTML fragment of search results.
pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let query = params.q.unwrap_or_default();

    if query.len() < 3 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let results = state.search().query(&query, 20);

    let template = SearchResultsTemplate {
        results,
        query,
    };

    let html = template.render().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html))
}
