use clap::Parser;
use tokio;
pub mod cli;
pub mod prometheus_exp;
pub mod server;
pub mod typesense;

#[tokio::main]
async fn main() {
    let args = cli::CliArgs::parse();
    server::start_metrics_server(args).await;
}
