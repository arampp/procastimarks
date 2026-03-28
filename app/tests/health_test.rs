//! Integration tests for the health endpoint.
//!
//! These tests satisfy US-1 acceptance criterion:
//!   "GET /health returns HTTP 200 `{"status":"ok"}` with no authentication required"
//!
//! Chicago-school style: we exercise the real Axum router via `tower::ServiceExt::oneshot`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use procastimarks::create_router_with_state;
use procastimarks::middleware::auth::AppState;
use procastimarks::session;
use std::sync::Arc;
use tower::ServiceExt;

fn test_state() -> AppState {
    AppState {
        api_key: Arc::from("test-key-health"),
        sessions: session::new_store(),
    }
}

/// GET /health must return 200 OK with {"status":"ok"}.
#[tokio::test]
async fn health_returns_200_ok() {
    let app = create_router_with_state(test_state());
    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

/// The health endpoint must be public — no api_key required.
#[tokio::test]
async fn health_requires_no_auth() {
    let app = create_router_with_state(test_state());
    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_ne!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "/health must be public — no authentication required"
    );
}
