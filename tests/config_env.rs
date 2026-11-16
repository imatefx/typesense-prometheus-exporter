//! Precedence with real environment variables. Kept in its own test binary
//! because it changes the process environment; the tests take a lock so they
//! never overlap.

use std::sync::Mutex;

use typesense_prometheus_exporter::{
    cli::{CliArgs, MetricNames},
    logging::{LogFormat, LogLevel},
};

static ENV: Mutex<()> = Mutex::new(());

const VARS: [&str; 14] = [
    "TYPESENSE_PORT",
    "TYPESENSE_HOST",
    "TYPESENSE_API_KEY",
    "TYPESENSE_PROBE_ALLOWED_TARGETS",
    "TYPESENSE_METRIC_NAMES",
    "TYPESENSE_EXPORTER_CONFIG",
    "TYPESENSE_API_KEY_FILE",
    "TYPESENSE_PROTOCOL",
    "TYPESENSE_TIMEOUT",
    "EXPORTER_BIND_ADDRESS",
    "EXPORTER_BIND_PORT",
    "TYPESENSE_PROBE_MAX_CONCURRENCY",
    "TYPESENSE_EXPORTER_LOG_LEVEL",
    "TYPESENSE_EXPORTER_LOG_FORMAT",
];

/// Run `test` with a clean environment and a config file holding `toml`.
fn with_file(name: &str, toml: &str, test: impl FnOnce(&str)) {
    let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
    for v in VARS {
        std::env::remove_var(v);
    }
    let path =
        std::env::temp_dir().join(format!("ts-config-env-{}-{name}.toml", std::process::id()));
    std::fs::write(&path, toml).unwrap();
    test(&format!("--config={}", path.display()));
    for v in VARS {
        std::env::remove_var(v);
    }
    std::fs::remove_file(&path).unwrap();
}

fn key_file(name: &str, contents: &str) -> String {
    let path = std::env::temp_dir().join(format!("ts-config-env-{}-{name}", std::process::id()));
    std::fs::write(&path, contents).unwrap();
    format!("{:?}", path.display().to_string())
}

#[test]
fn flag_beats_environment_beats_file_beats_default() {
    let toml = "typesense_host = \"filehost\"\ntypesense_port = 1111\ntypesense_timeout = 7\ntypesense_protocol = \"https\"\n";
    with_file("precedence", toml, |config| {
        std::env::set_var("TYPESENSE_PORT", "2222");
        std::env::set_var("TYPESENSE_HOST", "envhost");

        let args = CliArgs::load_from(["exporter", config]).unwrap();
        assert_eq!(args.typesense_port, 2222, "environment beats the file");
        assert_eq!(args.typesense_host.as_deref(), Some("envhost"));
        assert_eq!(args.typesense_timeout, 7, "file beats the default");
        assert_eq!(args.typesense_protocol, "https");

        let args = CliArgs::load_from(["exporter", config, "--typesense-port=3333"]).unwrap();
        assert_eq!(args.typesense_port, 3333, "flag beats the environment");

        std::env::remove_var("TYPESENSE_PORT");
        std::env::remove_var("TYPESENSE_HOST");
        let args = CliArgs::load_from(["exporter", config]).unwrap();
        assert_eq!(args.typesense_port, 1111);
        assert_eq!(args.typesense_host.as_deref(), Some("filehost"));

        let path = config.strip_prefix("--config=").unwrap();
        std::env::set_var("TYPESENSE_EXPORTER_CONFIG", path);
        let args = CliArgs::load_from(["exporter"]).unwrap();
        assert_eq!(
            args.typesense_port, 1111,
            "--config is read from its variable"
        );
    });
}

#[test]
fn empty_environment_variables_fall_back_to_the_file() {
    let key = key_file("empty-env-key", "file-key\n");
    let toml = format!(
        "typesense_host = \"filehost\"\nprobe_allowed_targets = [\"a:1\", \"b\"]\ntypesense_api_key_file = {key}\n"
    );
    with_file("empty", &toml, |config| {
        // The compose pattern `VAR: ${VAR:-}`.
        std::env::set_var("TYPESENSE_HOST", "");
        std::env::set_var("TYPESENSE_PROBE_ALLOWED_TARGETS", "");
        std::env::set_var("TYPESENSE_API_KEY", "");

        let args = CliArgs::load_from(["exporter", config]).unwrap();
        assert_eq!(args.typesense_host.as_deref(), Some("filehost"));
        assert_eq!(args.probe_allowed_targets, ["a:1", "b"]);
        assert_eq!(
            args.api_key().unwrap(),
            "file-key",
            "the file's key is used"
        );

        // With nothing in the file either, an empty host is no host.
        std::env::remove_var("TYPESENSE_API_KEY");
        let args = CliArgs::load_from(["exporter", "--typesense-api-key=k"]).unwrap();
        assert_eq!(args.typesense_host, None);
    });
}

#[test]
fn an_environment_key_beats_the_files_key_settings() {
    let key = key_file("env-key", "file-key\n");
    for toml in [
        format!("typesense_api_key_file = {key}\n"),
        "typesense_api_key_env = \"NOT_SET_ANYWHERE\"\n".to_string(),
    ] {
        with_file("envkey", &toml, |config| {
            std::env::set_var("TYPESENSE_API_KEY", "env-key");
            let args = CliArgs::load_from(["exporter", config]).unwrap();
            assert_eq!(args.api_key().unwrap(), "env-key", "{toml}");
        });
    }
}

