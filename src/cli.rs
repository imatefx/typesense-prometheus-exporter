use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Typesense Host url
    #[arg(long, env)]
    pub(crate) typesense_host: String,

    /// Typesense protocol
    #[arg(long, env, default_value_t= String::from("http"))]
    pub(crate) typesense_protocol: String,

    /// Typesense API key
    #[arg(long, env)]
    pub(crate) typesense_api_key: String,

    /// Typesense port number
    #[arg(long, env, default_value_t = 8081)]
    pub(crate) typesense_port: u16,

    /// Bind address for internal server
    #[arg(long, env, default_value_t = String::from("0.0.0.0"))]
    pub(crate) exporter_bind_address: String,

    /// Bind port for internal server
    #[arg(long, env, default_value_t = 8888)]
    pub(crate) exporter_bind_port: u16,


}