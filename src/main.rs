use typesense_prometheus_exporter::{cli::CliArgs, logging, server};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Logging starts after the options are loaded: the level can come from
    // the config file. Errors while loading are printed by clap.
    let args = CliArgs::load();
    if let Err(e) = logging::init(&args) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    server::run(args).await
}
