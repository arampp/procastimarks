/// Leptos server functions — thin HTTP boundary for bookmark operations.
///
/// Each function is compiled for both targets:
///
/// * **Server** — the function body runs; the `BookmarkRepository` is
///   obtained from the Leptos request context, which is injected at router
///   construction time via `leptos_routes_with_context`.
/// * **WASM** — the `#[server]` macro replaces the body with a generated
///   HTTP call to the corresponding `/api/…` endpoint; the original body is
///   not compiled for `wasm32`.
///
/// # US-11 (#17) — Full-text search
///
/// `search_bookmarks` satisfies:
/// * AC-3.1: keyword filters the list reactively.
/// * AC-3.2: search matches across title, description, comment, and tags.
/// * AC-3.3: no match returns an empty vec (UI shows "No bookmarks match your search.").
/// * AC-3.4: empty query returns all bookmarks newest first.
/// * AC-3.5: query + tag applied with AND logic.
///
/// # US-10 (#16) — Bookmark List View
///
/// `list_bookmarks` satisfies:
/// * AC-2.1: bookmarks returned newest first.
/// * AC-2.2: all fields (url, title, description, tags, created_at) included.
///
/// # US-9 (#15) — Save Bookmark
///
/// `save_bookmark` satisfies:
/// * AC-1.4: all fields persisted; redirect is handled by the caller (form).
/// * AC-1.5: tags and comment may be empty.
/// * AC-1.6: duplicate URL returns `SaveBookmarkError::DuplicateUrl`.
///
/// # US-8 (#14) — Add-bookmark form server-side helpers
///
/// `fetch_metadata` satisfies:
/// * AC-1.2: title and description fetched from target URL.
/// * AC-1.3: on fetch failure title = URL, description = empty.
///
/// `fetch_tags` satisfies:
/// * AC-4.1: prefix query returns matching tags in alphabetical order.
/// * AC-4.3: no match returns an empty list.
use leptos::prelude::*;

use crate::domain::{Bookmark, SaveBookmarkError};

/// Return all saved bookmarks ordered newest first.
///
/// Delegates to `BookmarkRepository::list` on a blocking thread to avoid
/// stalling the Tokio worker pool.
///
/// Satisfies AC-2.1 (reverse-chronological order) and AC-2.2 (all fields).
#[server(ListBookmarks, "/api")]
pub async fn list_bookmarks() -> Result<Vec<Bookmark>, ServerFnError> {
    use crate::persistence::BookmarkRepository;

    let repo = use_context::<BookmarkRepository>().ok_or_else(|| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "BookmarkRepository not found in context".to_string(),
        )
    })?;

    tokio::task::spawn_blocking(move || {
        repo.list().map_err(|e| {
            tracing::error!(error = %e, "list_bookmarks: repository error");
            ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "list_bookmarks: spawn_blocking join error");
        ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
    })?
}

/// Search bookmarks by full-text query and/or tag filter.
///
/// Delegates to `BookmarkRepository::search` on a blocking thread.
///
/// * An empty `query` with `tag = None` returns all bookmarks newest first
///   (AC-3.4 — clearing the search box restores the full list).
/// * A non-empty `query` runs an FTS5 `MATCH` search across title, description,
///   comment, and tags fields (AC-3.1, AC-3.2).
/// * An empty `query` with `tag = Some(t)` filters by tag only (UC-4).
/// * Both non-empty applies AND logic (AC-3.5).
#[server(SearchBookmarks, "/api")]
pub async fn search_bookmarks(
    query: String,
    tag: Option<String>,
) -> Result<Vec<Bookmark>, ServerFnError> {
    use crate::persistence::BookmarkRepository;

    let repo = use_context::<BookmarkRepository>().ok_or_else(|| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "BookmarkRepository not found in context".to_string(),
        )
    })?;

    tokio::task::spawn_blocking(move || {
        repo.search(&query, tag.as_deref()).map_err(|e| {
            tracing::error!(error = %e, "search_bookmarks: repository error");
            ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "search_bookmarks: spawn_blocking join error");
        ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
    })?
}


