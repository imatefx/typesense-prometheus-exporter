use std::{
    fmt,
    path::{Path, PathBuf},
};

use clap::{CommandFactory, FromArgMatches, Parser, ValueEnum};
use serde::Deserialize;

use crate::{config, key::KeySource, probe_config::ProbeConfig, target};

/// Which metric names the exporter emits. See `docs/metrics-v1.md`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    /// TOML file that can set any option below (snake_case keys) and define
    /// `[clusters.<name>]` with their own API keys. Precedence: flag, then
    /// environment variable, then file, then default. See
    /// deploy/exporter.example.toml.
    #[arg(long, env = "TYPESENSE_EXPORTER_CONFIG")]
    pub config: Option<PathBuf>,

    /// Typesense host served at /metrics. At least one of this,
    /// --probe-allowed-targets or config clusters is required; /probe?target=...
    /// then scrapes any allowed node.
    #[arg(long, env)]
    pub typesense_host: Option<String>,

    /// Typesense protocol
    #[arg(long, env, default_value_t= String::from("http"))]
    pub typesense_protocol: String,

    /// Typesense API key, used for --typesense-host and --probe-allowed-targets
    /// (required with either; clusters in the config file bring their own).
    /// Prefer --typesense-api-key-file (or the environment
    /// variable) over the flag, which other users can see in the process list.
    #[arg(
        long,
        env,
        hide_env_values = true,
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
    /// `both` and `legacy` are deprecated and will be removed in 2.0.0.
    #[arg(long, env = "TYPESENSE_METRIC_NAMES", value_enum, default_value_t = MetricNames::V1)]
    pub metric_names: MetricNames,

    /// Comma-separated `host[:port]` nodes that /probe may scrape (the port
    /// defaults to --typesense-port). `*` allows any target, which lets anyone
    /// who can reach the exporter send the API key to any host. The configured
    /// --typesense-host is always allowed.
    #[arg(long, env = "TYPESENSE_PROBE_ALLOWED_TARGETS", value_delimiter = ',')]
    pub probe_allowed_targets: Vec<String>,

    /// Most /probe scrapes run at once; more wait for a free slot. Each
    /// scrape makes six requests to its node, so this bounds the load an
    /// unauthenticated caller can put on the allowed nodes.
    #[arg(long, env = "TYPESENSE_PROBE_MAX_CONCURRENCY", default_value_t = 16, value_parser = clap::value_parser!(u32).range(1..))]
    pub probe_max_concurrency: u32,

    /// Clusters from the config file, with their keys read.
    #[arg(skip)]
    pub clusters: ProbeConfig,
}

impl CliArgs {
    /// Parse the process arguments and environment, merge the `--config` file
    /// and exit with a message on any error.
    pub fn load() -> Self {
        Self::load_from(std::env::args_os()).unwrap_or_else(|e| e.exit())
    }

    /// Like [`CliArgs::load`], for the given arguments.
    pub fn load_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        // An empty or blank environment variable (Compose's `${VAR:-}`) counts
        // as unset for every option, so the file or the default applies
        // instead of a parse error or an empty value.
        let mut command = Self::command();
        let blank: Vec<String> = command
            .get_arguments()
            .filter(|arg| {
                arg.get_env()
                    .and_then(std::env::var_os)
                    .is_some_and(|v| v.to_string_lossy().trim().is_empty())
            })
            .map(|arg| arg.get_id().to_string())
            .collect();
        for id in blank {
            command = command.mut_arg(id, |arg| arg.env(None::<&str>));
        }
        let matches = command.try_get_matches_from(args)?;
        let mut args = Self::from_arg_matches(&matches)?;
        config::merge(&mut args, &matches, |name| std::env::var(name).ok()).map_err(|msg| {
            clap::Error::raw(clap::error::ErrorKind::ValueValidation, format!("{msg}\n"))
        })?;
        Ok(args)
    }

    /// The API key, from `--typesense-api-key` or read from
    /// `--typesense-api-key-file`.
    pub fn api_key(&self) -> std::io::Result<String> {
        match (&self.typesense_api_key, &self.typesense_api_key_file) {
            (Some(key), _) if !key.trim().is_empty() => Ok(key.clone()),
            (_, Some(path)) if !path.as_os_str().is_empty() => read_key_file(path),
            _ => Err(std::io::Error::other("no Typesense API key configured")),
        }
    }

    /// Like [`Self::api_key`], but a key file is followed when it changes.
    pub fn key_source(&self) -> std::io::Result<KeySource> {
        match (&self.typesense_api_key, &self.typesense_api_key_file) {
            (Some(key), _) if !key.trim().is_empty() => Ok(KeySource::fixed(key.clone())),
            (_, Some(path)) if !path.as_os_str().is_empty() => KeySource::file(path),
            _ => Err(std::io::Error::other("no Typesense API key configured")),
        }
    }

    /// Whether a non-empty key or key file is configured.
    pub fn has_key(&self) -> bool {
        self.typesense_api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
            || self
                .typesense_api_key_file
                .as_deref()
                .is_some_and(|p| !p.as_os_str().is_empty())
    }

    /// Whether the API key would travel unencrypted to another machine
    /// when scraping `--typesense-host`.
    pub fn sends_key_in_plaintext(&self) -> bool {
        self.typesense_host
            .as_deref()
            .is_some_and(|host| !target::is_loopback(host))
            && self.typesense_protocol.eq_ignore_ascii_case("http")
    }

    /// Whether /probe could send the API key unencrypted to another machine:
    /// the default protocol is http and the allow list is `*` or names a
    /// non-loopback host. With an https default, `http://` targets are
    /// refused instead.
    pub fn probe_sends_key_in_plaintext(&self) -> bool {
        self.typesense_protocol.eq_ignore_ascii_case("http")
            && target::AllowList::parse(&self.probe_allowed_targets, self.typesense_port)
                .is_ok_and(|list| list.has_remote())
    }
}

