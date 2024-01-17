use std::sync::Arc;

use crate::prometheus_exp;
use crate::{
    cli::CliArgs,
    typesense::{metrics::get_typesense_metrics, stats::get_typesense_stats},
};

use axum::extract::State;
use axum::{routing::get, Router};
use futures::future;

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
    axum::serve(listener, app).await.unwrap();
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
