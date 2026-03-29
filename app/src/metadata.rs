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
/// The fetcher **must** enforce all four SSRF mitigations before issuing any
/// network request:
///
/// 1. **Scheme validation** — only `http` and `https` are accepted.
/// 2. **Private-IP rejection** — the hostname is resolved to an IP address;
///    RFC-1918 addresses and loopback addresses are rejected.
/// 3. **Timeout** — the HTTP client enforces a 5-second total request timeout.
/// 4. **Redirect policy** — automatic redirects are disabled so a 30x response
///    to a private address cannot bypass the private-IP guard.
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::time::Duration;

use scraper::{Html, Selector};
use tracing::warn;

// ── Public types ──────────────────────────────────────────────────────────────

/// Re-export so callers can use `crate::metadata::Metadata` without knowing
/// the domain module.
pub use crate::domain::Metadata;

/// Maximum response body size accepted by the metadata fetcher (1 MiB).
///
/// Responses with a `Content-Length` header exceeding this limit are rejected
/// before any bytes are read.  For responses without `Content-Length` the
/// streaming read is stopped as soon as the running total would exceed this
/// limit, and the fallback is returned immediately (no partial content parsed).
const MAX_BODY_BYTES: usize = 1024 * 1024; // 1 MiB

/// Fetches metadata from a remote URL, enforcing C-7 SSRF mitigations.
///
/// Cheaply cloneable — the underlying `reqwest::Client` uses an `Arc`-backed
/// connection pool, so cloning a `MetadataFetcher` shares the same pool.
#[derive(Clone)]
pub struct MetadataFetcher {
    client: reqwest::Client,
}

impl MetadataFetcher {
    /// Construct a fetcher with a 5-second timeout and no automatic redirects
    /// (C-7).
    ///
    /// Redirects are disabled so that a 30x response pointing to a private or
    /// loopback address cannot bypass the private-IP SSRF guard.
    ///
    /// Returns `Err` if the underlying TLS backend fails to initialise rather
    /// than panicking.
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build reqwest client: {e}"))?;
        Ok(Self { client })
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

