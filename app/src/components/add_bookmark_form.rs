/// Add-bookmark form component and TagInput sub-component.
///
/// # US-8 (#14) — Add-bookmark form with tag input and autocomplete
///
/// Satisfies:
/// * AC-1.1: `/add?url=…` renders the form with the URL pre-filled.
/// * AC-1.2: `fetch_metadata` is called on mount; title and description
///   are pre-filled from the response.
/// * AC-1.3: on fetch failure, title = raw URL, description = empty.
/// * AC-1.4: on success `save_bookmark` redirects to `/`.
/// * AC-1.5: tags and comment are optional.
/// * AC-1.6: duplicate URL shows inline error "This URL is already saved."
/// * AC-4.1: typing in TagInput calls `fetch_tags`; matching tags appear in
///   a dropdown.
/// * AC-4.2: selecting a suggestion appends the lowercase tag and clears the
///   input field.
/// * AC-4.3: when no tags match the prefix the dropdown is hidden.
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::domain::SaveBookmarkError;
use crate::server_fns::{fetch_metadata, fetch_tags};

// ── TagInput ─────────────────────────────────────────────────────────────────

/// A text input that displays tag autocomplete suggestions.
///
/// The parent controls the full tag list via `tags` / `set_tags`.
#[component]
pub fn TagInput(
    /// The current accumulated tag list (owned by the parent form).
    tags: RwSignal<Vec<String>>,
) -> impl IntoView {
    // Raw text the user is currently typing (the "next tag" input).
    let prefix = RwSignal::new(String::new());

    // Reactive resource: re-fetches whenever `prefix` changes.
    // Short-circuit to an empty list when the prefix is empty so we don't
    // issue a needless server/DB round-trip on initial render and after a
    // tag is added (the dropdown is hidden anyway when the prefix is empty).
    let suggestions = Resource::new(
        move || prefix.get(),
        |p| async move {
            if p.is_empty() {
                Vec::new()
            } else {
                fetch_tags(p).await.unwrap_or_default()
            }
        },
    );

    // Add a tag to the list when the user selects a suggestion.
    let add_tag = move |tag: String| {
        tags.update(|list| {
            if !list.contains(&tag) {
                list.push(tag);
            }
        });
        prefix.set(String::new());
    };

    view! {
        <div class="tag-input">
            // Existing tags as removable chips.
            <div class="tag-chips">
                <For
                    each=move || tags.get()
                    key=|t| t.clone()
                    children=move |tag| {
                        let tag_clone = tag.clone();
                        view! {
                            <span class="tag-chip">
                                {tag.clone()}
                                <button
                                    type="button"
                                    aria-label=format!("Remove tag {tag}")
                                    on:click=move |_| {
                                        tags.update(|list| list.retain(|t| t != &tag_clone));
                                    }
                                >
                                    "×"
                                </button>
                            </span>
                        }
                    }
                />
            </div>

            // Text input for typing the next tag.
            <input
                type="text"
                id="tag-input-field"
                name="tag_input_field"
                autocomplete="off"
                placeholder="Add a tag…"
                prop:value=move || prefix.get()
                on:input=move |ev| {
                    prefix.set(event_target_value(&ev));
                }
                on:keydown=move |ev| {
                    // Enter key inserts the raw typed value as a new tag.
                    if ev.key() == "Enter" {
                        ev.prevent_default();
                        let raw = prefix.get_untracked();
                        let trimmed = raw.trim().to_lowercase();
                        if !trimmed.is_empty() {
                            tags.update(|list| {
                                if !list.contains(&trimmed) {
                                    list.push(trimmed);
                                }
                            });
                            prefix.set(String::new());
                        }
                    }
                }
            />

            // Autocomplete dropdown — hidden when no suggestions.
            <Suspense fallback=|| ()>
                {move || {
                    let sugs = suggestions.get().unwrap_or_default();
                    let current_prefix = prefix.get();
                    // Hide the dropdown if there is nothing to show.
                    if sugs.is_empty() || current_prefix.is_empty() {
                        return view! { <ul class="tag-suggestions" style="display:none"></ul> }.into_any();
                    }
                    view! {
                        <ul class="tag-suggestions" role="listbox">
                            <For
                                each=move || sugs.clone()
                                key=|t| t.clone()
                                children=move |tag| {
                                    let t = tag.clone();
                                    view! {
                                        <li
                                            role="option"
                                            on:mousedown=move |ev| {
                                                ev.prevent_default();
                                                add_tag(t.clone());
                                            }
                                        >
                                            {tag}
                                        </li>
                                    }
                                }
                            />
                        </ul>
                    }.into_any()
                }}
            </Suspense>
        </div>
    }
}

// ── AddBookmarkForm ───────────────────────────────────────────────────────────

