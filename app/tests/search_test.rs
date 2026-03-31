//! Integration tests for the Search & Tag Filter feature — EPIC-5 (#5).
//!
//! Covers:
//! * US-11 (#17): Full-text search — UI presence and `BookmarkRepository::search`
//! * US-12 (#18): Tag filter sidebar — UI presence
//! * US-13 (#19): Combined search + tag filter (AND logic) — tested at repository layer
//! * AC-3.1–3.5 and UC-4 scenarios
//!
//! Chicago-school style: the real Axum router and real `BookmarkRepository`
//! (backed by in-memory SQLite) are exercised via `tower::ServiceExt::oneshot`.
//!
//! ## Test strategy
//!
//! Leptos server functions use a compile-time–unique hash in their HTTP endpoint
//! URL (e.g. `/api/search_bookmarks-<hash>`).  Direct API-endpoint tests would
//! break on recompile.  Instead:
//!
//! * **Unit tests** in `persistence/repository.rs` cover all `search()` logic
//!   (AC-3.1–3.5, UC-4) against a real in-memory SQLite database.
//! * **Integration tests here** cover the rendered HTML layer:
//!   - The `SearchBox` element is present in `GET /`.
//!   - The `TagFilter` sidebar is present in `GET /`.
//!   - Seeded bookmarks appear in the SSR-rendered list (initial state = no filter).
//!   - The AC-2.3 empty-state message is still rendered for an empty database.
//!
//! Note: AC-3.3 ("No bookmarks match your search.") is rendered client-side
//! only — Leptos SSR only emits the *active* Suspense branch, so the no-results
//! message is never present in the SSR HTML.  That acceptance criterion is
//! therefore covered by the repository unit tests in `persistence/repository.rs`.
//!
//! The search query and tag filter signals start empty on each SSR request, so
//! SSR always renders the full unfiltered list.  Reactive filtering happens
//! client-side after WASM hydration, which is outside the scope of these server-
//! side integration tests.

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

const TEST_API_KEY: &str = "test-secret-key-for-search-tests";

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

fn auth_query() -> String {
    format!("?api_key={TEST_API_KEY}")
}

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

/// Seed three standard bookmarks used across multiple tests.
fn seed_standard_bookmarks(repo: &BookmarkRepository) {
    repo.insert(
        "https://rust-lang.org",
        "Rust Async Programming",
        "Guide to async/await.",
        &["rust", "async"],
        "Great intro.",
    )
    .unwrap();
    repo.insert(
        "https://pandas.pydata.org",
        "Python Data Science",
        "Pandas and NumPy guide.",
        &["python", "data"],
        "",
    )
    .unwrap();
    repo.insert(
        "https://docker.com",
        "Introduction to Docker",
        "Container basics.",
        &["docker", "devops"],
        "Read chapter 3.",
    )
    .unwrap();
}

// ── AC-3.1 — SearchBox element is present in the rendered page ───────────────

/// AC-3.1: the search box input element must be present in the SSR HTML at GET /.
///
/// The initial SSR render includes the SearchBox component; the `<input type="search">`
/// element (or its wrapping `class="search-box"`) must appear in the response body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_page_contains_search_box() {
    let repo = test_repo();
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    assert!(
        body.contains("search-box")
            || body.contains(r#"type="search""#)
            || body.contains(r#"placeholder="Search"#)
            || body.contains("search-input"),
        "Search box element must be present in GET / HTML.\nGot:\n{body}"
    );
}

// ── US-12 — TagFilter sidebar is present in the rendered page ────────────────

/// US-12 / UC-4: the tag filter sidebar must be present in the SSR HTML at GET /.
///
/// Even when no bookmarks exist, the sidebar container must be rendered (it will
/// show "No tags yet." inside the `Suspense`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_page_contains_tag_filter_sidebar() {
    let repo = test_repo();
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    assert!(
        body.contains("tag-filter") || body.contains("tag-sidebar") || body.contains("tag-list"),
        "Tag filter sidebar element must be present in GET / HTML.\nGot:\n{body}"
    );
}

