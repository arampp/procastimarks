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

use crate::domain::SaveBookmarkError;

/// Persist a new bookmark.
///
/// `tags_csv` is a comma-separated list of raw tags typed by the user.
/// Splitting, trimming, lowercasing, and deduplication are performed inside
/// `BookmarkRepository::insert`.
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
    let raw_tags: Vec<&str> = tags_csv
        .split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();

    match repo.insert(&url, &title, &description, &raw_tags, &comment) {
        Ok(InsertResult::Inserted(_)) => Ok(()),
        Ok(InsertResult::DuplicateUrl) => {
            Err(ServerFnError::WrappedServerError(SaveBookmarkError::DuplicateUrl))
        }
        Err(e) => Err(ServerFnError::WrappedServerError(
            SaveBookmarkError::Internal(e.to_string()),
        )),
    }
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

    repo.fetch_tags(&prefix)
        .map_err(|e| ServerFnError::<server_fn::error::NoCustomError>::ServerError(e.to_string()))
}

/// Return the configured `API_KEY` to authenticated clients.
///
/// This is used by the [`BookmarkletInstall`] component to pre-fill the
/// bookmarklet URL with the correct API key, so the owner can install it
/// from the home page without having to look up the key separately.
///
/// The endpoint is protected by the auth middleware — an unauthenticated
/// client cannot retrieve the key by calling this function.
#[server(GetApiKey, "/api")]
pub async fn get_api_key() -> Result<String, ServerFnError> {
    std::env::var("API_KEY").map_err(|_| {
        ServerFnError::<server_fn::error::NoCustomError>::ServerError(
            "API_KEY is not set".to_string(),
        )
    })
}
///
/// On any error (network, non-200, timeout, private IP) returns a `Metadata`
/// where `title` is the raw URL and `description` is empty (AC-1.3).
///
/// This is a thin server-side wrapper around `MetadataFetcher::fetch`.
#[server(FetchMetadata, "/api")]
pub async fn fetch_metadata(url: String) -> Result<(String, String), ServerFnError> {
    use crate::metadata::MetadataFetcher;

    let fetcher = MetadataFetcher::new();
    let m = fetcher.fetch(&url).await;
    Ok((m.title, m.description))
}

