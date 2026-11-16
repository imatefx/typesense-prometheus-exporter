use clap::Parser;

mod cli;
mod prometheus_exp;
mod server;
mod typesense;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    let args = cli::CliArgs::parse();
    server::start_metrics_server(args).await
}
