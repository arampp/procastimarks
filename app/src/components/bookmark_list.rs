/// Bookmark list component — the primary read surface of Procastimarks.
///
/// # US-10 (#16) — Bookmark list view
///
/// Satisfies:
/// * AC-2.1: `list_bookmarks` returns bookmarks newest first;
///   `<For>` renders them in that order.
/// * AC-2.2: `BookmarkEntry` shows title as a hyperlink, description excerpt,
///   all tags, and the creation date.
/// * AC-2.3: when the server returns an empty `Vec`, the empty-state message
///   "No bookmarks yet. Use the bookmarklet to save your first one." is rendered.
use leptos::prelude::*;

use crate::domain::Bookmark;
use crate::server_fns::list_bookmarks;

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
/// Loads all bookmarks via the `list_bookmarks` server function and renders
/// them newest first.  When the list is empty, shows the AC-2.3 empty-state
/// message.
#[component]
pub fn BookmarkList() -> impl IntoView {
    let bookmarks = Resource::new(|| (), |_| list_bookmarks());

    view! {
        <main>
            <h1>"Procastimarks"</h1>

            <Suspense fallback=|| view! { <p>"Loading bookmarks…"</p> }>
                {move || {
                    bookmarks.get().map(|result| match result {
                        Err(_) => view! {
                            <p class="error-message" role="alert">
                                "An error occurred while loading bookmarks."
                            </p>
                        }.into_any(),

                        Ok(items) if items.is_empty() => view! {
                            <p class="empty-state">
                                "No bookmarks yet. Use the bookmarklet to save your first one."
                            </p>
                        }.into_any(),

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
        </main>
    }
}