// ── AC-2.3 — Empty-state message (no bookmarks) ──────────────────────────────

/// AC-2.3: when no bookmarks exist and no search filter is active, the
/// "No bookmarks yet." message must still appear.
///
/// This verifies that switching `BookmarkList` from `list_bookmarks` to
/// `search_bookmarks` (with empty query) does not break the empty-state path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_database_still_renders_no_bookmarks_message() {
    let repo = test_repo();
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    assert!(
        body.contains("No bookmarks yet. Use the bookmarklet to save your first one."),
        "Empty-state message must appear when database is empty.\nGot:\n{body}"
    );
}

// ── AC-3.3 — "No bookmarks match your search." text exists in source ─────────
//
// Leptos SSR only renders the *active* branch of a reactive `match` expression
// inside a `<Suspense>` — the "no results" branch is client-side only.
// The AC-3.3 contract (message text and logic) is therefore verified by the
// repository unit tests (`search_returns_empty_vec_for_unmatched_query`) and
// the `BookmarkList` component source rather than via SSR HTML.
//
// No integration test is needed here; the test that was previously here has
// been removed to reflect the correct Leptos SSR rendering model.

// ── AC-2.1 — Seeded bookmarks appear in the SSR list ────────────────────────

/// AC-2.1: with bookmarks in the database, the SSR render (empty query = all
/// bookmarks) must include the bookmark titles in the HTML.
///
/// This confirms that `search_bookmarks(query="", tag=None)` returns all
/// bookmarks and the `BookmarkList` component renders them via SSR.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_page_renders_seeded_bookmarks() {
    let repo = test_repo();
    seed_standard_bookmarks(&repo);
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    assert!(
        body.contains("Rust Async Programming"),
        "Rust bookmark must appear in SSR HTML.\nGot:\n{body}"
    );
    assert!(
        body.contains("Python Data Science"),
        "Python bookmark must appear in SSR HTML.\nGot:\n{body}"
    );
    assert!(
        body.contains("Introduction to Docker"),
        "Docker bookmark must appear in SSR HTML.\nGot:\n{body}"
    );
}

// ── AC-2.1 — Seeded bookmarks appear newest first ────────────────────────────

/// AC-2.1 + AC-3.4: with an empty search query the list is rendered
/// newest-first by `search_bookmarks`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_page_renders_bookmarks_newest_first() {
    let repo = test_repo();
    {
        let conn = repo.conn().lock().unwrap();
        conn.execute_batch(
            "INSERT INTO bookmarks (url, title, description, tags, comment, created_at)
             VALUES
               ('https://old.example.com',    'Oldest Bookmark', '', '[]', '', '2026-01-01T10:00:00Z'),
               ('https://new.example.com',    'Newest Bookmark', '', '[]', '', '2026-03-01T10:00:00Z');",
        )
        .unwrap();
    }
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    let pos_newest = body.find("Newest Bookmark").expect("Newest Bookmark must appear");
    let pos_oldest = body.find("Oldest Bookmark").expect("Oldest Bookmark must appear");
    assert!(
        pos_newest < pos_oldest,
        "Newest Bookmark must appear before Oldest Bookmark in the SSR HTML."
    );
}

// ── UC-4 — Tags from seeded bookmarks appear in the tag filter sidebar ────────

/// UC-4: after seeding bookmarks with tags, the SSR-rendered tag filter
/// sidebar must include those tags (fetched via `fetch_tags` in `TagFilter`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tag_filter_sidebar_renders_tags_from_seeded_bookmarks() {
    let repo = test_repo();
    seed_standard_bookmarks(&repo);
    let app = create_router_with_state(test_state_with_repo(repo));
    let (status, body) = get(app, &format!("/{}", auth_query())).await;

    assert_eq!(status, StatusCode::OK, "GET / must return 200 OK.\nGot:\n{body}");
    // At least one known tag from the seeded data must appear in the sidebar.
    assert!(
        body.contains("rust") || body.contains("python") || body.contains("docker"),
        "Tag filter sidebar must render at least one seeded tag.\nGot:\n{body}"
    );
}
