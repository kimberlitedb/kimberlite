//! Static asset serving routes.

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

/// Serves CSS files with correct content-type.
pub async fn serve_css(Path(path): Path<String>) -> Response {
    match crate::assets::get_css(&path) {
        Some(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            content,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "CSS file not found").into_response(),
    }
}

/// Serves font files with correct content-type.
pub async fn serve_font(Path(path): Path<String>) -> Response {
    match crate::assets::get_font(&path) {
        Some(content) => {
            let content_type = if path.ends_with(".woff2") {
                "font/woff2"
            } else if path.ends_with(".woff") {
                "font/woff"
            } else if path.ends_with(".ttf") {
                "font/ttf"
            } else {
                "application/octet-stream"
            };

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                content,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Font file not found").into_response(),
    }
}

/// Serves vendor JavaScript and other files.
pub async fn serve_vendor(Path(path): Path<String>) -> Response {
    match crate::assets::get_vendor(&path) {
        Some(content) => {
            let content_type = if path.ends_with(".js") {
                "application/javascript; charset=utf-8"
            } else if path.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "application/octet-stream"
            };

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                content,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Vendor file not found").into_response(),
    }
}

/// Serves the icon sprite SVG.
pub async fn serve_icons() -> Response {
    match crate::assets::get_icons() {
        Some(content) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/svg+xml"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            content,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Icon sprite not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_serve_css_exists() {
        let response = serve_css(Path("global/variables.css".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_css_not_found() {
        let response = serve_css(Path("nonexistent.css".to_string())).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_serve_font_exists() {
        let response = serve_font(Path("test-signifier-regular.woff2".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_vendor_exists() {
        let response = serve_vendor(Path("datastar.js".to_string())).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}
