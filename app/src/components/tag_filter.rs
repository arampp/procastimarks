/// TagFilter component — sidebar that lets the owner filter by a single tag.
///
/// # US-12 (#18) — Tag filter sidebar — narrow list by tag
///
/// Satisfies:
/// * UC-4 main scenario: clicking a tag activates the tag filter and the list
///   shows only bookmarks carrying that tag.
/// * UC-4 A1: clicking an active tag deactivates the filter and restores the
///   full/searched list.
/// * UC-4 A2: no-results message is shown when no bookmarks carry the tag
///   (rendered by `BookmarkList` which reads the `active_tag` signal).
/// * Active tag is visually highlighted via `class="tag-item tag-item--active"`.
/// * List updates reactively without a full page reload.
///
/// The component reads all distinct tags via the `fetch_tags` server function
/// (empty prefix → all tags) and writes the selected tag to an `active_tag`
/// signal owned by the parent `BookmarkList`.
use leptos::prelude::*;

use crate::server_fns::fetch_tags;

/// Sidebar that renders all existing tags and lets the owner toggle one.
///
/// Clicking a tag sets `active_tag` to `Some(tag)`.  Clicking the active tag
/// sets it back to `None`.
#[component]
pub fn TagFilter(
    /// Shared signal that holds the currently active tag (`None` = no filter).
    active_tag: RwSignal<Option<String>>,
) -> impl IntoView {
    // Fetch all tags (empty prefix = all) once on mount.
    let tags = Resource::new(|| (), |_| fetch_tags(String::new()));

    view! {
        <aside class="tag-filter tag-sidebar tag-list" aria-label="Filter by tag">
            <h2 class="tag-filter-title">"Tags"</h2>
            <Suspense fallback=|| view! { <p>"Loading tags…"</p> }>
                {move || {
                    tags.get().map(|result| match result {
                        Err(_) => view! {
                            <p class="error-message" role="alert">
                                "Could not load tags."
                            </p>
                        }.into_any(),

                        Ok(tag_list) if tag_list.is_empty() => view! {
                            <p class="tag-filter-empty">"No tags yet."</p>
                        }.into_any(),

                        Ok(tag_list) => {
                            let items = tag_list.into_iter().map(|tag| {
                                let tag_for_click = tag.clone();
                                let tag_for_class = tag.clone();
                                let tag_for_aria = tag.clone();
                                view! {
                                    <button
                                        class=move || {
                                            if active_tag.get().as_deref() == Some(&tag_for_class) {
                                                "tag-item tag-item--active"
                                            } else {
                                                "tag-item"
                                            }
                                        }
                                        aria-pressed=move || {
                                            (active_tag.get().as_deref() == Some(&tag_for_aria)).to_string()
                                        }
                                        on:click=move |_| {
                                            let current = active_tag.get();
                                            if current.as_deref() == Some(&tag_for_click) {
                                                active_tag.set(None);
                                            } else {
                                                active_tag.set(Some(tag_for_click.clone()));
                                            }
                                        }
                                    >
                                        {tag.clone()}
                                    </button>
                                }
                            }).collect_view();
                            view! {
                                <div class="tag-items">
                                    {items}
                                </div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </aside>
    }
}
