/// Leptos application root component.
///
/// This module is compiled for both server and WASM targets.
use leptos::prelude::*;
use leptos_meta::{provide_meta_context, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use crate::components::{AddBookmarkPage, BookmarkList, BookmarkletInstall};

/// Root component — sets up routing and metadata context.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Title text="Procastimarks"/>
        <Router>
            <Routes fallback=|| view! { <p>"Page not found."</p> }>
                <Route path=path!("/") view=BookmarkList/>
                <Route path=path!("/add") view=AddBookmarkPage/>
                <Route path=path!("/bookmarklet") view=BookmarkletInstall/>
            </Routes>
        </Router>
    }
}
