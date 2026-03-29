/// Bookmarklet installation component.
///
/// # US-6 (#12) — Bookmarklet JavaScript snippet
///
/// Satisfies:
/// * AC-1.1: The bookmarklet opens a new tab at
///   `<origin>/add?url=<encoded-url>&api_key=<key>`.
/// * The URL parameter is `encodeURIComponent`-encoded.
/// * The bookmarklet is < 50 lines of vanilla JavaScript (one-liner URI).
/// * The bookmarklet is accessible from the application UI (home page).
/// * Works in Firefox and Chrome — only `window.open` and `encodeURIComponent`
///   are used; no browser-extension APIs.
use leptos::prelude::*;

use crate::server_fns::get_api_key;

/// Builds the bookmarklet `javascript:` URI for a given API key.
///
/// The host is resolved at click time via `window.location.origin` so the
/// bookmarklet works regardless of which hostname the instance is accessed
/// from.
///
/// The API key is percent-encoded for the query parameter so that any
/// reserved characters (`&`, `=`, `+`, etc.) are transmitted correctly.
///
/// # Panics
///
/// Never — the returned string is always a valid attribute value.
pub fn bookmarklet_uri(api_key: &str) -> String {
    // Percent-encode the API key for safe embedding in a query string.
    let encoded_key: String = form_urlencoded::byte_serialize(api_key.as_bytes()).collect();

    // One-liner: safe for use in an `href` attribute.
    // window.open returns a Window handle; the trailing void(0) makes the
    // bookmarklet URI return undefined, preventing the browser from
    // navigating the current tab.
    format!(
        "javascript:(function(){{window.open(window.location.origin+'/add?url='+encodeURIComponent(location.href)+'&api_key={encoded_key}','_blank');}})();void(0);"
    )
}

/// Renders a draggable anchor the owner can add to their bookmarks bar.
///
/// The API key is fetched server-side so it is pre-embedded in the
/// bookmarklet URL — the owner does not need to configure anything after
/// dragging the link.
#[component]
pub fn BookmarkletInstall() -> impl IntoView {
    // Fetch the API key from the server (AC auth-protected endpoint).
    let api_key_res = Resource::new(|| (), |_| async { get_api_key().await });

    view! {
        <section class="bookmarklet-install">
            <h2>"Install Bookmarklet"</h2>
            <p>
                "Drag the button below to your bookmarks bar. "
                "Click it on any web page to save it to Procastimarks."
            </p>
            <Suspense fallback=|| view! { <p>"Loading bookmarklet…"</p> }>
                {move || {
                    match api_key_res.get() {
                        None => view! { <p>"Loading…"</p> }.into_any(),
                        Some(Err(err)) => {
                            // Log details server-side; show a generic message
                            // so users aren't pointed at a specific (possibly
                            // wrong) remediation.
                            leptos::logging::error!("Failed to load API key for bookmarklet: {err}");
                            view! {
                                <p class="error-message" role="alert">
                                    "Could not load bookmarklet. Please try again later."
                                </p>
                            }.into_any()
                        },
                        Some(Ok(key)) => {
                            let href = bookmarklet_uri(&key);
                            view! {
                                <a
                                    href=href
                                    class="bookmarklet-link"
                                >
                                    "📌 Save to Procastimarks"
                                </a>
                                <p class="bookmarklet-hint">
                                    "Right-click → \"Bookmark this link\" or drag to your bookmarks bar."
                                </p>
                            }.into_any()
                        }
                    }
                }}
            </Suspense>
        </section>
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// API key with reserved characters is percent-encoded in the URI.
    #[test]
    fn bookmarklet_uri_percent_encodes_api_key() {
        // A key with '&' and '=' would break the query string if not encoded.
        let uri = bookmarklet_uri("key&foo=bar");
        assert!(
            uri.contains("key%26foo%3Dbar"),
            "reserved chars in API key must be percent-encoded; got: {uri}"
        );
        assert!(
            !uri.contains("key&foo"),
            "raw '&' must not appear in the encoded key"
        );
    }

    /// The bookmarklet URI must contain the API key verbatim.
    #[test]
    fn bookmarklet_uri_contains_api_key() {
        let uri = bookmarklet_uri("my-secret");
        assert!(
            uri.contains("my-secret"),
            "bookmarklet URI must embed the API key"
        );
    }

    /// The bookmarklet URI must start with `javascript:`.
    #[test]
    fn bookmarklet_uri_is_javascript_scheme() {
        let uri = bookmarklet_uri("any-key");
        assert!(
            uri.starts_with("javascript:"),
            "bookmarklet URI must start with 'javascript:'"
        );
    }

    /// The bookmarklet URI must open a new tab (`'_blank'`).
    #[test]
    fn bookmarklet_uri_opens_new_tab() {
        let uri = bookmarklet_uri("any-key");
        assert!(
            uri.contains("'_blank'"),
            "bookmarklet must open a new tab"
        );
    }

    /// The bookmarklet URI must use `encodeURIComponent` to encode the URL.
    #[test]
    fn bookmarklet_uri_uses_encode_uri_component() {
        let uri = bookmarklet_uri("any-key");
        assert!(
            uri.contains("encodeURIComponent"),
            "bookmarklet must use encodeURIComponent"
        );
    }

    /// The raw JavaScript source is < 50 lines (PRD constraint).
    ///
    /// The bookmarklet is a single-line URI, so this is trivially satisfied,
    /// but we assert it explicitly to document and enforce the constraint.
    #[test]
    fn bookmarklet_js_is_under_50_lines() {
        let uri = bookmarklet_uri("any-key");
        // Strip the "javascript:" prefix to get the JS body.
        let js_body = uri.trim_start_matches("javascript:");
        let line_count = js_body.lines().count();
        assert!(
            line_count <= 50,
            "bookmarklet JS must be < 50 lines; got {line_count}"
        );
    }
}
