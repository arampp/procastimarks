/// API-key authentication middleware.
///
/// This module implements EPIC-2 (Authentication) across two user stories:
///
/// **US-4 (#10)** — API-key middleware:
/// - C-1: constant-time comparison via `subtle::ConstantTimeEq`
/// - C-2: outermost Axum layer; `/health` and `/pkg/*` exempted
///
/// **US-5 (#11)** — Session cookie:
/// - C-5: session store typed `Arc<RwLock<HashMap<String, Session>>>`
/// - AC-6.1: valid `api_key` → `Set-Cookie: session=<token>; HttpOnly; SameSite=Strict`
/// - AC-6.2: valid session cookie accepted without `api_key` param
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use subtle::ConstantTimeEq;

use crate::session::{self, SessionStore};

/// Shared application state threaded through the auth middleware.
///
/// Bundled into a single struct because Axum supports only one typed state
/// per router.  Both fields are cheaply cloneable (`Arc` / `Arc<str>`).
#[derive(Clone)]
pub struct AppState {
    /// Expected API key (from `API_KEY` env var), stored as `Arc<str>`.
    pub api_key: Arc<str>,
    /// In-memory session store (C-5).
    pub sessions: SessionStore,
}

/// Paths that are accessible without any credentials (C-2).
///
/// The check is a prefix match so `/health` covers the exact path and
/// `/pkg/` covers all static WASM / JS assets served by Leptos.
const PUBLIC_PREFIXES: &[&str] = &["/health", "/pkg/"];

/// Cookie name used for the session token.
const SESSION_COOKIE_NAME: &str = "session";

/// Axum `from_fn_with_state` middleware handler.
///
/// Decision tree:
/// 1. Public path → pass through immediately (C-2).
/// 2. Valid session cookie → pass through (AC-6.2).
/// 3. Valid `api_key` query param → create session, attach `Set-Cookie`, pass through (AC-6.1).
/// 4. Otherwise → return `401 Unauthorized`.
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // C-2: Exempt public routes.
    if PUBLIC_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return next.run(request).await;
    }

    // Check session cookie (AC-6.2).
    if let Some(token) = extract_session_cookie(&request) {
        if session::is_valid_session(&state.sessions, token) {
            return next.run(request).await;
        }
    }

    // Check `api_key` query parameter (C-1, AC-6.1).
    if let Some(query) = request.uri().query() {
        if let Some(candidate) = extract_api_key_param(query) {
            if constant_time_eq(candidate, &state.api_key) {
                // Create a new session and set the cookie on the response.
                let token = session::create_session(&state.sessions);
                let mut response = next.run(request).await;
                attach_session_cookie(&mut response, &token);
                return response;
            }
        }
    }

    // No valid credential found.
    unauthorized_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the value of the `api_key` query parameter from a raw query string.
fn extract_api_key_param(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("api_key=") {
            return Some(value);
        }
    }
    None
}

/// Extract the session token from the `Cookie` request header, if present.
///
/// Returns the token value (the part after `session=`) or `None`.
fn extract_session_cookie(request: &Request<Body>) -> Option<&str> {
    let cookie_header = request.headers().get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("session=") {
            return Some(value);
        }
    }
    None
}

/// Append `Set-Cookie: session=<token>; Path=/; HttpOnly; SameSite=Strict`
/// to `response`.
fn attach_session_cookie(response: &mut Response, token: &str) {
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict"
    );
    if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
}

/// Constant-time equality check for API key strings (C-1).
///
/// Uses `subtle::ConstantTimeEq` with length-difference folded in to prevent
/// length-based timing leaks.
fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    let a = candidate.as_bytes();
    let b = expected.as_bytes();
    let len = a.len().min(b.len());
    let prefix_eq = a[..len].ct_eq(&b[..len]);
    let same_len = subtle::Choice::from((a.len() == b.len()) as u8);
    (prefix_eq & same_len).into()
}

/// Build the HTTP 401 Unauthorized response.
fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(
            r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>401 Unauthorized</title></head>
<body>
<h1>401 Unauthorized</h1>
<p>A valid <code>api_key</code> is required to access this page.</p>
</body>
</html>"#,
        ),
    )
        .into_response()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_api_key_param ─────────────────────────────────────────────────

    #[test]
    fn extracts_api_key_when_present() {
        assert_eq!(
            extract_api_key_param("api_key=my-secret"),
            Some("my-secret")
        );
    }

    #[test]
    fn extracts_api_key_from_multiple_params() {
        assert_eq!(
            extract_api_key_param("foo=bar&api_key=my-secret&baz=qux"),
            Some("my-secret")
        );
    }

    #[test]
    fn returns_none_when_api_key_absent() {
        assert_eq!(extract_api_key_param("foo=bar&baz=qux"), None);
    }

    #[test]
    fn returns_none_for_empty_query() {
        assert_eq!(extract_api_key_param(""), None);
    }

    // ── extract_session_cookie ────────────────────────────────────────────────

    #[test]
    fn extracts_session_cookie_when_present() {
        use axum::http::Request;
        let req = Request::builder()
            .header(header::COOKIE, "session=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_session_cookie(&req), Some("abc123"));
    }

    #[test]
    fn extracts_session_from_multi_cookie() {
        use axum::http::Request;
        let req = Request::builder()
            .header(header::COOKIE, "foo=bar; session=tok; baz=qux")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_session_cookie(&req), Some("tok"));
    }

    #[test]
    fn returns_none_when_no_cookie_header() {
        use axum::http::Request;
        let req = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(extract_session_cookie(&req), None);
    }

    #[test]
    fn returns_none_when_session_cookie_absent() {
        use axum::http::Request;
        let req = Request::builder()
            .header(header::COOKIE, "other=value")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_session_cookie(&req), None);
    }

    // ── constant_time_eq ──────────────────────────────────────────────────────

    #[test]
    fn equal_strings_return_true() {
        assert!(constant_time_eq("secret", "secret"));
    }

    #[test]
    fn different_strings_return_false() {
        assert!(!constant_time_eq("wrong", "secret"));
    }

    #[test]
    fn different_length_shorter_returns_false() {
        assert!(!constant_time_eq("sec", "secret"));
    }

    #[test]
    fn different_length_longer_returns_false() {
        assert!(!constant_time_eq("secret-longer", "secret"));
    }

    #[test]
    fn empty_vs_nonempty_returns_false() {
        assert!(!constant_time_eq("", "secret"));
    }

    #[test]
    fn both_empty_returns_true() {
        assert!(constant_time_eq("", ""));
    }
}
