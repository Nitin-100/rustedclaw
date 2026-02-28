//! Embedded static frontend assets.
//!
//! The HTML, CSS, and JS files from `frontend/` are compiled into the binary
//! using `include_str!`, enabling single-binary deployment.

use axum::{
    Router,
    http::{StatusCode, header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
};

/// The embedded frontend files.
const INDEX_HTML: &str = include_str!("../../../frontend/index.html");
const STYLE_CSS: &str = include_str!("../../../frontend/style.css");
const APP_JS: &str = include_str!("../../../frontend/app.js");

/// Build a router that serves the embedded frontend with security headers.
pub fn frontend_router() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/static/style.css", get(css_handler))
        .route("/static/app.js", get(js_handler))
}

/// Security headers applied to all frontend HTML responses.
fn security_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ws: wss:; img-src 'self' data:; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers
}

async fn index_handler() -> Response {
    (StatusCode::OK, security_headers(), INDEX_HTML).into_response()
}

async fn css_handler() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        STYLE_CSS,
    )
        .into_response()
}

async fn js_handler() -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        APP_JS,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_index_html() {
        let app = frontend_router();

        let req = Request::builder().uri("/").body(Body::empty()).unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("RustedClaw"),
            "Index HTML should contain 'RustedClaw'"
        );
        assert!(text.contains("<!DOCTYPE html>"), "Should be valid HTML");
    }

    #[tokio::test]
    async fn serves_css() {
        let app = frontend_router();

        let req = Request::builder()
            .uri("/static/style.css")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("text/css"));
    }

    #[tokio::test]
    async fn serves_js() {
        let app = frontend_router();

        let req = Request::builder()
            .uri("/static/app.js")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("javascript"));

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("RustedClaw"), "JS should contain app code");
    }
}
