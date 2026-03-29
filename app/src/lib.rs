/// Procastimarks library root.
///
/// This crate compiles for two targets:
///   - `x86_64-unknown-linux-gnu` (server binary + tests)
///   - `wasm32-unknown-unknown`   (Leptos UI, runs in the browser)
///
/// Code under `#[cfg(not(target_arch = "wasm32"))]` is server-only.

pub mod app;
pub mod components;
pub mod domain;

#[cfg(not(target_arch = "wasm32"))]
pub mod metadata;

#[cfg(not(target_arch = "wasm32"))]
pub mod middleware;

#[cfg(not(target_arch = "wasm32"))]
pub mod persistence;

#[cfg(not(target_arch = "wasm32"))]
pub mod routes;

pub mod server_fns;

#[cfg(not(target_arch = "wasm32"))]
pub mod session;

/// Construct the Axum [`axum::Router`] for the application.
///
/// Reads `API_KEY` from the environment at construction time.  Panics if the
/// variable is absent or empty — there is no safe default; a missing key would
/// allow any `?api_key=` value to authenticate.
///
/// This function is the single composition root for the HTTP layer.
/// Tests should use [`create_router_with_state`] directly with an explicit
/// [`middleware::auth::AppState`] to avoid mutating process-wide environment
/// variables.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_router(repo: persistence::BookmarkRepository) -> axum::Router {
    use middleware::auth::AppState;
    use std::sync::Arc;

    let api_key_str = std::env::var("API_KEY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("API_KEY environment variable must be set and non-empty"));

    let api_key: Arc<str> = api_key_str.into();

    let metadata_fetcher = metadata::MetadataFetcher::new()
        .unwrap_or_else(|e| panic!("Failed to build MetadataFetcher: {e}"));

    let state = AppState {
        api_key,
        sessions: session::new_store(),
        repo,
        metadata_fetcher,
    };

    build_router(state)
}

/// Construct the router with an explicit [`middleware::auth::AppState`].
///
/// Exposed for testing: tests that must send two requests to the *same*
/// session store construct an `AppState` themselves, call this function
/// twice, and reuse the store between calls.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_router_with_state(state: middleware::auth::AppState) -> axum::Router {
    build_router(state)
}

#[cfg(not(target_arch = "wasm32"))]
fn build_router(state: middleware::auth::AppState) -> axum::Router {
    use axum::middleware as axum_middleware;
    use axum::routing::get;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options.clone();

    let routes = generate_route_list(app::App);

    // Clone the repo, api_key, and metadata_fetcher so the closure can
    // capture them by value.
    let repo = state.repo.clone();
    let api_key = state.api_key.clone();
    let metadata_fetcher = state.metadata_fetcher.clone();

    axum::Router::new()
        // Public health check — no authentication required (C-2).
        .route("/health", get(routes::health::handler))
        // Leptos server functions are mounted automatically at /api/;
        // `leptos_routes_with_context` injects the BookmarkRepository, the
        // API key, and the MetadataFetcher into every server function's
        // Leptos context.
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || {
                leptos::context::provide_context(repo.clone());
                leptos::context::provide_context(api_key.clone());
                leptos::context::provide_context(metadata_fetcher.clone());
            },
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        // Authentication middleware — outermost layer, wraps every route (C-2).
        .layer(axum_middleware::from_fn_with_state(
            state,
            middleware::auth::require_auth,
        ))
        .with_state(leptos_options)
}

/// Leptos SSR shell — renders the initial HTML document.
#[cfg(not(target_arch = "wasm32"))]
fn shell(options: leptos::config::LeptosOptions) -> impl leptos::IntoView {
    use leptos::prelude::*;
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options=options.clone()/>
                <leptos_meta::MetaTags/>
            </head>
            <body>
                <app::App/>
            </body>
        </html>
    }
}
