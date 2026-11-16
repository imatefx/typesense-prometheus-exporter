use std::{fmt, path::PathBuf};

use clap::{Parser, ValueEnum};

/// Which metric names the exporter emits. See `docs/metrics-v1.md`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum MetricNames {
    /// The 1.0 names only.
    #[default]
    V1,
    /// The 1.0 names and the 0.4 names, for migrating queries.
    Both,
    /// The 0.4 names only (`typesense_metrics{key}`, `typesense_stats{key}`, ...).
    Legacy,
}

impl MetricNames {
    /// Whether the 1.0 names are emitted.
    pub fn v1(self) -> bool {
        matches!(self, Self::V1 | Self::Both)
    }

    /// Whether the 0.4 names are emitted.
    pub fn legacy(self) -> bool {
        matches!(self, Self::Legacy | Self::Both)
    }
}

/// Expose Typesense metrics and stats in Prometheus format
#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Typesense Host url
    #[arg(long, env)]
    pub typesense_host: String,

    /// Typesense protocol
    #[arg(long, env, default_value_t= String::from("http"))]
    pub typesense_protocol: String,

    /// Typesense API key. Prefer --typesense-api-key-file (or the environment
    /// variable) over the flag, which other users can see in the process list.
    #[arg(
        long,
        env,
        hide_env_values = true,
        required_unless_present = "typesense_api_key_file",
        conflicts_with = "typesense_api_key_file"
    )]
    pub typesense_api_key: Option<String>,

    /// File containing the Typesense API key, e.g. a Docker or Kubernetes
    /// secret. Surrounding whitespace is ignored.
    #[arg(long, env)]
    pub typesense_api_key_file: Option<PathBuf>,

    /// Typesense port number
    #[arg(long, env, default_value_t = 8108)]
    pub typesense_port: u16,

    /// Bind address for internal server
    #[arg(long, env, default_value_t = String::from("0.0.0.0"))]
    pub exporter_bind_address: String,

    /// Bind port for internal server
    #[arg(long, env, default_value_t = 8888)]
    pub exporter_bind_port: u16,

    /// Timeout for each Typesense request, in seconds; 0 disables it.
    /// Keep it below the Prometheus scrape timeout so a hung Typesense shows
    /// up as unhealthy instead of a failed scrape.
    #[arg(long, env, default_value_t = 5)]
    pub typesense_timeout: u64,

    /// Metric names to export: `v1` (the 1.0 names), `both` (1.0 and 0.4
    /// names, while moving queries over) or `legacy` (0.4 names only).
    #[arg(long, env = "TYPESENSE_METRIC_NAMES", value_enum, default_value_t = MetricNames::V1)]
    pub metric_names: MetricNames,
}

impl CliArgs {
    /// The API key, from `--typesense-api-key` or read from
    /// `--typesense-api-key-file`.
    pub fn api_key(&self) -> std::io::Result<String> {
        match (&self.typesense_api_key, &self.typesense_api_key_file) {
            (Some(key), _) => Ok(key.clone()),
            (None, Some(path)) => {
                let key = std::fs::read_to_string(path)?.trim().to_string();
                if key.is_empty() {
                    return Err(std::io::Error::other(format!(
                        "API key file {} is empty",
                        path.display()
                    )));
                }
                Ok(key)
            }
            (None, None) => Err(std::io::Error::other("no Typesense API key configured")),
        }
    }

    /// Whether the API key would travel unencrypted to another machine.
    pub fn sends_key_in_plaintext(&self) -> bool {
        let local = matches!(
            self.typesense_host.as_str(),
            "localhost" | "127.0.0.1" | "::1" | "[::1]"
        );
        self.typesense_protocol.eq_ignore_ascii_case("http") && !local
    }
}

