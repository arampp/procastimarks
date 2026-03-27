/// Procastimarks — server entry-point.
///
/// Reads configuration from environment variables, initialises the database,
/// builds the Axum router, and binds the server.
///
/// Startup fails fast (panic) if required environment variables are absent.
use anyhow::Context;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Logging ──────────────────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer())
        .init();

    // ── Configuration ────────────────────────────────────────────────────────
    let api_key = std::env::var("API_KEY")
        .context("API_KEY environment variable is required but was not set")?;
    if api_key.is_empty() {
        anyhow::bail!("API_KEY must not be empty");
    }

    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL environment variable is required but was not set")?;

    let bind_address = std::env::var("BIND_ADDRESS")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    info!(bind_address, "Procastimarks starting");

    // ── Database ─────────────────────────────────────────────────────────────
    procastimarks::persistence::init_db(&database_url)
        .context("Failed to initialise the SQLite database")?;

    // ── HTTP server ──────────────────────────────────────────────────────────
    let router = procastimarks::create_router();
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .with_context(|| format!("Failed to bind to {bind_address}"))?;

    info!(bind_address, "Server listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    info!("Server shut down gracefully");
    Ok(())
}

/// Resolves when SIGTERM or SIGINT is received.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
}
