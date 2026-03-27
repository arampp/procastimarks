/// Procastimarks library root.
///
/// This crate compiles for two targets:
///   - `x86_64-unknown-linux-gnu` (server binary + tests)
///   - `wasm32-unknown-unknown`   (Leptos UI, runs in the browser)
///
/// Code under `#[cfg(not(target_arch = "wasm32"))]` is server-only.

pub mod app;

#[cfg(not(target_arch = "wasm32"))]
pub mod persistence;

#[cfg(not(target_arch = "wasm32"))]
pub mod routes;

/// Construct the Axum [`axum::Router`] for the application.
///
/// This function is the single composition root for the HTTP layer.
/// Tests drive it directly via [`tower::ServiceExt::oneshot`] without
/// spawning a process.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_router() -> axum::Router {
    use axum::routing::get;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options.clone();

    let routes = generate_route_list(app::App);

    axum::Router::new()
        // Public health check — no authentication required.
        .route("/health", get(routes::health::handler))
        // Leptos server functions are mounted automatically at /api/
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
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
