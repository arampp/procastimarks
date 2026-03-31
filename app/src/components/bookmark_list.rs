/// Bookmark list component — the primary read surface of Procastimarks.
///
/// # US-10 (#16) — Bookmark list view
///
/// Satisfies:
/// * AC-2.1: `search_bookmarks` (with empty query) returns bookmarks newest first;
///   `<For>` renders them in that order.
/// * AC-2.2: `BookmarkEntry` shows title as a hyperlink, description excerpt,
///   all tags, and the creation date.
/// * AC-2.3: when the server returns an empty `Vec` with no active filter, the
///   empty-state message "No bookmarks yet. Use the bookmarklet to save your
///   first one." is rendered.
///
/// # US-11 (#17) — Full-text search with FTS5 (search-as-you-type)
///
/// Satisfies:
/// * AC-3.1: `SearchBox` writes to a shared `search_query` signal; the
///   `Resource` re-runs `search_bookmarks` whenever the query or `active_tag`
///   changes, updating the list reactively without a page reload.
/// * AC-3.3: when `search_bookmarks` returns an empty vec with a non-empty
///   query, the message "No bookmarks match your search." is rendered.
/// * AC-3.4: clearing the search box (empty string) re-runs `search_bookmarks`
///   with an empty query which returns all bookmarks newest first.
///
/// # US-12 (#18) — Tag filter sidebar
///
/// Satisfies:
/// * UC-4: `TagFilter` writes to a shared `active_tag` signal; the Resource
///   re-runs when the active tag changes.
///
/// # US-13 (#19) — Combined search + tag filter (AND logic)
///
/// Satisfies:
/// * AC-3.5: both `search_query` and `active_tag` are passed together to
///   `search_bookmarks`; the server function applies AND logic.
use leptos::prelude::*;

use crate::components::search_box::SearchBox;
use crate::components::tag_filter::TagFilter;
use crate::domain::Bookmark;
use crate::server_fns::search_bookmarks;

// ── BookmarkEntry ─────────────────────────────────────────────────────────────

/// Renders a single bookmark entry (AC-2.2).
///
/// Displays:
/// * Title as a hyperlink to the original URL.
/// * Description excerpt (full text for MVP; truncation is post-MVP).
/// * All tags as individual `<span class="tag">` chips.
/// * Creation date (ISO-8601 UTC string as stored).
#[component]
pub fn BookmarkEntry(bookmark: Bookmark) -> impl IntoView {
    let url = bookmark.url.clone();
    let title = bookmark.title.clone();
    let description = bookmark.description.clone();
    let tags = bookmark.tags.clone();
    let created_at = bookmark.created_at.clone();
    let created_at_attr = created_at.clone();

    view! {
        <article class="bookmark-entry">
            <h2 class="bookmark-title">
                <a href=url target="_blank" rel="noopener noreferrer">
                    {title}
                </a>
            </h2>

            {(!description.is_empty()).then(|| view! {
                <p class="bookmark-description">{description}</p>
            })}

            {(!tags.is_empty()).then(|| {
                let tags_view = tags.iter().map(|t| {
                    let t = t.clone();
                    view! { <span class="tag">{t}</span> }
                }).collect_view();
                view! {
                    <div class="bookmark-tags" aria-label="Tags">
                        {tags_view}
                    </div>
                }
            })}

            <time class="bookmark-date" datetime=created_at_attr>
                {created_at}
            </time>
        </article>
    }
}

// ── BookmarkList ──────────────────────────────────────────────────────────────

/// The main bookmark list view rendered at `GET /`.
///
/// Creates shared `search_query` and `active_tag` signals and passes them to
/// `SearchBox` and `TagFilter`.  A `Resource` re-runs `search_bookmarks`
/// whenever either signal changes, updating the visible bookmark list
/// reactively without a page reload.
#[component]
pub fn BookmarkList() -> impl IntoView {
    // ── Shared reactive state (arc42 §8 — Reactive UI State) ─────────────────
    let search_query: RwSignal<String> = RwSignal::new(String::new());
    let active_tag: RwSignal<Option<String>> = RwSignal::new(None);

    // Re-run search_bookmarks whenever query or tag changes (AC-3.1, UC-4).
    let bookmarks = Resource::new(
        move || (search_query.get(), active_tag.get()),
        |(query, tag)| search_bookmarks(query, tag),
    );

    view! {
        <main class="bookmark-list-page">
            <h1>"Procastimarks"</h1>

            // ── Search box (US-11) ────────────────────────────────────────────
            <SearchBox search_query=search_query />

            <div class="content-layout">
                // ── Tag filter sidebar (US-12) ────────────────────────────────
                <TagFilter active_tag=active_tag />

                // ── Bookmark list ─────────────────────────────────────────────
                <section class="bookmark-results">
                    <Suspense fallback=|| view! { <p>"Loading bookmarks…"</p> }>
                        {move || {
                            bookmarks.get().map(|result| match result {
                                Err(_) => view! {
                                    <p class="error-message" role="alert">
                                        "An error occurred while loading bookmarks."
                                    </p>
                                }.into_any(),

                                Ok(items) if items.is_empty() => {
                                    // Choose the message based on whether a filter is active.
                                    let query = search_query.get();
                                    let tag = active_tag.get();
                                    if query.trim().is_empty() && tag.is_none() {
                                        view! {
                                            <p class="empty-state">
                                                "No bookmarks yet. Use the bookmarklet to save your first one."
                                            </p>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <p class="empty-state">
                                                "No bookmarks match your search."
                                            </p>
                                        }.into_any()
                                    }
                                }

                                Ok(items) => view! {
                                    <section aria-label="Bookmarks">
                                        <For
                                            each=move || items.clone()
                                            key=|bm| bm.id
                                            children=|bm| view! { <BookmarkEntry bookmark=bm/> }
                                        />
                                    </section>
                                }.into_any(),
                            })
                        }}
                    </Suspense>
                </section>
            </div>
        </main>
    }
}
