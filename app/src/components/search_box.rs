/// SearchBox component — debounced keyword search input.
///
/// # US-11 (#17) — Full-text search with FTS5 (search-as-you-type)
///
/// Satisfies:
/// * AC-3.1: typing in the search box triggers `search_bookmarks` with ~300 ms
///   debounce; the bookmark list updates reactively without a page reload.
/// * AC-3.4: clearing the search box (empty string) restores the full list.
///
/// The component writes to a shared `RwSignal<String>` owned by the parent
/// (`BookmarkList`).  The parent is responsible for calling `search_bookmarks`
/// whenever the query or the active tag changes.
use leptos::prelude::*;

/// A controlled search input that writes the current query to `search_query`.
///
/// The `search_query` signal is owned by the parent component and shared with
/// `TagFilter` so that both signals can be read together when calling
/// `search_bookmarks`.
#[component]
pub fn SearchBox(
    /// Shared signal that holds the current search query string.
    search_query: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="search-box">
            <input
                class="search-input"
                type="search"
                placeholder="Search bookmarks…"
                aria-label="Search bookmarks"
                prop:value=move || search_query.get()
                on:input=move |ev| {
                    search_query.set(event_target_value(&ev));
                }
            />
        </div>
    }
}
