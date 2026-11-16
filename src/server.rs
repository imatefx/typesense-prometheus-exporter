use std::sync::Arc;

use axum::{extract::State, routing::get, Router};
use tokio::signal;

use crate::{
    cli::CliArgs,
    prometheus_exp::{self, Scrape},
    typesense::TypesenseClient,
};

struct AppState {
    client: TypesenseClient,
    host: String,
    port: u16,
}

/// Build the exporter's HTTP router for the Typesense node described by `args`.
pub fn router(args: &CliArgs) -> reqwest::Result<Router> {
    let state = Arc::new(AppState {
        client: TypesenseClient::new(args)?,
        host: args.typesense_host.clone(),
        port: args.typesense_port,
    });

    Ok(Router::new()
        .route("/", get(root))
        .route("/metrics", get(metrics_handler))
        .with_state(state))
}

/// Bind to the configured address and serve until SIGINT/SIGTERM.
pub async fn run(args: CliArgs) -> std::io::Result<()> {
    let app = router(&args).map_err(std::io::Error::other)?;

    let bind_address = format!("{}:{}", args.exporter_bind_address, args.exporter_bind_port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!("exporter listening on {bind_address}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn root() -> &'static str {
    "Typesense Prometheus Exporter. Metrics are served at /metrics\n"
}

/// Scrape Typesense and render metrics. Each endpoint is fetched concurrently;
/// a failed endpoint is logged and its metrics are omitted, while
/// `typesense_health` and `typesense_raft_state` drop to 0.
async fn metrics_handler(State(state): State<Arc<AppState>>) -> String {
    let client = &state.client;
    let (metrics, stats, health, debug, status, collections) = tokio::join!(
        client.metrics(),
        client.stats(),
        client.health(),
        client.debug(),
        client.status(),
        client.collections(),
    );
    let debug = log_failure("/debug", debug);

    let scrape = Scrape {
        metrics: log_failure("/metrics.json", metrics),
        stats: log_failure("/stats.json", stats),
        healthy: log_failure("/health", health).is_some_and(|h| h.ok),
        raft_state: debug.as_ref().map_or(0, |d| d.state),
        version: debug.and_then(|d| d.version),
        status: log_failure("/status", status),
        collections: log_failure("/collections", collections),
    };

    prometheus_exp::generate_metrics(&scrape, &state.host, state.port)
}

fn log_failure<T>(endpoint: &str, result: reqwest::Result<T>) -> Option<T> {
    result
        .inspect_err(|err| tracing::warn!("typesense {endpoint} failed: {err}"))
        .ok()
}
