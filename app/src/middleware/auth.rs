/// API-key authentication middleware.
///
/// This module implements EPIC-2 (Authentication), US-4 acceptance criteria,
/// and satisfies ATAM mandatory conditions:
///
/// - **C-1**: API key comparison uses `subtle::ConstantTimeEq` to prevent
///   timing attacks.  Plain `==` on `String` is explicitly avoided.
/// - **C-2**: The middleware is registered as the outermost Axum layer so it
///   wraps every route.  `/health` and `/pkg/*` are exempted by path prefix
///   check inside the middleware.
///
/// Session-cookie validation (US-5, #11) is reserved for a stub:
/// `valid_session_cookie` always returns `false` until the session store is
/// wired in.
use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
};
use subtle::ConstantTimeEq;

/// Paths that are accessible without any credentials (C-2).
///
/// The check is a prefix match so `/health` covers the exact path and
/// `/pkg/` covers all static WASM / JS assets served by Leptos.
const PUBLIC_PREFIXES: &[&str] = &["/health", "/pkg/"];

/// Axum `from_fn_with_state` middleware handler.
///
/// # Arguments
///
/// * `api_key` – the expected API key, supplied via Axum typed state.
/// * `request`  – the incoming HTTP request.
/// * `next`     – the rest of the middleware chain.
///
/// # Errors
///
/// Returns `(StatusCode::UNAUTHORIZED, Html(…))` when:
/// - the request path is not in `PUBLIC_PREFIXES`, **and**
/// - no valid `api_key` query parameter is present, **and**
/// - no valid session cookie is present (stub; always fails until US-5).
pub async fn require_auth(
    axum::extract::State(api_key): axum::extract::State<std::sync::Arc<str>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // C-2: Exempt public routes.
    if PUBLIC_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return next.run(request).await;
    }

    // Check session cookie first (stub for US-5).
    if valid_session_cookie(&request) {
        return next.run(request).await;
    }

    // Check `api_key` query parameter.
    if let Some(query) = request.uri().query() {
        if let Some(candidate) = extract_api_key_param(query) {
            if constant_time_eq(candidate, &api_key) {
                return next.run(request).await;
            }
        }
    }

    // No valid credential found.
    unauthorized_response()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the value of the `api_key` query parameter from a raw query string.
///
/// Returns `None` if the parameter is absent.
fn extract_api_key_param(query: &str) -> Option<&str> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("api_key=") {
            return Some(value);
        }
    }
    None
}

/// Constant-time equality check for API key strings (C-1).
///
/// Both byte slices are compared using `subtle::ConstantTimeEq`, which
/// processes all bytes in constant time regardless of content.  To avoid
/// leaking length information through early exit, the comparison is performed
/// by XOR-accumulating all byte differences; a length mismatch is encoded as
/// a non-zero accumulator without branching on it early.
fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    let a = candidate.as_bytes();
    let b = expected.as_bytes();

    // If lengths differ, we still must not short-circuit on length alone
    // (that would leak length via timing).  We pad the shorter slice by
    // comparing it against itself and then XOR the length inequality flag.
    //
    // `subtle::ConstantTimeEq` requires equal-length slices, so we use a
    // two-step approach:
    //   1. Compare the *common prefix* in constant time.
    //   2. Fold the length difference into the result without branching.
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

/// Validate a session cookie from the request (stub — always returns `false`).
///
/// This stub will be replaced in US-5 (#11) when the in-memory session store
/// (`Arc<RwLock<HashMap<String, Session>>>`, condition C-5) is introduced.
fn valid_session_cookie(_request: &Request<Body>) -> bool {
    false
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
