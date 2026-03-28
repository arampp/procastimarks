//! Integration tests for the API-key authentication middleware.
//!
//! Satisfies US-4 acceptance criteria (AC-6.3, AC-6.4, AC-6.5, AC-6.6) and
//! US-5 acceptance criteria (AC-6.1, AC-6.2), and ATAM mandatory conditions
//! C-1, C-2, and C-5.
//!
//! Chicago-school style: the real Axum router is exercised via
//! `tower::ServiceExt::oneshot`.  All routers are built from an explicit
//! `AppState` (using `create_router_with_state`) so tests never mutate the
//! process-wide `API_KEY` environment variable and can safely run in parallel.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use procastimarks::create_router_with_state;
use procastimarks::middleware::auth::AppState;
use procastimarks::session;
use std::sync::Arc;
use tower::ServiceExt;

const TEST_API_KEY: &str = "test-secret-key-for-auth-tests";

fn test_state() -> AppState {
    AppState {
        api_key: Arc::from(TEST_API_KEY),
        sessions: session::new_store(),
    }
}

/// Build a router from a fresh `AppState` with the test API key.
fn router() -> axum::Router {
    create_router_with_state(test_state())
}

/// Build a router from an explicit `AppState` (for two-request session tests).
fn router_with(state: AppState) -> axum::Router {
    create_router_with_state(state)
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

// ── C-2: /health sub-paths are NOT public ────────────────────────────────────

/// Routes starting with `/health` but not exactly `/health` must still require
/// auth, guarding against accidental public exposure of future admin endpoints.
#[tokio::test]
async fn health_sub_path_requires_auth() {
    let app = router();
    let request = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "/healthz must not be public — only exact /health is exempted"
    );
}

// ── Timing-attack mitigation (C-1) ───────────────────────────────────────────

/// A key of different length from the correct key must still return 401
/// (guards against trivial length-based early exit leaking length information).
#[tokio::test]
async fn wrong_key_different_length_returns_401() {
    let app = router();
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

// ── AC-6.1: valid api_key → Set-Cookie response header ───────────────────────

/// When the correct api_key is presented the response must include a
/// `Set-Cookie` header whose value starts with `session=` and includes the
/// `HttpOnly`, `Secure`, and `SameSite=Strict` attributes (AC-6.1).
#[tokio::test]
async fn correct_api_key_sets_session_cookie() {
    let app = router();
    let request = Request::builder()
        .uri(&format!("/?api_key={TEST_API_KEY}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);

    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("response must contain a Set-Cookie header when api_key is correct");

    let cookie_str = set_cookie.to_str().unwrap();
    assert!(
        cookie_str.starts_with("session="),
        "Set-Cookie must start with 'session=', got: {cookie_str}"
    );
    assert!(
        cookie_str.contains("HttpOnly"),
        "session cookie must be HttpOnly"
    );
    assert!(
        cookie_str.contains("Secure"),
        "session cookie must have the Secure attribute"
    );
    assert!(
        cookie_str.contains("SameSite=Strict"),
        "session cookie must be SameSite=Strict"
    );
}

// ── AC-6.2: valid session cookie → no api_key needed ─────────────────────────

/// A subsequent request bearing a valid session cookie (obtained from a prior
/// api_key-authenticated response) must be accepted without an api_key param
/// (AC-6.2).
///
/// Both requests use routers built from the *same* `AppState` (sharing the
/// same session store) so the token created in step 1 is visible in step 2.
#[tokio::test]
async fn valid_session_cookie_grants_access_without_api_key() {
    let state = test_state();

    // Step 1: authenticate with api_key to obtain a session token.
    let app1 = router_with(state.clone());
    let login_req = Request::builder()
        .uri(&format!("/?api_key={TEST_API_KEY}"))
        .body(Body::empty())
        .unwrap();

    let login_resp = app1.oneshot(login_req).await.unwrap();
    assert_ne!(login_resp.status(), StatusCode::UNAUTHORIZED);

    // Extract the token: "session=<token>; ..." → "session=<token>"
    let set_cookie_hdr = login_resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("must receive Set-Cookie on api_key login")
        .to_str()
        .unwrap()
        .to_owned();

    let cookie_header_value = set_cookie_hdr
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_owned(); // "session=<token>"

    // Step 2: follow-up request with only the session cookie — same store.
    let app2 = router_with(state);
    let cookie_req = Request::builder()
        .uri("/")
        .header(header::COOKIE, &cookie_header_value)
        .body(Body::empty())
        .unwrap();

    let cookie_resp = app2.oneshot(cookie_req).await.unwrap();

    assert_ne!(
        cookie_resp.status(),
        StatusCode::UNAUTHORIZED,
        "A valid session cookie must be accepted without api_key"
    );
}

// ── Session token is not the API key itself ───────────────────────────────────

/// The session token must differ from the API key (AC-6.1 — random token).
#[tokio::test]
async fn session_token_is_not_the_api_key() {
    let app = router();
    let request = Request::builder()
        .uri(&format!("/?api_key={TEST_API_KEY}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();

    // "session=<token>; ..."  — extract just the token value
    let token_value = set_cookie
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("session=");

    assert_ne!(
        token_value, TEST_API_KEY,
        "session token must not equal the API key"
    );
}