#[test]
fn list_and_enum_options_follow_the_same_precedence() {
    let toml = "probe_allowed_targets = [\"file1\", \"file2\"]\nmetric_names = \"legacy\"\n";
    with_file("vec", toml, |config| {
        let args = CliArgs::load_from(["exporter", config]).unwrap();
        assert_eq!(args.probe_allowed_targets, ["file1", "file2"]);
        assert_eq!(args.metric_names, MetricNames::Legacy);

        std::env::set_var("TYPESENSE_PROBE_ALLOWED_TARGETS", "env1,env2,env3");
        std::env::set_var("TYPESENSE_METRIC_NAMES", "both");
        let args = CliArgs::load_from(["exporter", config]).unwrap();
        assert_eq!(args.probe_allowed_targets, ["env1", "env2", "env3"]);
        assert_eq!(args.metric_names, MetricNames::Both);

        let args = CliArgs::load_from(["exporter", config, "--metric-names=v1"]).unwrap();
        assert_eq!(
            args.metric_names,
            MetricNames::V1,
            "flag beats the environment"
        );
    });
}

#[test]
fn every_blank_environment_variable_counts_as_unset() {
    let key = key_file("blank-env-key", "file-key\n");
    let toml = format!(
        "typesense_host = \"filehost\"\ntypesense_port = 1111\ntypesense_protocol = \"https\"\n\
         typesense_timeout = 7\nexporter_bind_address = \"127.0.0.1\"\nexporter_bind_port = 9999\n\
         metric_names = \"both\"\nprobe_max_concurrency = 3\nlog_level = \"debug\"\nlog_format = \"json\"\ntypesense_api_key_file = {key}\n"
    );
    with_file("blank", &toml, |config| {
        // Compose's `VAR: ${VAR:-}` sets every variable, most of them empty.
        for v in VARS {
            if v != "TYPESENSE_EXPORTER_CONFIG" {
                std::env::set_var(v, " ");
            }
        }
        let args = CliArgs::load_from(["exporter", config]).expect("blank variables are ignored");
        assert_eq!(args.typesense_host.as_deref(), Some("filehost"));
        assert_eq!(args.typesense_port, 1111);
        assert_eq!(args.typesense_protocol, "https");
        assert_eq!(args.typesense_timeout, 7);
        assert_eq!(args.exporter_bind_address, "127.0.0.1");
        assert_eq!(args.exporter_bind_port, 9999);
        assert_eq!(args.metric_names, MetricNames::Both);
        assert_eq!(args.probe_max_concurrency, 3);
        assert_eq!(args.log_level, LogLevel::Debug);
        assert_eq!(args.log_format, LogFormat::Json);
        assert_eq!(args.api_key().unwrap(), "file-key");

        // Without a file, blank variables give the built-in defaults.
        std::env::set_var("TYPESENSE_HOST", "h");
        std::env::set_var("TYPESENSE_API_KEY", "k");
        let args = CliArgs::load_from(["exporter"]).expect("defaults");
        assert_eq!(args.typesense_port, 8108);
        assert_eq!(args.metric_names, MetricNames::V1);
        assert_eq!(args.probe_max_concurrency, 16);
        assert_eq!(args.log_level, LogLevel::Info);
        assert_eq!(args.log_format, LogFormat::Text);
    });
}

#[test]
fn log_options_follow_flag_then_environment_then_file_then_default() {
    with_file(
        "log",
        "log_level = \"warn\"\nlog_format = \"json\"\n",
        |config| {
            let args = CliArgs::load_from(["exporter", config]).unwrap();
            assert_eq!(args.log_level, LogLevel::Warn, "file beats the default");
            assert_eq!(args.log_format, LogFormat::Json);

            std::env::set_var("TYPESENSE_EXPORTER_LOG_LEVEL", "debug");
            std::env::set_var("TYPESENSE_EXPORTER_LOG_FORMAT", "text");
            let args = CliArgs::load_from(["exporter", config]).unwrap();
            assert_eq!(
                args.log_level,
                LogLevel::Debug,
                "environment beats the file"
            );
            assert_eq!(args.log_format, LogFormat::Text);

            let args =
                CliArgs::load_from(["exporter", config, "--log-level=trace", "--log-format=json"])
                    .unwrap();
            assert_eq!(
                args.log_level,
                LogLevel::Trace,
                "flag beats the environment"
            );
            assert_eq!(args.log_format, LogFormat::Json);

            // Blank variables are ignored, so the file applies again.
            std::env::set_var("TYPESENSE_EXPORTER_LOG_LEVEL", " ");
            std::env::set_var("TYPESENSE_EXPORTER_LOG_FORMAT", "");
            let args = CliArgs::load_from(["exporter", config]).unwrap();
            assert_eq!(args.log_level, LogLevel::Warn);
            assert_eq!(args.log_format, LogFormat::Json);
        },
    );
}

#[test]
fn invalid_log_values_are_rejected_with_a_clear_error() {
    with_file("badlog", "", |config| {
        std::env::set_var("TYPESENSE_EXPORTER_LOG_LEVEL", "loud");
        let e = CliArgs::load_from(["exporter", config])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("loud") && e.contains("[possible values: error, warn, info, debug, trace]"),
            "{e}"
        );

        std::env::remove_var("TYPESENSE_EXPORTER_LOG_LEVEL");
        std::env::set_var("TYPESENSE_EXPORTER_LOG_FORMAT", "yaml");
        let e = CliArgs::load_from(["exporter", config])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("yaml") && e.contains("text") && e.contains("json"),
            "{e}"
        );
    });
    with_file("badlogfile", "log_level = \"loud\"\n", |config| {
        let e = CliArgs::load_from(["exporter", config])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("config file") && e.contains("log_level") || e.contains("at line"),
            "{e}"
        );
        assert!(!e.contains("loud"), "{e}");
    });
}
