//! `--config`: a TOML file that can set every option.
//!
//! Precedence is command-line flag, then environment variable, then this
//! file, then the built-in default. Secrets are never read inline: the file
//! names a key file or an environment variable instead.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use clap::{parser::ValueSource, ArgMatches};
use serde::Deserialize;

use crate::{
    cli::{CliArgs, MetricNames},
    probe_config::{ClusterFile, ProbeConfig},
};

/// Same names as the command-line options, in snake_case. An inline API key is
/// not a field, so `deny_unknown_fields` rejects it (and any typo).
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    typesense_host: Option<String>,
    typesense_protocol: Option<String>,
    typesense_port: Option<u16>,
    typesense_timeout: Option<u64>,
    typesense_api_key_file: Option<PathBuf>,
    typesense_api_key_env: Option<String>,
    exporter_bind_address: Option<String>,
    exporter_bind_port: Option<u16>,
    metric_names: Option<MetricNames>,
    probe_allowed_targets: Option<Vec<String>>,
    probe_max_concurrency: Option<u32>,
    #[serde(default)]
    clusters: BTreeMap<String, ClusterFile>,
}

const NO_INLINE_SECRETS: &str = "secrets are not accepted inline: use typesense_api_key_file or typesense_api_key_env (api_key_file or api_key_env in a cluster)";

fn parse(text: &str, path: &Path) -> Result<FileConfig, String> {
    toml::from_str(text).map_err(|e| {
        // Not `Display`: it quotes the offending source line, which could be
        // a secret. Name the line and its key instead.
        let message = e.message();
        let (line, key) = match e.span() {
            Some(span) => {
                let n = text[..span.start].matches('\n').count();
                let key = text[..span.start]
                    .rsplit('\n')
                    .next()
                    .and_then(|l| l.split_once('='))
                    .map(|(k, _)| format!(" (key {})", k.trim()))
                    .unwrap_or_default();
                (format!(" at line {}", n + 1), key)
            }
            None => Default::default(),
        };
        let hint = if message.contains("unknown field `api_key`")
            || message.contains("unknown field `typesense_api_key`")
        {
            format!("; {NO_INLINE_SECRETS}")
        } else {
            String::new()
        };
        format!(
            "invalid config file {}{line}{key}: {message}{hint}",
            path.display()
        )
    })
}