/// Like the derived `Debug`, but never prints the API key.
impl fmt::Debug for CliArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliArgs")
            .field("typesense_host", &self.typesense_host)
            .field("typesense_protocol", &self.typesense_protocol)
            .field(
                "typesense_api_key",
                &self.typesense_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("typesense_api_key_file", &self.typesense_api_key_file)
            .field("typesense_port", &self.typesense_port)
            .field("exporter_bind_address", &self.exporter_bind_address)
            .field("exporter_bind_port", &self.exporter_bind_port)
            .field("typesense_timeout", &self.typesense_timeout)
            .field("metric_names", &self.metric_names)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<CliArgs, clap::Error> {
        CliArgs::try_parse_from(std::iter::once("exporter").chain(args.iter().copied()))
    }

    #[test]
    fn applies_defaults() {
        let args = parse(&["--typesense-host", "ts", "--typesense-api-key", "key"]).unwrap();

        assert_eq!(args.typesense_protocol, "http");
        assert_eq!(args.typesense_port, 8108);
        assert_eq!(args.exporter_bind_address, "0.0.0.0");
        assert_eq!(args.exporter_bind_port, 8888);
        assert_eq!(args.typesense_timeout, 5);
        assert_eq!(args.metric_names, MetricNames::V1);
        assert_eq!(args.api_key().unwrap(), "key");
    }

    #[test]
    fn accepts_overrides() {
        let args = parse(&[
            "--typesense-host=ts",
            "--typesense-api-key=key",
            "--typesense-protocol=https",
            "--typesense-port=443",
            "--typesense-timeout=0",
            "--exporter-bind-port=9000",
        ])
        .unwrap();

        assert_eq!(args.typesense_protocol, "https");
        assert_eq!(args.typesense_port, 443);
        assert_eq!(args.typesense_timeout, 0);
        assert_eq!(args.exporter_bind_port, 9000);
    }

    #[test]
    fn rejects_invalid_port_and_negative_timeout() {
        assert!(parse(&[
            "--typesense-host=ts",
            "--typesense-api-key=k",
            "--typesense-port=99999"
        ])
        .is_err());
        assert!(parse(&[
            "--typesense-host=ts",
            "--typesense-api-key=k",
            "--typesense-timeout=-1"
        ])
        .is_err());
    }

    #[test]
    fn requires_exactly_one_key_source() {
        assert!(parse(&["--typesense-host=ts"]).is_err(), "no key");
        assert!(
            parse(&[
                "--typesense-host=ts",
                "--typesense-api-key=k",
                "--typesense-api-key-file=/k"
            ])
            .is_err(),
            "both key sources"
        );
    }

    #[test]
    fn reads_and_trims_the_key_file() {
        let path = std::env::temp_dir().join(format!("ts-key-{}", std::process::id()));
        std::fs::write(&path, "  secret-key\n").unwrap();
        let args = parse(&[
            "--typesense-host=ts",
            &format!("--typesense-api-key-file={}", path.display()),
        ])
        .unwrap();

        assert_eq!(args.api_key().unwrap(), "secret-key");

        std::fs::write(&path, "\n").unwrap();
        assert!(args.api_key().is_err(), "empty key file");
        std::fs::remove_file(&path).unwrap();
        assert!(args.api_key().is_err(), "missing key file");
    }

    #[test]
    fn never_prints_the_key() {
        let args = parse(&["--typesense-host=ts", "--typesense-api-key=supersecret"]).unwrap();
        assert!(!format!("{args:?}").contains("supersecret"));

        let api_key_arg = CliArgs::command()
            .get_arguments()
            .find(|a| a.get_id() == "typesense_api_key")
            .unwrap()
            .clone();
        assert!(
            api_key_arg.is_hide_env_values_set(),
            "--help must not show the env value"
        );
    }

    #[test]
    fn flags_plaintext_keys_to_remote_hosts() {
        let remote = parse(&["--typesense-host=search.internal", "--typesense-api-key=k"]).unwrap();
        let local = parse(&["--typesense-host=localhost", "--typesense-api-key=k"]).unwrap();
        let tls = parse(&[
            "--typesense-host=search.internal",
            "--typesense-api-key=k",
            "--typesense-protocol=https",
        ])
        .unwrap();

        assert!(remote.sends_key_in_plaintext());
        assert!(!local.sends_key_in_plaintext());
        assert!(!tls.sends_key_in_plaintext());
    }

    #[test]
    fn parses_metric_names() {
        for (value, expected) in [
            ("v1", MetricNames::V1),
            ("both", MetricNames::Both),
            ("legacy", MetricNames::Legacy),
        ] {
            let args = parse(&[
                "--typesense-host=ts",
                "--typesense-api-key=k",
                &format!("--metric-names={value}"),
            ])
            .unwrap();
            assert_eq!(args.metric_names, expected, "{value}");
        }
        assert!(parse(&[
            "--typesense-host=ts",
            "--typesense-api-key=k",
            "--metric-names=v2"
        ])
        .is_err());
    }

    #[test]
    fn metric_names_select_the_name_sets() {
        assert!(MetricNames::V1.v1() && !MetricNames::V1.legacy());
        assert!(MetricNames::Both.v1() && MetricNames::Both.legacy());
        assert!(!MetricNames::Legacy.v1() && MetricNames::Legacy.legacy());
    }

    #[test]
    fn metric_names_reads_its_environment_variable() {
        let arg = CliArgs::command()
            .get_arguments()
            .find(|a| a.get_id() == "metric_names")
            .unwrap()
            .clone();
        assert_eq!(
            arg.get_env().and_then(|e| e.to_str()),
            Some("TYPESENSE_METRIC_NAMES")
        );
    }
}
