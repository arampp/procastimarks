//! Integration tests for the bookmark list view — US-10 (#16).
//!
//! Covers AC-2.1 through AC-2.4 using the real Axum router exercised via
//! `tower::ServiceExt::oneshot` (Chicago-school style).
//!
//! AC-2.4 (unauthenticated GET / → 401) is already covered by `auth_test.rs`
//! (`protected_route_without_credentials_returns_401`).  This file adds the
//! positive cases: seeded database renders titles, empty state message, and
//! ordered output.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use procastimarks::create_router_with_state;
use procastimarks::metadata::MetadataFetcher;
use procastimarks::middleware::auth::AppState;
use procastimarks::persistence::{schema::run_schema, BookmarkRepository};
use procastimarks::session;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const TEST_API_KEY: &str = "test-secret-key-for-list-tests";

/// Build an in-memory `BookmarkRepository` with the schema applied.
fn test_repo() -> BookmarkRepository {
    let conn = Connection::open_in_memory().expect("in-memory DB must open");
    run_schema(&conn).expect("schema init must succeed");
    BookmarkRepository::new(Arc::new(Mutex::new(conn)))
}

fn test_state_with_repo(repo: BookmarkRepository) -> AppState {
    AppState {
        api_key: Arc::from(TEST_API_KEY),
        sessions: session::new_store(),
        repo,
        metadata_fetcher: MetadataFetcher::new().expect("MetadataFetcher must build"),
    }
}

fn auth_header() -> String {
    format!("?api_key={TEST_API_KEY}")
}

/// Perform a GET request to the given path and return (status, body_string).
async fn get(app: axum::Router, path: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    (status, body)
}

// ── AC-2.4 — Unauthenticated GET / returns 401 ───────────────────────────────

/// AC-2.4: a request to GET / without credentials must return HTTP 401.
#[tokio::test]
async fn unauthenticated_get_root_returns_401() {
    let repo = test_repo();
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, _) = get(app, "/").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET / without credentials must return 401"
    );
}

// ── AC-2.3 — Empty state message ─────────────────────────────────────────────

/// AC-2.3: when no bookmarks exist the response body must contain the exact
/// empty-state message.
#[tokio::test]
async fn empty_database_renders_empty_state_message() {
    let repo = test_repo();
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_header())).await;

    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert!(
        body.contains("No bookmarks yet. Use the bookmarklet to save your first one."),
        "Empty-state message must appear in the response body.\nGot:\n{body}"
    );
}

// ── AC-2.1 — Reverse-chronological order ─────────────────────────────────────

/// AC-2.1: bookmarks are rendered newest first.
///
/// We insert three bookmarks with explicit `created_at` timestamps and assert
/// the title of the newest one appears before the oldest one in the HTML.
#[tokio::test]
async fn bookmarks_are_rendered_newest_first() {
    let repo = test_repo();
    {
        let conn_guard = repo.conn().lock().unwrap();
        conn_guard
            .execute_batch(
                "INSERT INTO bookmarks (url, title, description, tags, comment, created_at)
                 VALUES
                   ('https://old.example.com',    'Oldest Article', '', '[]', '', '2026-01-01T10:00:00Z'),
                   ('https://middle.example.com', 'Middle Article',  '', '[]', '', '2026-02-01T10:00:00Z'),
                   ('https://new.example.com',    'Newest Article',  '', '[]', '', '2026-03-01T10:00:00Z');",
            )
            .unwrap();
    }

    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_header())).await;

    assert_ne!(status, StatusCode::UNAUTHORIZED);

    let pos_newest = body.find("Newest Article").expect("Newest Article must appear");
    let pos_oldest = body.find("Oldest Article").expect("Oldest Article must appear");
    assert!(
        pos_newest < pos_oldest,
        "Newest Article must appear before Oldest Article in the HTML.\nHTML:\n{body}"
    );
}

// ── AC-2.2 — Entry fields ─────────────────────────────────────────────────────

/// AC-2.2: each bookmark entry must contain the title as a hyperlink,
/// description excerpt, all tags, and the creation date.
#[tokio::test]
async fn bookmark_entry_contains_required_fields() {
    let repo = test_repo();
    repo.insert(
        "https://example.com/article",
        "An Interesting Article",
        "A summary of the article.",
        &["rust", "programming"],
        "",
    )
    .unwrap();

    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_header())).await;

    assert_ne!(status, StatusCode::UNAUTHORIZED);

    // Title rendered as a hyperlink.
    assert!(
        body.contains("An Interesting Article"),
        "Title must appear in the HTML.\nGot:\n{body}"
    );
    assert!(
        body.contains("https://example.com/article"),
        "URL must appear as a hyperlink href.\nGot:\n{body}"
    );

    // Description excerpt.
    assert!(
        body.contains("A summary of the article."),
        "Description must appear in the HTML.\nGot:\n{body}"
    );

    // Tags.
    assert!(
        body.contains("rust"),
        "Tag 'rust' must appear.\nGot:\n{body}"
    );
    assert!(
        body.contains("programming"),
        "Tag 'programming' must appear.\nGot:\n{body}"
    );

    // Creation date — the timestamp is generated at insert time; just assert
    // that a 4-digit year is present as a proxy for the date field.
    assert!(
        body.contains("2026"),
        "Creation date (year 2026) must appear.\nGot:\n{body}"
    );
}
