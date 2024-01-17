use std::sync::Arc;

use crate::typesense::models::typesense_metrics_model::TypesenseMetrics;
use crate::typesense::models::typesense_stats_model::TypesenseStats;
use crate::typesense::stats::get_typesense_stats;
use crate::{cli::CliArgs, typesense::metrics::get_typesense_metrics};

use axum::extract::State;
// use crate::typesense::metrics;
// use serde::Deserialize;
use axum::{routing::get, Router};

// #[tokio::main]
pub(crate) async fn main_server(args: CliArgs) {
    // initialize tracing
    tracing_subscriber::fmt::init();

    let shared_cli_args: Arc<CliArgs> = Arc::new(args.clone());

    // build our application with a route
    let app = Router::new()
        .route("/", get(root))
        .route("/metrics", get(metrics_route_handler))
        .with_state(shared_cli_args);

    // run our app with hyper, listening globally on port 3000
    let bind_address = format!("{}:{}", args.exporter_bind_address, args.exporter_bind_port);
    println!("Starting exporter server at {:?}", bind_address);
    let listener = tokio::net::TcpListener::bind(bind_address).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Hello, World! cool nice"
}

async fn metrics_route_handler(State(args): State<Arc<CliArgs>>) -> String {
    println!("Args :  {:?}", args.exporter_bind_address);
    let data: TypesenseMetrics = get_typesense_metrics(args.clone()).await.unwrap();
    // format!("get_typesense_metrics, {:?}", data);

    let data2: TypesenseStats = get_typesense_stats(args.clone()).await.unwrap();
    format!(
        "get_typesense_metrics : {:#?}   get_typesense_stats : {:#?}",
        data, data2
    )
}

// #[tokio::main]
