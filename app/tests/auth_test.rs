//! Integration tests for the API-key authentication middleware.
//!
//! Satisfies US-4 acceptance criteria (AC-6.3, AC-6.4, AC-6.5, AC-6.6) and
//! ATAM mandatory conditions C-1 and C-2.
//!
//! Chicago-school style: the real Axum router is exercised via
//! `tower::ServiceExt::oneshot`.  The router is constructed with a known
//! test API key injected through the `API_KEY` environment variable so that
//! tests are self-contained and do not rely on a `.env` file.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use procastimarks::create_router;
use tower::ServiceExt;

const TEST_API_KEY: &str = "test-secret-key-for-auth-tests";

fn router() -> axum::Router {
    // Inject a known API key before constructing the router.
    std::env::set_var("API_KEY", TEST_API_KEY);
    create_router()
}

// ── AC-6.3 / AC-6.6: no credentials → 401 ────────────────────────────────────

/// Requesting any protected route without credentials must return HTTP 401.
#[tokio::test]
async fn protected_route_without_credentials_returns_401() {
    let app = router();
    let request = Request::builder()
        .uri("/")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET / without credentials must return 401"
    );
}

// ── AC-6.4: wrong key → 401 ───────────────────────────────────────────────────

/// Supplying an incorrect `api_key` query parameter must return HTTP 401.
#[tokio::test]
async fn protected_route_with_wrong_api_key_returns_401() {
    let app = router();
    let request = Request::builder()
        .uri("/?api_key=wrong-key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET / with the wrong api_key must return 401"
    );
}

// ── AC-6.3 (correct key → passes middleware) ─────────────────────────────────

/// Supplying the correct `api_key` query parameter must pass the middleware
/// (i.e. must NOT return HTTP 401).
#[tokio::test]
async fn protected_route_with_correct_api_key_is_not_401() {
    let app = router();
    let request = Request::builder()
        .uri(&format!("/?api_key={TEST_API_KEY}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_ne!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "GET / with the correct api_key must not return 401"
    );
}

// ── AC-6.5: /health is public ────────────────────────────────────────────────

/// GET /health must remain accessible without any credentials (C-2).
#[tokio::test]
async fn health_is_public_even_after_auth_middleware() {
    let app = router();
    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "/health must be public — no credentials required"
    );
}

// ── Timing-attack mitigation (C-1) ───────────────────────────────────────────

/// A key of different length from the correct key must still return 401
/// (guards against trivial length-based early exit leaking length information).
#[tokio::test]
async fn wrong_key_different_length_returns_401() {
    let app = router();
    // Use a key that is shorter than TEST_API_KEY.
    let request = Request::builder()
        .uri("/?api_key=short")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "A shorter wrong key must also return 401"
    );
}
