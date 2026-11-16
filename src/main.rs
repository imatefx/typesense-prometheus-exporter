use typesense_prometheus_exporter::{cli::CliArgs, server};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    server::run(CliArgs::load()).await
}