/// The capture form rendered at `/add?url=…`.
///
/// On mount it calls `fetch_metadata` to pre-fill title and description.
/// On submit it calls `save_bookmark`; on success it navigates to `/`.
/// On `DuplicateUrl` error it shows an inline message.
#[component]
pub fn AddBookmarkForm(
    /// The URL to save, taken from the `?url=` query parameter.
    url: String,
) -> impl IntoView {
    // ── Form field signals ────────────────────────────────────────────────────
    let url_signal = RwSignal::new(url.clone());
    let title = RwSignal::new(url.clone()); // default = URL until metadata loads
    let description = RwSignal::new(String::new());
    let tags: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let comment = RwSignal::new(String::new());
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);

    // ── Metadata prefetch (AC-1.2, AC-1.3) ───────────────────────────────────
    let url_for_resource = url.clone();
    let _metadata = Resource::new(
        move || url_for_resource.clone(),
        move |u| {
            let title_w = title;
            let description_w = description;
            async move {
                match fetch_metadata(u).await {
                    Ok(m) => {
                        title_w.set(m.title);
                        description_w.set(m.description);
                    }
                    Err(_) => {
                        // AC-1.3: title stays as the raw URL; description stays empty.
                    }
                }
            }
        },
    );

    // ── Save action (AC-1.4, AC-1.5, AC-1.6) ─────────────────────────────────
    let save_action = ServerAction::<crate::server_fns::SaveBookmark>::new();

    // Navigate to "/" after a successful save.
    let navigate = use_navigate();
    Effect::new(move |_| {
        if let Some(result) = save_action.value().get() {
            match result {
                Ok(()) => {
                    navigate("/", Default::default());
                }
                Err(ref e) => {
                    let msg = match e {
                        leptos::prelude::ServerFnError::WrappedServerError(
                            SaveBookmarkError::DuplicateUrl,
                        ) => "This URL is already saved.".to_string(),
                        leptos::prelude::ServerFnError::WrappedServerError(
                            SaveBookmarkError::Internal(_),
                        ) => "An unexpected error occurred while saving the bookmark.".to_string(),
                        other => format!("Error: {other}"),
                    };
                    error_msg.set(Some(msg));
                }
            }
        }
    });

    view! {
        <main>
            <h1>"Add Bookmark"</h1>

            // Inline error message (AC-1.6).
            {move || error_msg.get().map(|msg| view! {
                <p class="error-message" role="alert">{msg}</p>
            })}

            <form
                on:submit=move |ev| {
                    ev.prevent_default();
                    // Clear any previous error.
                    error_msg.set(None);
                    // Build the tags CSV.
                    let tags_csv = tags.get_untracked().join(",");
                    save_action.dispatch(crate::server_fns::SaveBookmark {
                        url: url_signal.get_untracked(),
                        title: title.get_untracked(),
                        description: description.get_untracked(),
                        tags_csv,
                        comment: comment.get_untracked(),
                    });
                }
            >
                // URL — pre-filled, read-only (AC-1.1).
                <div class="form-field">
                    <label for="url">"URL"</label>
                    <input
                        type="url"
                        id="url"
                        name="url"
                        readonly=true
                        prop:value=move || url_signal.get()
                    />
                </div>

                // Title — pre-filled from metadata (AC-1.2).
                <div class="form-field">
                    <label for="title">"Title"</label>
                    <input
                        type="text"
                        id="title"
                        name="title"
                        required=true
                        prop:value=move || title.get()
                        on:input=move |ev| title.set(event_target_value(&ev))
                    />
                </div>

                // Description — pre-filled from metadata (AC-1.2).
                <div class="form-field">
                    <label for="description">"Description"</label>
                    <textarea
                        id="description"
                        name="description"
                        prop:value=move || description.get()
                        on:input=move |ev| description.set(event_target_value(&ev))
                    />
                </div>

                // Tags — optional, with autocomplete (AC-1.5, AC-4.1).
                <div class="form-field">
                    <label for="tag-input-field">"Tags"</label>
                    <TagInput tags=tags/>
                </div>

                // Comment — optional (AC-1.5).
                <div class="form-field">
                    <label for="comment">"Comment"</label>
                    <textarea
                        id="comment"
                        name="comment"
                        prop:value=move || comment.get()
                        on:input=move |ev| comment.set(event_target_value(&ev))
                    />
                </div>

                <div class="form-actions">
                    <button type="submit"
                        disabled=move || save_action.pending().get()
                    >
                        {move || if save_action.pending().get() {
                            "Saving…"
                        } else {
                            "Save"
                        }}
                    </button>
                </div>
            </form>
        </main>
    }
}

// ── AddBookmarkPage ───────────────────────────────────────────────────────────

/// Leptos route component for `/add`.
///
/// Reads the `url` query parameter and renders [`AddBookmarkForm`].
/// If `url` is absent or empty, shows a user-friendly error rather than
/// rendering a broken form.
#[component]
pub fn AddBookmarkPage() -> impl IntoView {
    let query = use_query_map();
    let url = move || {
        query
            .get()
            .get("url")
            .map(|s| s.clone())
            .unwrap_or_default()
    };

    move || {
        let u = url();
        if u.is_empty() {
            view! {
                <main>
                    <p class="error-message">"No URL provided. Use the bookmarklet to open this page."</p>
                </main>
            }.into_any()
        } else {
            view! { <AddBookmarkForm url=u/> }.into_any()
        }
    }
}
