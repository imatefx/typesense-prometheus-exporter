use clap::Parser;
use tokio;
pub mod typesense;
pub mod cli;
pub mod server;
pub mod prometheus_exp;

#[tokio::main]
async fn main() {
    let args = cli::CliArgs::parse();
    println!("{:#?}", args);
    // prometheus_exp::main_server().await;
    server::main_server(args).await;
    // println!("Hello, world!");

    // print all values in debug format
}
