use clap::Parser;

/// Expose Typesense metrics and stats in Prometheus format
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Typesense Host url
    #[arg(long, env)]
    pub typesense_host: String,

    /// Typesense protocol
    #[arg(long, env, default_value_t= String::from("http"))]
    pub typesense_protocol: String,

    /// Typesense API key
    #[arg(long, env)]
    pub typesense_api_key: String,

    /// Typesense port number
    #[arg(long, env, default_value_t = 8108)]
    pub typesense_port: u16,

    /// Bind address for internal server
    #[arg(long, env, default_value_t = String::from("0.0.0.0"))]
    pub exporter_bind_address: String,

    /// Bind port for internal server
    #[arg(long, env, default_value_t = 8888)]
    pub exporter_bind_port: u16,

    /// Timeout for Typesense API requests in seconds (0 or less disables it).
    /// Keep it below the Prometheus scrape timeout so a hung Typesense shows
    /// up as unhealthy instead of a failed scrape.
    #[arg(long, env, default_value_t = 5)]
    pub typesense_timeout: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_defaults() {
        let args = CliArgs::try_parse_from([
            "exporter",
            "--typesense-host",
            "ts",
            "--typesense-api-key",
            "key",
        ])
        .unwrap();

        assert_eq!(args.typesense_protocol, "http");
        assert_eq!(args.typesense_port, 8108);
        assert_eq!(args.exporter_bind_address, "0.0.0.0");
        assert_eq!(args.exporter_bind_port, 8888);
        assert_eq!(args.typesense_timeout, 5);
    }

    #[test]
    fn accepts_overrides() {
        let args = CliArgs::try_parse_from([
            "exporter",
            "--typesense-host=ts",
            "--typesense-api-key=key",
            "--typesense-protocol=https",
            "--typesense-port=443",
            "--typesense-timeout=5",
            "--exporter-bind-port=9000",
        ])
        .unwrap();

        assert_eq!(args.typesense_protocol, "https");
        assert_eq!(args.typesense_port, 443);
        assert_eq!(args.typesense_timeout, 5);
        assert_eq!(args.exporter_bind_port, 9000);
    }

    #[test]
    fn rejects_invalid_port() {
        assert!(CliArgs::try_parse_from([
            "exporter",
            "--typesense-host=ts",
            "--typesense-api-key=key",
            "--typesense-port=99999",
        ])
        .is_err());
    }
}
