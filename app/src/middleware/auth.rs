/// API-key authentication middleware.
///
/// This module implements EPIC-2 (Authentication) across two user stories:
///
/// **US-4 (#10)** — API-key middleware:
/// - C-1: constant-time comparison via `subtle::ConstantTimeEq`; compares over
///   `max(a.len(), b.len())` bytes so key length is not leaked by timing.
/// - C-2: outermost Axum layer; `/health` (exact) and `/pkg/*` exempted
///
/// **US-5 (#11)** — Session cookie:
/// - C-5: session store typed `Arc<RwLock<HashMap<String, Session>>>`
/// - AC-6.1: valid `api_key` → `Set-Cookie: session=<token>; HttpOnly; Secure; SameSite=Strict`
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

/// Routes that are accessible without any credentials (C-2).
///
/// `/health` is an **exact** match so future routes like `/healthz` or
/// `/health/admin` are not inadvertently made public.
/// `/pkg/` uses a prefix match to cover all static WASM / JS assets.
fn is_public(path: &str) -> bool {
    path == "/health" || path.starts_with("/pkg/")
}

/// Cookie name used for the session token.
pub const SESSION_COOKIE_NAME: &str = "session";

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
    if is_public(path) {
        return next.run(request).await;
    }

    // Check session cookie (AC-6.2).
    if let Some(token) = extract_session_cookie(&request) {
        if session::is_valid_session(&state.sessions, &token) {
            return next.run(request).await;
        }
    }

    // Check `api_key` query parameter (C-1, AC-6.1).
    if let Some(query) = request.uri().query() {
        if let Some(candidate) = extract_api_key_param(query) {
            if constant_time_eq(&candidate, &state.api_key) {
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

/// Extract the value of the `api_key` query parameter from a raw query string,
/// percent-decoding it so that API keys containing reserved characters work
/// reliably.
fn extract_api_key_param(query: &str) -> Option<String> {
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "api_key")
        .map(|(_, value)| value.into_owned())
}

/// Extract the session token from the `Cookie` request header, if present.
///
/// Uses `SESSION_COOKIE_NAME` so the cookie name is defined in a single place.
fn extract_session_cookie(request: &Request<Body>) -> Option<String> {
    let cookie_header = request.headers().get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{}=", SESSION_COOKIE_NAME);
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(prefix.as_str()) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Append `Set-Cookie: session=<token>; Path=/; HttpOnly; Secure; SameSite=Strict`
/// to `response`.
///
/// `Secure` is included so browsers only transmit the session token over HTTPS,
/// preventing token leakage over plaintext connections.
fn attach_session_cookie(response: &mut Response, token: &str) {
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; Secure; SameSite=Strict"
    );
    if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

/// Constant-time equality check for API key strings (C-1).
///
/// Compares over `max(a.len(), b.len())` bytes — missing bytes are treated as
/// 0 — and separately asserts equal lengths.  This ensures both the content
/// and the length comparison run in constant time regardless of candidate input,
/// preventing length-based timing oracles.
fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    let a = candidate.as_bytes();
    let b = expected.as_bytes();
    let max_len = a.len().max(b.len());

    // Pad both slices to max_len with zeroes for the comparison.
    let mut result = subtle::Choice::from(1u8); // start: equal
    for i in 0..max_len {
        let byte_a = a.get(i).copied().unwrap_or(0);
        let byte_b = b.get(i).copied().unwrap_or(0);
        result &= byte_a.ct_eq(&byte_b);
    }
    // Also require equal lengths (prevents zero-length key matching anything).
    let same_len = subtle::Choice::from((a.len() == b.len()) as u8);
    (result & same_len).into()
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

    // ── is_public ─────────────────────────────────────────────────────────────

    #[test]
    fn health_exact_is_public() {
        assert!(is_public("/health"));
    }

    #[test]
    fn health_sub_path_is_not_public() {
        assert!(!is_public("/healthz"));
        assert!(!is_public("/health/admin"));
        assert!(!is_public("/health/"));
    }

    #[test]
    fn pkg_prefix_is_public() {
        assert!(is_public("/pkg/app.wasm"));
        assert!(is_public("/pkg/"));
    }

    #[test]
    fn root_is_not_public() {
        assert!(!is_public("/"));
        assert!(!is_public("/bookmarks"));
    }

    // ── extract_api_key_param ─────────────────────────────────────────────────

    #[test]
    fn extracts_api_key_when_present() {
        assert_eq!(
            extract_api_key_param("api_key=my-secret"),
            Some("my-secret".to_owned())
        );
    }

    #[test]
    fn extracts_api_key_from_multiple_params() {
        assert_eq!(
            extract_api_key_param("foo=bar&api_key=my-secret&baz=qux"),
            Some("my-secret".to_owned())
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

    #[test]
    fn percent_decodes_api_key_value() {
        // A key containing '+' encoded as '%2B' must decode correctly.
        assert_eq!(
            extract_api_key_param("api_key=my%2Bsecret"),
            Some("my+secret".to_owned())
        );
    }

    #[test]
    fn plus_in_query_decoded_as_space() {
        // RFC 1866 / application/x-www-form-urlencoded: '+' means space.
        assert_eq!(
            extract_api_key_param("api_key=hello+world"),
            Some("hello world".to_owned())
        );
    }

    // ── extract_session_cookie ────────────────────────────────────────────────

    #[test]
    fn extracts_session_cookie_when_present() {
        use axum::http::Request;
        let req = Request::builder()
            .header(header::COOKIE, "session=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_session_cookie(&req), Some("abc123".to_owned()));
    }

    #[test]
    fn extracts_session_from_multi_cookie() {
        use axum::http::Request;
        let req = Request::builder()
            .header(header::COOKIE, "foo=bar; session=tok; baz=qux")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_session_cookie(&req), Some("tok".to_owned()));
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