/// Read an API key file: surrounding whitespace is ignored; empty or not
/// usable as an HTTP header value (a byte order mark, control characters,
/// non-ASCII) is an error. Never includes the key in the error.
pub fn read_key_file(path: &Path) -> std::io::Result<String> {
    let key = std::fs::read_to_string(path)?.trim().to_string();
    if key.is_empty() {
        return Err(std::io::Error::other(format!(
            "API key file {} is empty",
            path.display()
        )));
    }
    if !key.is_ascii() || reqwest::header::HeaderValue::from_str(&key).is_err() {
        return Err(std::io::Error::other(format!(
            "API key file {} has characters that are not valid in an HTTP header (a byte order mark, control or non-ASCII characters?)",
            path.display()
        )));
    }
    Ok(key)
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
            .field("probe_allowed_targets", &self.probe_allowed_targets)
            .field("probe_max_concurrency", &self.probe_max_concurrency)
            .field("config", &self.config)
            .field("clusters", &self.clusters)
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
    fn probe_concurrency_defaults_to_16_and_must_be_positive() {
        let base = ["--typesense-host", "ts", "--typesense-api-key", "key"];
        assert_eq!(parse(&base).unwrap().probe_max_concurrency, 16);
        let mut set = base.to_vec();
        set.extend(["--probe-max-concurrency", "4"]);
        assert_eq!(parse(&set).unwrap().probe_max_concurrency, 4);
        let mut zero = base.to_vec();
        zero.extend(["--probe-max-concurrency", "0"]);
        assert!(parse(&zero).is_err());
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
    fn rejects_two_key_sources() {
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
    fn parses_probe_allowed_targets() {
        let none = parse(&["--typesense-host=ts", "--typesense-api-key=k"]).unwrap();
        assert!(none.probe_allowed_targets.is_empty());

        let args = parse(&[
            "--typesense-host=ts",
            "--typesense-api-key=k",
            "--probe-allowed-targets=a:1,b,[::1]:2",
        ])
        .unwrap();
        assert_eq!(args.probe_allowed_targets, ["a:1", "b", "[::1]:2"]);
    }

    #[test]
    fn probe_allowed_targets_reads_its_environment_variable() {
        let arg = CliArgs::command()
            .get_arguments()
            .find(|a| a.get_id() == "probe_allowed_targets")
            .unwrap()
            .clone();
        assert_eq!(
            arg.get_env().and_then(|e| e.to_str()),
            Some("TYPESENSE_PROBE_ALLOWED_TARGETS")
        );
    }

    #[test]
    fn host_is_optional_with_an_allow_list() {
        let args = parse(&["--typesense-api-key=k", "--probe-allowed-targets=ts1,ts2"]).unwrap();
        assert_eq!(args.typesense_host, None);
        assert!(!args.sends_key_in_plaintext());
    }

    #[test]
    fn config_reads_its_environment_variable() {
        let arg = CliArgs::command()
            .get_arguments()
            .find(|a| a.get_id() == "config")
            .unwrap()
            .clone();
        assert_eq!(
            arg.get_env().and_then(|e| e.to_str()),
            Some("TYPESENSE_EXPORTER_CONFIG")
        );
        let args = parse(&["--config=/etc/e.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("/etc/e.toml")));
        assert_eq!(args.typesense_host, None);
        assert!(args.api_key().is_err());
    }

    #[test]
    fn load_from_merges_the_file_and_reports_errors_as_clap_errors() {
        let path = std::env::temp_dir().join(format!("ts-cli-config-{}", std::process::id()));
        std::fs::write(
            &path,
            "typesense_host = \"fromfile\"\ntypesense_api_key_env = \"TS_CLI_TEST_NO_SUCH_VAR\"\n",
        )
        .unwrap();
        let cfg = format!("--config={}", path.display());

        let err = CliArgs::load_from(["exporter", &cfg]).unwrap_err();
        assert!(err.to_string().contains("TS_CLI_TEST_NO_SUCH_VAR"), "{err}");

        let args = CliArgs::load_from(["exporter", &cfg, "--typesense-api-key=k"]).unwrap();
        assert_eq!(args.typesense_host.as_deref(), Some("fromfile"));
        std::fs::remove_file(&path).unwrap();

        assert!(CliArgs::load_from(["exporter", "--bogus"]).is_err());
    }

    #[test]
    fn flags_plaintext_keys_sent_by_probe() {
        let probe = |extra: &[&str]| {
            let mut a = vec!["--typesense-api-key=k"];
            a.extend_from_slice(extra);
            parse(&a).unwrap().probe_sends_key_in_plaintext()
        };
        assert!(probe(&["--probe-allowed-targets=search.internal"]));
        assert!(probe(&["--probe-allowed-targets=*"]));
        assert!(!probe(&["--probe-allowed-targets=localhost,127.0.0.1:9"]));
        assert!(!probe(&["--typesense-host=ts"]), "no allow list");
        assert!(!probe(&[
            "--probe-allowed-targets=search.internal",
            "--typesense-protocol=https"
        ]));
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