/// Fill in `args` from the `--config` file, for every option the flags and
/// environment left at its default. `env` looks up `*_env` variables.
pub fn merge(
    args: &mut CliArgs,
    matches: &ArgMatches,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let Some(path) = args.config.clone() else {
        return Ok(());
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config file {}: {e}", path.display()))?;
    let file = parse(&text, &path)?;
    let at = format!("config file {}", path.display());

    let unset = |id: &str| {
        matches!(
            matches.value_source(id),
            None | Some(ValueSource::DefaultValue)
        )
    };
    macro_rules! take {
        ($field:ident) => {
            if unset(stringify!($field)) {
                if let Some(v) = file.$field {
                    args.$field = v;
                }
            }
        };
        ($field:ident, some) => {
            if unset(stringify!($field)) {
                if let Some(v) = file.$field {
                    args.$field = Some(v);
                }
            }
        };
    }
    take!(typesense_host, some);
    take!(typesense_protocol);
    take!(typesense_port);
    take!(typesense_timeout);
    take!(exporter_bind_address);
    take!(exporter_bind_port);
    take!(metric_names);
    take!(probe_allowed_targets);
    if unset("probe_max_concurrency") {
        if let Some(v) = file.probe_max_concurrency {
            if v == 0 {
                return Err(format!("{at}: probe_max_concurrency must be at least 1"));
            }
            args.probe_max_concurrency = v;
        }
    }

    // A key from the flags or environment replaces the file's key settings.
    let flag_key = args.typesense_api_key.is_some() || args.typesense_api_key_file.is_some();
    if !flag_key {
        match (file.typesense_api_key_file, file.typesense_api_key_env) {
            (Some(_), Some(_)) => {
                return Err(format!(
                    "{at}: set only one of typesense_api_key_file and typesense_api_key_env"
                ))
            }
            (Some(key_file), None) => {
                args.typesense_api_key_file = Some(relative_to(&path, key_file))
            }
            (None, Some(var)) => match env(&var).map(|v| v.trim().to_string()) {
                Some(v) if !v.is_empty() => args.typesense_api_key = Some(v),
                _ => {
                    return Err(format!(
                        "{at}: environment variable {var} (typesense_api_key_env) is not set or is empty"
                    ))
                }
            },
            (None, None) => {}
        }
    }

    args.clusters = ProbeConfig::resolve(
        file.clusters,
        &path,
        args.typesense_port,
        &args.typesense_protocol,
        env,
    )?;
    Ok(())
}

/// A relative path in the config file is relative to the file's directory,
/// so the config works the same from any working directory.
pub(crate) fn relative_to(config: &std::path::Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    config
        .parent()
        .map_or_else(|| path.clone(), |dir| dir.join(&path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn relative_paths_are_relative_to_the_config_file() {
        let cfg = std::path::Path::new("/etc/exporter/exporter.toml");
        assert_eq!(
            relative_to(cfg, "secrets/key".into()),
            PathBuf::from("/etc/exporter/secrets/key")
        );
        assert_eq!(
            relative_to(cfg, "/run/key".into()),
            PathBuf::from("/run/key")
        );
        assert_eq!(
            relative_to(std::path::Path::new("exporter.toml"), "key".into()),
            PathBuf::from("key")
        );
    }

    use super::*;
    use clap::{CommandFactory, FromArgMatches};

    fn temp(name: &str, contents: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "ts-config-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Parse `flags` plus `--config <file with toml>`, with `env` standing in
    /// for the environment of `*_env` keys.
    fn load(flags: &[&str], toml: &str, env: &[(&str, &str)]) -> Result<CliArgs, String> {
        let file = temp("config.toml", toml);
        let argv: Vec<String> = [
            "exporter".to_string(),
            format!("--config={}", file.display()),
        ]
        .into_iter()
        .chain(flags.iter().map(|f| f.to_string()))
        .collect();
        let matches = CliArgs::command().try_get_matches_from(argv).unwrap();
        let mut args = CliArgs::from_arg_matches(&matches).unwrap();
        let result = merge(&mut args, &matches, |n| {
            env.iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.to_string())
        });
        std::fs::remove_file(&file).unwrap();
        result.map(|()| args)
    }

    #[test]
    fn every_option_can_be_set_in_the_file() {
        let key = temp("key", "k\n");
        let toml = format!(
            r#"
            typesense_host = "ts.example"
            typesense_protocol = "https"
            typesense_port = 443
            typesense_timeout = 9
            typesense_api_key_file = {:?}
            exporter_bind_address = "127.0.0.1"
            exporter_bind_port = 9100
            metric_names = "both"
            probe_allowed_targets = ["a:1", "b"]
            probe_max_concurrency = 3
            [clusters.prod]
            targets = ["p1"]
            api_key_env = "PROD_KEY"
            "#,
            key.display().to_string()
        );
        let a = load(&[], &toml, &[("PROD_KEY", "pk")]).unwrap();

        assert_eq!(a.typesense_host.as_deref(), Some("ts.example"));
        assert_eq!(a.typesense_protocol, "https");
        assert_eq!(a.typesense_port, 443);
        assert_eq!(a.typesense_timeout, 9);
        assert_eq!(a.api_key().unwrap(), "k");
        std::fs::remove_file(&key).unwrap();
        assert_eq!(a.exporter_bind_address, "127.0.0.1");
        assert_eq!(a.exporter_bind_port, 9100);
        assert_eq!(a.metric_names, MetricNames::Both);
        assert_eq!(a.probe_allowed_targets, ["a:1", "b"]);
        assert_eq!(a.probe_max_concurrency, 3);
        let prod = &a.clusters.clusters[0];
        assert_eq!(
            (prod.name.as_str(), prod.api_key(), prod.protocol.as_str()),
            ("prod", "pk", "https")
        );
        assert_eq!(
            prod.nodes,
            [("p1".to_string(), 443)],
            "cluster inherits file port"
        );
    }

    #[test]
    fn flags_beat_the_file_and_the_file_beats_defaults() {
        let toml = "typesense_host = \"filehost\"\ntypesense_port = 1111\ntypesense_timeout = 7\nexporter_bind_port = 1\nmetric_names = \"legacy\"\nprobe_allowed_targets = [\"x\"]";
        let a = load(
            &[
                "--typesense-host=cli",
                "--typesense-port=2222",
                "--metric-names=v1",
                "--typesense-api-key=k",
            ],
            toml,
            &[],
        )
        .unwrap();
        assert_eq!(a.typesense_host.as_deref(), Some("cli"));
        assert_eq!(a.typesense_port, 2222);
        assert_eq!(
            a.metric_names,
            MetricNames::V1,
            "an explicit default still wins"
        );
        assert_eq!(a.typesense_timeout, 7, "file beats the built-in default");
        assert_eq!(a.exporter_bind_port, 1);
        assert_eq!(a.probe_allowed_targets, ["x"]);
    }

    #[test]
    fn a_flag_key_replaces_the_files_key_settings() {
        let toml = "typesense_api_key_env = \"NOPE\"";
        let a = load(&["--typesense-api-key=cli-key"], toml, &[]).unwrap();
        assert_eq!(a.api_key().unwrap(), "cli-key");

        let a = load(&[], "typesense_api_key_env = \"E\"", &[("E", " envkey ")]).unwrap();
        assert_eq!(a.api_key().unwrap(), "envkey");
    }

    #[test]
    fn rejects_bad_key_settings_in_the_file() {
        let e = load(&[], "typesense_api_key_env = \"NOPE\"", &[]).unwrap_err();
        assert!(e.contains("NOPE") && e.contains("config file"), "{e}");
        let e = load(&[], "typesense_api_key_env = \"E\"", &[("E", "")]).unwrap_err();
        assert!(e.contains("not set or is empty"), "{e}");
        let e = load(
            &[],
            "typesense_api_key_env = \"E\"\ntypesense_api_key_file = \"/k\"",
            &[("E", "k")],
        )
        .unwrap_err();
        assert!(e.contains("only one"), "{e}");
    }

    #[test]
    fn rejects_unknown_keys_and_inline_secrets_without_echoing_them() {
        let e = load(&[], "typesense_hots = \"x\"", &[]).unwrap_err();
        assert!(e.contains("typesense_hots") && e.contains("line 1"), "{e}");

        for toml in [
            "typesense_api_key = \"inline-secret\"",
            "[clusters.p]\ntargets = [\"a\"]\napi_key = \"inline-secret\"",
        ] {
            let e = load(&[], toml, &[]).unwrap_err();
            assert!(
                e.contains("typesense_api_key_file or typesense_api_key_env"),
                "{e}"
            );
            assert!(!e.contains("inline-secret"), "{e}");
        }
        let e = load(&[], "[nope]\nx = 1", &[]).unwrap_err();
        assert!(e.contains("unknown field"), "{e}");
    }

    #[test]
    fn invalid_values_name_the_file_and_the_key() {
        for (toml, key) in [
            ("typesense_port = \"abc\"", "typesense_port"),
            ("typesense_port = 99999", "typesense_port"),
            ("metric_names = \"v2\"", "metric_names"),
            ("exporter_bind_port = -1", "exporter_bind_port"),
        ] {
            let e = load(&[], toml, &[]).unwrap_err();
            assert!(e.contains("config file") && e.contains(key), "{toml}: {e}");
        }
        let e = load(&[], "probe_max_concurrency = 0", &[]).unwrap_err();
        assert!(e.contains("probe_max_concurrency"), "{e}");
        let e = load(&[], "not toml [", &[]).unwrap_err();
        assert!(e.contains("invalid config file"), "{e}");
    }

    #[test]
    fn a_missing_config_file_is_an_error_and_no_file_changes_nothing() {
        let argv = ["exporter", "--config=/nonexistent/exporter.toml"];
        let matches = CliArgs::command().try_get_matches_from(argv).unwrap();
        let mut args = CliArgs::from_arg_matches(&matches).unwrap();
        let e = merge(&mut args, &matches, |_| None).unwrap_err();
        assert!(e.contains("/nonexistent/exporter.toml"), "{e}");

        let matches = CliArgs::command()
            .try_get_matches_from(["exporter"])
            .unwrap();
        let mut args = CliArgs::from_arg_matches(&matches).unwrap();
        merge(&mut args, &matches, |_| None).unwrap();
        assert_eq!(args.typesense_port, 8108);
    }
}
