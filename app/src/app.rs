/// Leptos application root component.
///
/// This module is compiled for both server and WASM targets.
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

/// Root component — sets up routing and metadata context.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Title text="Procastimarks"/>
        <Router>
            <Routes fallback=|| view! { <p>"Page not found."</p> }>
                <Route path=path!("/") view=HomePage/>
            </Routes>
        </Router>
    }
}

/// Placeholder home page — replaced by BookmarkList in EPIC-4.
#[component]
fn HomePage() -> impl IntoView {
    view! {
        <main>
            <h1>"Procastimarks"</h1>
            <p>"No bookmarks yet. Use the bookmarklet to save your first one."</p>
        </main>
    }
}
