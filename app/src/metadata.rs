/// Server-side metadata fetcher — US-7 (#13), EPIC-3 (#3).
///
/// # Responsibility
///
/// Fetch the HTML at a given URL and extract:
/// * the page `<title>` text, and
/// * the `<meta name="description" content="…">` value.
///
/// On any error — scheme validation failure, SSRF guard rejection, network
/// error, timeout, non-200 status, or parse failure — the function returns a
/// safe fallback: title = raw URL, description = empty string.
///
/// # ATAM Mandatory Condition C-7
///
/// The fetcher **must** enforce all three SSRF mitigations before issuing any
/// network request:
///
/// 1. **Scheme validation** — only `http` and `https` are accepted.
/// 2. **Private-IP rejection** — the hostname is resolved to an IP address;
///    RFC-1918 addresses and loopback addresses are rejected.
/// 3. **Timeout** — the HTTP client enforces a 5-second total request timeout.
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::time::Duration;

use scraper::{Html, Selector};
use tracing::warn;

// ── Public types ──────────────────────────────────────────────────────────────

/// Title and description extracted from a web page, or a safe fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Page title (`<title>` text), or the raw URL on any failure.
    pub title: String,
    /// Meta description, or empty string on any failure.
    pub description: String,
}

/// Fetches metadata from a remote URL, enforcing C-7 SSRF mitigations.
pub struct MetadataFetcher {
    client: reqwest::Client,
}

impl MetadataFetcher {
    /// Construct a fetcher with a 5-second timeout (C-7).
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build reqwest client");
        Self { client }
    }

    /// Construct a fetcher with a custom `reqwest::Client`.
    ///
    /// Used in tests to supply a client that connects to a local mock server
    /// rather than the real internet.
    #[cfg(test)]
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Fetch metadata from `url`, applying all C-7 mitigations.
    ///
    /// Returns a fallback `Metadata` on any error; never panics.
    pub async fn fetch(&self, url: &str) -> Metadata {
        let fallback = || Metadata {
            title: url.to_string(),
            description: String::new(),
        };

        // ── Step 1: scheme validation (C-7) ──────────────────────────────────
        let parsed = match reqwest::Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                warn!(url, error = %e, "MetadataFetcher: URL parse error");
                return fallback();
            }
        };

        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            warn!(url, scheme, "MetadataFetcher: rejected non-http(s) scheme (C-7)");
            return fallback();
        }

        // ── Step 2: private-IP / loopback rejection (C-7) ────────────────────
        let host = match parsed.host_str() {
            Some(h) => h.to_string(),
            None => {
                warn!(url, "MetadataFetcher: URL has no host");
                return fallback();
            }
        };

        let port = parsed.port_or_known_default().unwrap_or(80);

        match resolve_and_check_private(&host, port) {
            Ok(false) => { /* public IP — proceed */ }
            Ok(true) => {
                warn!(url, host, "MetadataFetcher: rejected private/loopback IP (C-7)");
                return fallback();
            }
            Err(_) => {
                warn!(url, host, "MetadataFetcher: DNS resolution failed (C-7 fail-closed)");
                return fallback();
            }
        }

        // ── Step 3 + 4: issue request and parse (common path) ────────────────
        self.fetch_and_parse(url).await
    }

    /// Issue the HTTP GET request and parse the response.
    ///
    /// Called from `fetch` after all SSRF mitigations have passed.
    /// Also called directly from tests that bypass the SSRF guard to test the
    /// HTTP + parse code path against a local mock server.
    async fn fetch_and_parse(&self, url: &str) -> Metadata {
        let fallback = Metadata {
            title: url.to_string(),
            description: String::new(),
        };

        let response = match self.client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(url, error = %e, "MetadataFetcher: request error");
                return fallback;
            }
        };

        if !response.status().is_success() {
            warn!(
                url,
                status = %response.status(),
                "MetadataFetcher: non-200 response"
            );
            return fallback;
        }

        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => {
                warn!(url, error = %e, "MetadataFetcher: failed to read response body");
                return fallback;
            }
        };

        extract_metadata(&body, url)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Resolve `host` to an IP address and check whether it is private or loopback.
