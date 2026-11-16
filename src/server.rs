use std::{sync::Arc, time::Instant};

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
/// Fails if the API key can't be read or the HTTP client can't be built.
pub fn router(args: &CliArgs) -> std::io::Result<Router> {
    let client = TypesenseClient::new(args, args.api_key()?).map_err(std::io::Error::other)?;
    let state = Arc::new(AppState {
        client,
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
    let bind_address = format!("{}:{}", args.exporter_bind_address, args.exporter_bind_port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!("exporter listening on {bind_address}");
    serve(listener, args).await
}

/// Serve on an already-bound listener until SIGINT/SIGTERM.
pub async fn serve(listener: tokio::net::TcpListener, args: CliArgs) -> std::io::Result<()> {
    let app = router(&args)?;
    if args.sends_key_in_plaintext() {
        tracing::warn!(
            "sending the Typesense API key over unencrypted http to {}; use --typesense-protocol https",
            args.typesense_host
        );
    }

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
/// a failed endpoint is logged, its metrics are omitted and its
/// `typesense_scrape_success` is 0, while `typesense_health` and
/// `typesense_raft_state` drop to 0.
async fn metrics_handler(State(state): State<Arc<AppState>>) -> String {
    let client = &state.client;
    let started = Instant::now();
    let (metrics, stats, health, debug, status, collections) = tokio::join!(
        client.metrics(),
        client.stats(),
        client.health(),
        client.debug(),
        client.status(),
        client.collections(),
    );
    let scrape_duration = started.elapsed().as_secs_f64();

    let mut endpoints = Vec::with_capacity(6);
    let mut ok = |endpoint: &'static str, succeeded: bool| endpoints.push((endpoint, succeeded));
    let metrics = log_failure("/metrics.json", metrics);
    ok("/metrics.json", metrics.is_some());
    let stats = log_failure("/stats.json", stats);
    ok("/stats.json", stats.is_some());
    let health = log_failure("/health", health);
    ok("/health", health.is_some());
    let debug = log_failure("/debug", debug);
    ok("/debug", debug.is_some());
    let status = log_failure("/status", status);
    ok("/status", status.is_some());
    let collections = log_failure("/collections", collections);
    ok("/collections", collections.is_some());

    let scrape = Scrape {
        metrics,
        stats,
        healthy: health.is_some_and(|h| h.ok),
        raft_state: debug.as_ref().map_or(0, |d| d.state),
        version: debug.and_then(|d| d.version),
        status,
        collections,
        endpoints,
        scrape_duration: Some(scrape_duration),
    };

    prometheus_exp::generate_metrics(&scrape, &state.host, state.port)
}

fn log_failure<T>(endpoint: &str, result: reqwest::Result<T>) -> Option<T> {
    result
        .inspect_err(|err| tracing::warn!("typesense {endpoint} failed: {err}"))
        .ok()
}