/// Persist a new bookmark.
///
/// `tags_csv` is a comma-separated list of raw tags typed by the user.
/// This function splits the CSV string and trims each tag before passing
/// the slice to `BookmarkRepository::insert`, which performs lowercasing
/// and deduplication.
///
/// The caller (form component) is responsible for redirecting the user after
/// a successful save.
///
/// # Errors
///
/// * `SaveBookmarkError::DuplicateUrl` — a bookmark with the same URL already
///   exists.
/// * `SaveBookmarkError::Internal(msg)` — an unexpected database error.
#[server(SaveBookmark, "/api")]
pub async fn save_bookmark(
    url: String,
    title: String,
    description: String,
    /// Comma-separated tag string as typed by the user; splitting and
    /// normalisation are delegated to the repository layer.
    tags_csv: String,
    comment: String,
) -> Result<(), ServerFnError<SaveBookmarkError>> {
    use crate::persistence::{BookmarkRepository, InsertResult};

    let repo = use_context::<BookmarkRepository>().ok_or_else(|| {
        ServerFnError::ServerError("BookmarkRepository not found in context".to_string())
    })?;

    // Split the CSV tag string into individual raw tags.
    let raw_tags: Vec<String> = tags_csv
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    // `BookmarkRepository::insert` acquires a `std::sync::Mutex`, which
    // blocks.  Run it on a dedicated blocking thread so we don't stall the
    // Tokio worker pool under concurrent requests.
    tokio::task::spawn_blocking(move || {
        match repo.insert(&url, &title, &description, &raw_tags, &comment) {
            Ok(InsertResult::Inserted(_)) => Ok(()),
            Ok(InsertResult::DuplicateUrl) => {
                Err(ServerFnError::WrappedServerError(SaveBookmarkError::DuplicateUrl))
            }
            Err(e) => {
                // Log the detail server-side; the wire format of
                // `SaveBookmarkError::Internal` is the opaque token
                // "Internal" so the raw DB error never reaches the browser.
                tracing::error!(error = %e, "save_bookmark: database error");
                Err(ServerFnError::WrappedServerError(
                    SaveBookmarkError::Internal(e.to_string()),
                ))
            }
        }
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "save_bookmark: spawn_blocking join error");
        ServerFnError::WrappedServerError(SaveBookmarkError::Internal(e.to_string()))
    })?
}

/// Return all stored tags whose value starts with `prefix`, sorted
/// alphabetically.
///
/// Used by the tag-autocomplete widget (AC-4.1).  An empty `prefix` returns
/// all tags.
#[server(FetchTags, "/api")]
pub async fn fetch_tags(prefix: String) -> Result<Vec<String>, ServerFnError> {
    use crate::persistence::BookmarkRepository;

    let repo = use_context::<BookmarkRepository>().ok_or_else(|| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "BookmarkRepository not found in context".to_string(),
        )
    })?;

    // `fetch_tags` acquires a `std::sync::Mutex`; run on a blocking thread.
    tokio::task::spawn_blocking(move || {
        repo.fetch_tags(&prefix).map_err(|e| {
            // Log the full detail server-side; never send internal DB errors
            // over the wire to the browser.
            tracing::error!(error = %e, "fetch_tags: repository error");
            ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "fetch_tags: spawn_blocking join error");
        ServerFnError::<server_fn::error::NoCustomError>::ServerError("Internal".to_string())
    })?
}

/// Return the configured `API_KEY` to authenticated clients.
///
/// This is used by the [`BookmarkletInstall`] component to pre-fill the
/// bookmarklet URL with the correct API key, so the owner can install it
/// from the home page without having to look up the key separately.
///
/// The endpoint is protected by the auth middleware — an unauthenticated
/// client cannot retrieve the key by calling this function.
///
/// The key is sourced from the Leptos request context, where it is injected
/// at router construction time alongside the `BookmarkRepository`.  This
/// ensures tests that use `create_router_with_state` with an explicit
/// `AppState` get the same key the middleware uses, without any env reads.
#[server(GetApiKey, "/api")]
pub async fn get_api_key() -> Result<String, ServerFnError> {
    use std::sync::Arc;

    let api_key = use_context::<Arc<str>>().ok_or_else(|| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "API_KEY not found in context".to_string(),
        )
    })?;
    Ok(api_key.to_string())
}
/// Fetch the `<title>` and meta description from a remote URL.
///
/// On any error (network, non-200, timeout, private IP) returns a `Metadata`
/// where `title` is the raw URL and `description` is empty (AC-1.3).
///
/// This is a thin server-side wrapper around `MetadataFetcher::fetch`.
/// The `MetadataFetcher` (and its underlying `reqwest::Client`) is built once
/// at startup and injected into the Leptos context — it is **not** constructed
/// on each call.
#[server(FetchMetadata, "/api")]
pub async fn fetch_metadata(url: String) -> Result<crate::domain::Metadata, ServerFnError> {
    use crate::metadata::MetadataFetcher;

    let fetcher = use_context::<MetadataFetcher>().ok_or_else(|| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "MetadataFetcher not found in context".to_string(),
        )
    })?;
    Ok(fetcher.fetch(&url).await)
}