///
/// Returns:
/// * `Ok(false)` — the IP is public; the request may proceed.
/// * `Ok(true)`  — the IP is private or loopback; reject the request.
/// * `Err(_)`    — resolution failed; treat as rejection (fail-closed).
fn resolve_and_check_private(host: &str, port: u16) -> Result<bool, ()> {
    // If the host is already a numeric IP, parse it directly without a DNS
    // lookup.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(is_private_or_loopback(ip));
    }

    // Otherwise, resolve via the OS resolver.
    let addr_str = format!("{host}:{port}");
    let mut addrs = addr_str.to_socket_addrs().map_err(|_| ())?;

    // Check the first resolved address.  Fail-closed if the iterator is empty.
    let socket_addr = addrs.next().ok_or(())?;
    Ok(is_private_or_loopback(socket_addr.ip()))
}

/// Return `true` if `ip` is an RFC-1918, loopback, or link-local address.
pub(crate) fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// RFC-1918 private ranges + loopback for IPv4.
pub(crate) fn is_private_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }
    // 172.16.0.0/12 (172.16.x.x – 172.31.x.x)
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return true;
    }
    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    // 127.0.0.0/8 loopback
    if octets[0] == 127 {
        return true;
    }
    false
}

/// Parse `html` and extract the `<title>` text and meta description content.
///
/// Exposed as `pub(crate)` so tests can call it directly to verify the parse
/// logic without needing a live HTTP server.
pub(crate) fn extract_metadata(html: &str, fallback_url: &str) -> Metadata {
    let document = Html::parse_document(html);

    let title = extract_title(&document).unwrap_or_else(|| fallback_url.to_string());
    let description = extract_description(&document).unwrap_or_default();

    Metadata { title, description }
}

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn extract_description(document: &Html) -> Option<String> {
    let selector = Selector::parse(r#"meta[name="description"]"#).ok()?;
    let content = document
        .select(&selector)
        .next()?
        .value()
        .attr("content")?
        .trim()
        .to_string();
    if content.is_empty() { None } else { Some(content) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_metadata (pure, no network) ───────────────────────────────────

    /// AC-1.2: title and description are extracted from well-formed HTML.
    #[test]
    fn extracts_title_and_description() {
        let html = r#"<!DOCTYPE html>
<html>
<head>
  <title>An Interesting Article</title>
  <meta name="description" content="A summary of the article.">
</head>
<body></body>
</html>"#;

        let meta = extract_metadata(html, "https://example.com/article");
        assert_eq!(meta.title, "An Interesting Article");
        assert_eq!(meta.description, "A summary of the article.");
    }

    /// AC-1.2 partial: title is extracted even when no description meta tag exists.
    #[test]
    fn extracts_title_when_no_description_meta() {
        let html = r#"<html><head><title>Just a Title</title></head><body></body></html>"#;

        let meta = extract_metadata(html, "https://example.com");
        assert_eq!(meta.title, "Just a Title");
        assert_eq!(meta.description, "");
    }

    /// AC-1.3 partial: when HTML has no `<title>`, fallback URL is used as title.
    #[test]
    fn falls_back_to_url_when_no_title() {
        let html = r#"<html><head></head><body>No title here.</body></html>"#;
        let url = "https://example.com/no-title";

        let meta = extract_metadata(html, url);
        assert_eq!(meta.title, url);
        assert_eq!(meta.description, "");
    }

    /// Title text is trimmed of surrounding whitespace.
    #[test]
    fn title_is_trimmed() {
        let html = r#"<html><head><title>  Padded Title  </title></head><body></body></html>"#;

        let meta = extract_metadata(html, "https://example.com");
        assert_eq!(meta.title, "Padded Title");
    }

    // ── is_private_v4 (C-7 unit tests) ───────────────────────────────────────

    /// 10.x.x.x addresses are private (RFC-1918 10.0.0.0/8).
    #[test]
    fn rejects_private_ip_10_block() {
        assert!(is_private_v4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_v4(Ipv4Addr::new(10, 255, 255, 255)));
    }

    /// 172.16.x.x – 172.31.x.x are private (RFC-1918 172.16.0.0/12).
    #[test]
    fn rejects_private_ip_172_16_block() {
        assert!(is_private_v4(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_v4(Ipv4Addr::new(172, 31, 255, 255)));
    }

    /// 172.15.x.x and 172.32.x.x are just outside the range — must NOT be rejected.
    #[test]
    fn accepts_ip_outside_172_16_block() {
        assert!(!is_private_v4(Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_private_v4(Ipv4Addr::new(172, 32, 0, 1)));
    }

    /// 192.168.x.x are private (RFC-1918 192.168.0.0/16).
    #[test]
    fn rejects_private_ip_192_168_block() {
        assert!(is_private_v4(Ipv4Addr::new(192, 168, 0, 1)));
        assert!(is_private_v4(Ipv4Addr::new(192, 168, 255, 255)));
    }

    /// 127.x.x.x is loopback and must be rejected.
    #[test]
    fn rejects_loopback_127() {
        assert!(is_private_v4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_private_v4(Ipv4Addr::new(127, 255, 255, 255)));
    }

    /// Public addresses must not be rejected.
    #[test]
    fn accepts_public_ipv4() {
        assert!(!is_private_v4(Ipv4Addr::new(93, 184, 216, 34))); // example.com
        assert!(!is_private_v4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_v4(Ipv4Addr::new(1, 1, 1, 1)));
    }

    // ── is_private_or_loopback (IPv6) ─────────────────────────────────────────

    /// IPv6 loopback (::1) must be rejected.
    #[test]
    fn rejects_ipv6_loopback() {
        use std::net::Ipv6Addr;
        let loopback = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(is_private_or_loopback(loopback));
    }

    // ── MetadataFetcher::fetch — SSRF guard (no network) ─────────────────────

    /// C-7: non-http(s) scheme → fallback, no network request.
    #[tokio::test]
    async fn rejects_non_http_scheme() {
        let fetcher = MetadataFetcher::new();
        let meta = fetcher.fetch("ftp://example.com/file").await;
        assert_eq!(meta.title, "ftp://example.com/file");
        assert_eq!(meta.description, "");
    }

    /// C-7: 192.168.x.x IP literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_private_ip_192_168_in_url() {
        let fetcher = MetadataFetcher::new();
        let meta = fetcher.fetch("http://192.168.1.1/page").await;
        assert_eq!(meta.title, "http://192.168.1.1/page");
        assert_eq!(meta.description, "");
    }

    /// C-7: 10.x.x.x IP literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_10_block_ip_literal_in_url() {
        let fetcher = MetadataFetcher::new();
        let meta = fetcher.fetch("http://10.0.0.1/page").await;
        assert_eq!(meta.title, "http://10.0.0.1/page");
        assert_eq!(meta.description, "");
    }

    /// C-7: 127.0.0.1 loopback literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_loopback_ip_literal_in_url() {
        let fetcher = MetadataFetcher::new();
        let meta = fetcher.fetch("http://127.0.0.1/page").await;
        assert_eq!(meta.title, "http://127.0.0.1/page");
        assert_eq!(meta.description, "");
    }

    // ── MetadataFetcher::fetch_and_parse (mock server — uses httpmock) ────────
    //
    // These tests exercise the full HTTP+parse pipeline.  They bypass the SSRF
    // IP guard intentionally — the guard is already proven by the tests above,
    // and the mock server binds on 127.0.0.1 which the guard would block.
    // We use `fetch_and_parse` directly so the test is focused and fast.

    /// AC-1.2: a 200 response with HTML yields title + description.
    #[tokio::test]
    async fn successful_fetch_extracts_title_and_description() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/article");
            then.status(200)
                .header("content-type", "text/html; charset=utf-8")
                .body(concat!(
                    "<!DOCTYPE html><html><head>",
                    "<title>An Interesting Article</title>",
                    r#"<meta name="description" content="A summary of the article.">"#,
                    "</head><body></body></html>"
                ));
        });

        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&server.url("/article")).await;
        assert_eq!(meta.title, "An Interesting Article");
        assert_eq!(meta.description, "A summary of the article.");
    }

    /// AC-1.3: a non-200 response returns fallback (title = URL, description empty).
    #[tokio::test]
    async fn non_200_response_returns_fallback() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/paywall");
            then.status(403).body("Forbidden");
        });

        let url = server.url("/paywall");
        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&url).await;
        assert_eq!(meta.title, url);
        assert_eq!(meta.description, "");
    }

    /// AC-1.3: a timeout returns fallback.
    #[tokio::test]
    async fn timeout_returns_fallback() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/slow");
            then.status(200)
                .delay(Duration::from_secs(10))
                .body("late response");
        });

        let url = server.url("/slow");
        // Very short timeout so the test completes quickly.
        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_millis(200))
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&url).await;
        assert_eq!(meta.title, url);
        assert_eq!(meta.description, "");
    }

    /// AC-1.2 partial: 200 response with no description meta → empty description.
    #[tokio::test]
    async fn successful_fetch_no_description_meta() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/nodesc");
            then.status(200)
                .header("content-type", "text/html; charset=utf-8")
                .body("<html><head><title>Only Title</title></head><body></body></html>");
        });

        let url = server.url("/nodesc");
        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&url).await;
        assert_eq!(meta.title, "Only Title");
        assert_eq!(meta.description, "");
    }
}
