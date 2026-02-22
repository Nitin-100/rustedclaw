//! Embedded static frontend assets.
//!
//! The HTML, CSS, and JS files from `frontend/` are compiled into the binary
//! using `include_str!`, enabling single-binary deployment.

use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

/// The embedded frontend files.
const INDEX_HTML: &str = include_str!("../../../frontend/index.html");
const STYLE_CSS: &str = include_str!("../../../frontend/style.css");
const APP_JS: &str = include_str!("../../../frontend/app.js");

/// Build a router that serves the embedded frontend.
pub fn frontend_router() -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/static/style.css", get(css_handler))
        .route("/static/app.js", get(js_handler))
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn css_handler() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLE_CSS,
    )
        .into_response()
}

async fn js_handler() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
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

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("RustedClaw"), "Index HTML should contain 'RustedClaw'");
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