        match resolve_and_check_private(&host, port).await {
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
    ///
    /// Responses whose `Content-Length` exceeds `MAX_BODY_BYTES`, or whose
    /// streamed body would exceed `MAX_BODY_BYTES`, are rejected and return the
    /// fallback immediately.  No partial content is parsed.
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

        // Reject up-front if Content-Length is declared and exceeds the cap.
        if let Some(len) = response.content_length() {
            if len > MAX_BODY_BYTES as u64 {
                warn!(
                    url,
                    content_length = len,
                    max = MAX_BODY_BYTES,
                    "MetadataFetcher: Content-Length exceeds limit, rejecting"
                );
                return fallback;
            }
        }

        // Stream the body and stop reading once MAX_BODY_BYTES is consumed.
        let mut buf: Vec<u8> = Vec::with_capacity(MAX_BODY_BYTES.min(64 * 1024));
        let mut stream = response;
        loop {
            match stream.chunk().await {
                Ok(Some(chunk)) => {
                    if buf.len() + chunk.len() > MAX_BODY_BYTES {
                        warn!(
                            url,
                            max = MAX_BODY_BYTES,
                            "MetadataFetcher: body exceeds size limit, rejecting"
                        );
                        return fallback;
                    }
                    buf.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => {
                    warn!(url, error = %e, "MetadataFetcher: failed to read response body");
                    return fallback;
                }
            }
        }

        // Convert bytes to string, replacing any invalid UTF-8 sequences.
        let body = String::from_utf8_lossy(&buf).into_owned();

        extract_metadata(&body, url)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Resolve `host` to IP addresses and check whether *any* is private or
/// loopback.
///
/// Resolves all addresses returned by the OS resolver so that a multi-A-record
/// host cannot bypass the guard by returning a mix of public and private IPs.
///
/// DNS resolution is run via `tokio::task::spawn_blocking` to avoid blocking
/// the async runtime's worker threads.
///
/// Returns:
/// * `Ok(false)` — all resolved IPs are public; the request may proceed.
/// * `Ok(true)`  — at least one IP is private or loopback; reject the request.
/// * `Err(_)`    — resolution failed or returned no addresses; treat as
///                 rejection (fail-closed).
async fn resolve_and_check_private(host: &str, port: u16) -> Result<bool, ()> {
    // If the host is already a numeric IP, parse it directly without a DNS
    // lookup.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(is_private_or_loopback(ip));
    }

    // Otherwise, resolve via the OS resolver on a blocking thread so we don't
    // stall the Tokio worker pool during DNS I/O.
    let addr_str = format!("{host}:{port}");
    let addrs = tokio::task::spawn_blocking(move || {
        addr_str.to_socket_addrs().map(|iter| iter.collect::<Vec<_>>())
    })
    .await
    .map_err(|_| ())?   // join error
    .map_err(|_| ())?;  // IO error

    if addrs.is_empty() {
        return Err(());
    }

    // Reject if *any* resolved address is private or loopback (TOCTOU
    // mitigation: we check all records, not just the first).
    let any_private = addrs.iter().any(|sa| is_private_or_loopback(sa.ip()));
    Ok(any_private)
}

/// Return `true` if `ip` is an RFC-1918, loopback, link-local, or ULA address.
pub(crate) fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => {
            // IPv6-mapped IPv4 addresses (::ffff:0:0/96) — e.g. ::ffff:127.0.0.1.
            // These present as IPv6 but resolve to an IPv4 address; apply the
            // same IPv4 private/loopback checks to prevent SSRF bypass.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            // ::1 loopback
            if v6.is_loopback() {
                return true;
            }
            let segments = v6.segments();
            // fe80::/10 link-local unicast
            if (segments[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // fc00::/7 unique local (ULA)
            if (segments[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            false
        }
    }
}

/// RFC-1918 private ranges + loopback + link-local for IPv4.
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
    // 169.254.0.0/16 link-local
    if octets[0] == 169 && octets[1] == 254 {
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

    /// IPv6 link-local fe80::/10 must be rejected.
    #[test]
    fn rejects_ipv6_link_local() {
        use std::net::Ipv6Addr;
        // fe80::1 is the canonical link-local address.
        let link_local: Ipv6Addr = "fe80::1".parse().unwrap();
        assert!(is_private_or_loopback(IpAddr::V6(link_local)));
    }

    /// IPv6 ULA fc00::/7 must be rejected.
    #[test]
    fn rejects_ipv6_ula() {
        use std::net::Ipv6Addr;
        // fd00::1 is inside the ULA fc00::/7 range.
        let ula: Ipv6Addr = "fd00::1".parse().unwrap();
        assert!(is_private_or_loopback(IpAddr::V6(ula)));
    }

    /// IPv4 link-local 169.254.0.0/16 must be rejected.
    #[test]
    fn rejects_ipv4_link_local() {
        assert!(is_private_v4(Ipv4Addr::new(169, 254, 0, 1)));
        assert!(is_private_v4(Ipv4Addr::new(169, 254, 255, 254)));
    }

    /// IPv6-mapped IPv4 loopback (::ffff:127.0.0.1) must be rejected.
    ///
    /// Without this check a URL like `http://[::ffff:127.0.0.1]/` would
    /// present as IPv6 and bypass the RFC-1918/loopback guards.
    #[test]
    fn rejects_ipv6_mapped_ipv4_loopback() {
        use std::net::Ipv6Addr;
        // ::ffff:127.0.0.1 — IPv6-mapped loopback.
        let mapped: Ipv6Addr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(
            is_private_or_loopback(IpAddr::V6(mapped)),
            "::ffff:127.0.0.1 must be rejected as loopback"
        );
    }

    /// IPv6-mapped RFC-1918 addresses must be rejected.
    #[test]
    fn rejects_ipv6_mapped_ipv4_private() {
        use std::net::Ipv6Addr;
        // ::ffff:10.0.0.1 — IPv6-mapped 10.x.x.x.
        let mapped_10: Ipv6Addr = "::ffff:10.0.0.1".parse().unwrap();
        assert!(
            is_private_or_loopback(IpAddr::V6(mapped_10)),
            "::ffff:10.0.0.1 must be rejected as RFC-1918"
        );
        // ::ffff:192.168.1.1 — IPv6-mapped 192.168.x.x.
        let mapped_192: Ipv6Addr = "::ffff:192.168.1.1".parse().unwrap();
        assert!(
            is_private_or_loopback(IpAddr::V6(mapped_192)),
            "::ffff:192.168.1.1 must be rejected as RFC-1918"
        );
    }

    /// IPv6-mapped public IPv4 addresses must NOT be rejected.
    #[test]
    fn accepts_ipv6_mapped_public_ipv4() {
        use std::net::Ipv6Addr;
        // ::ffff:93.184.216.34 — IPv6-mapped example.com.
        let mapped_public: Ipv6Addr = "::ffff:93.184.216.34".parse().unwrap();
        assert!(
            !is_private_or_loopback(IpAddr::V6(mapped_public)),
            "::ffff:93.184.216.34 must not be rejected (public address)"
        );
    }

    // ── MetadataFetcher::fetch — SSRF guard (no network) ─────────────────────

    /// C-7: non-http(s) scheme → fallback, no network request.
    #[tokio::test]
    async fn rejects_non_http_scheme() {
        let fetcher = MetadataFetcher::new().unwrap();
        let meta = fetcher.fetch("ftp://example.com/file").await;
        assert_eq!(meta.title, "ftp://example.com/file");
        assert_eq!(meta.description, "");
    }

    /// C-7: 192.168.x.x IP literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_private_ip_192_168_in_url() {
        let fetcher = MetadataFetcher::new().unwrap();
        let meta = fetcher.fetch("http://192.168.1.1/page").await;
        assert_eq!(meta.title, "http://192.168.1.1/page");
        assert_eq!(meta.description, "");
    }

    /// C-7: 10.x.x.x IP literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_10_block_ip_literal_in_url() {
        let fetcher = MetadataFetcher::new().unwrap();
        let meta = fetcher.fetch("http://10.0.0.1/page").await;
        assert_eq!(meta.title, "http://10.0.0.1/page");
        assert_eq!(meta.description, "");
    }

    /// C-7: 127.0.0.1 loopback literal → fallback, no network request.
    #[tokio::test]
    async fn rejects_loopback_ip_literal_in_url() {
        let fetcher = MetadataFetcher::new().unwrap();
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

    /// C-7: a response with Content-Length above the cap returns fallback.
    #[tokio::test]
    async fn oversized_content_length_returns_fallback() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/huge");
            then.status(200)
                .header("content-type", "text/html")
                // Declare a body larger than MAX_BODY_BYTES (1 MiB + 1 byte).
                .header("content-length", &(MAX_BODY_BYTES + 1).to_string())
                .body("x");
        });

        let url = server.url("/huge");
        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&url).await;
        assert_eq!(meta.title, url, "oversized Content-Length must return fallback");
        assert_eq!(meta.description, "");
    }

    /// C-7: a 301 redirect to an internal address must NOT be followed.
    #[tokio::test]
    async fn redirect_to_private_ip_is_not_followed() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        // The mock redirects to 10.0.0.1 — a private address.
        server.mock(|when, then| {
            when.method(GET).path("/redirect");
            then.status(301)
                .header("location", "http://10.0.0.1/secret");
        });

        let url = server.url("/redirect");
        // Use a client with redirects disabled (matching MetadataFetcher::new).
        let fetcher = MetadataFetcher::with_client(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        );

        let meta = fetcher.fetch_and_parse(&url).await;
        // 301 is not a 2xx success — fetch_and_parse must return the fallback.
        assert_eq!(meta.title, url, "redirect response must return fallback");
        assert_eq!(meta.description, "");
    }
}
