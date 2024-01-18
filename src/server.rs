use std::sync::Arc;

use crate::prometheus_exp;
use crate::{
    cli::CliArgs,
    typesense::{metrics::get_typesense_metrics, stats::get_typesense_stats},
};

use axum::extract::State;
use axum::{routing::get, Router};
use futures::future;
use tokio::signal;

pub(crate) async fn start_metrics_server(args: CliArgs) {
    tracing_subscriber::fmt::init();

    let shared_cli_args: Arc<CliArgs> = Arc::new(args.clone());

    let app = Router::new()
        .route("/", get(root))
        .route("/metrics", get(metrics_route_handler))
        .with_state(shared_cli_args);

    let bind_address = format!("{}:{}", args.exporter_bind_address, args.exporter_bind_port);
    println!("Starting exporter server at {:?}", bind_address);
    let listener = tokio::net::TcpListener::bind(bind_address).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to run Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to run signal handler")
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
    "Hello, World!"
}

async fn metrics_route_handler(State(args): State<Arc<CliArgs>>) -> String {
    let (metrics_data, stats_data) = future::join(
        get_typesense_metrics(args.clone()),
        get_typesense_stats(args.clone()),
    )
    .await;

    let promdata = prometheus_exp::generate_metrics(
        metrics_data.unwrap().clone(),
        stats_data.unwrap().clone(),
        args.clone(),
    )
    .await;

    return promdata;
}
