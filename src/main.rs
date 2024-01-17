use clap::Parser;
use tokio;
pub mod cli;
pub mod prometheus_exp;
pub mod server;
pub mod typesense;

#[tokio::main]
async fn main() {
    let args = cli::CliArgs::parse();
    // println!("{:#?}", args);
    server::main_server(args).await;
    // println!("Hello, world!");

    // print all values in debug format
}
