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

/// An error message that never repeats a configured value: serde quotes the
/// value it rejects (`string "..."`, ``unknown variant `...` ``), which could
/// be a secret in the wrong field.
fn redact(message: &str) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(i) = rest.find("string \"") {
        out.push_str(&rest[..i]);
        out.push_str("a string");
        let quoted = &rest[i + "string \"".len()..];
        let mut escaped = false;
        let end = quoted
            .char_indices()
            .find(|&(_, c)| {
                let done = c == '"' && !escaped;
                escaped = c == '\\' && !escaped;
                done
            })
            .map_or(quoted.len(), |(n, _)| n + 1);
        rest = &quoted[end..];
    }
    out.push_str(rest);
    match out.split_once("unknown variant `") {
        Some((head, tail)) => match tail.split_once('`') {
            Some((_, tail)) => format!("{head}unknown variant{tail}"),
            None => format!("{head}unknown variant"),
        },
        None => out,
    }
}

fn parse(text: &str, path: &Path) -> Result<FileConfig, String> {
    toml::from_str(text).map_err(|e| {
        // Not `Display`: it quotes the offending source line, which could be
        // a secret. Name the file and line instead.
        let message = redact(e.message());
        let line = e.span().map_or_else(String::new, |span| {
            format!(" at line {}", text[..span.start].matches('\n').count() + 1)
        });
        let hint = if message.contains("unknown field `api_key`")
            || message.contains("unknown field `typesense_api_key`")
        {
            format!("; {NO_INLINE_SECRETS}")
        } else {
            String::new()
        };
        format!(
            "invalid config file {}{line}: {message}{hint}",
            path.display()
        )
    })
}

/// A valid environment variable name, so a mistakenly pasted key in a
/// `*_env` setting is never echoed back in an error.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 128
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Read an API key from the environment variable a `*_env` setting names.
/// `at` says where the setting is; errors never contain the key, and the
/// setting's value is only echoed when it is a valid variable name.
pub(crate) fn key_from_env(
    var: &str,
    setting: &str,
    at: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<String, String> {
    if !is_env_name(var) {
        return Err(format!(
            "{at}: {setting} must be an environment variable name"
        ));
    }
    match env(var).map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!(
            "{at}: environment variable {var} ({setting}) is not set or is empty"
        )),
    }
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// Accept only `http` or `https` (any case) and store it in lowercase. An
/// empty value (`TYPESENSE_PROTOCOL=`) means the default.
fn check_protocol(args: &mut CliArgs) -> Result<(), String> {
    let p = args.typesense_protocol.trim().to_ascii_lowercase();
    args.typesense_protocol = match p.as_str() {
        "" => "http".to_string(),
        "http" | "https" => p,
        _ => return Err("typesense_protocol must be http or https".to_string()),
    };
    Ok(())
}

/// Fill in `args` from the `--config` file, for every option the flags and
/// environment left at its default. `env` looks up `*_env` variables.
/// Also checks what no source can: the protocol, and empty values.
pub fn merge(
    args: &mut CliArgs,
    matches: &ArgMatches,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    // `TYPESENSE_HOST=` (a compose `${VAR:-}`) is not a host or a key.
    if args.typesense_host.as_deref().is_some_and(blank) {
        args.typesense_host = None;
    }
    if args.typesense_api_key.as_deref().is_some_and(blank) {
        args.typesense_api_key = None;
    }
    if args
        .typesense_api_key_file
        .as_deref()
        .is_some_and(|p| p.as_os_str().is_empty())
    {
        args.typesense_api_key_file = None;
    }
    let Some(path) = args.config.clone() else {
        return check_protocol(args);
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config file {}: {e}", path.display()))?;
    let file = parse(&text, &path)?;
    let at = format!("config file {}", path.display());

    // An environment variable that is set but empty counts as unset.
    let unset = |id: &str| match matches.value_source(id) {
        None | Some(ValueSource::DefaultValue) => true,
        Some(ValueSource::EnvVariable) => matches
            .get_raw(id)
            .is_some_and(|mut v| v.all(|s| blank(&s.to_string_lossy()))),
        Some(_) => false,
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
    check_protocol(args).map_err(|e| format!("{at}: {e}"))?;
    if args.typesense_host.as_deref().is_some_and(blank) {
        args.typesense_host = None;
    }

    // A key from the flags or environment replaces the file's key settings.
    if !args.has_key() {
        match (file.typesense_api_key_file, file.typesense_api_key_env) {
            (Some(_), Some(_)) => {
                return Err(format!(
                    "{at}: set only one of typesense_api_key_file and typesense_api_key_env"
                ))
            }
            (Some(key_file), None) => {
                args.typesense_api_key_file = Some(relative_to(&path, key_file))
            }
            (None, Some(var)) => {
                args.typesense_api_key =
                    Some(key_from_env(&var, "typesense_api_key_env", &at, &env)?);
            }
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
    fn invalid_values_name_the_file_and_the_line() {
        for toml in [
            "typesense_port = \"abc\"",
            "typesense_port = 99999",
            "metric_names = \"v2\"",
            "exporter_bind_port = -1",
            "probe_allowed_targets = [\"a\",\n1]",
        ] {
            let e = load(&[], toml, &[]).unwrap_err();
            assert!(
                e.contains("config file") && e.contains("at line"),
                "{toml}: {e}"
            );
        }
        let e = load(&[], "probe_max_concurrency = 0", &[]).unwrap_err();
        assert!(e.contains("probe_max_concurrency"), "{e}");
        let e = load(&[], "not toml [", &[]).unwrap_err();
        assert!(e.contains("invalid config file"), "{e}");
    }

    #[test]
    fn errors_never_contain_the_values_they_reject() {
        const S: &str = "SECRET-123";
        let cases = [
            format!("typesense_api_key_env = \"{S}\""),
            format!("[clusters.p]\ntargets = [\"a\"]\napi_key_env = \"{S}\""),
            format!("typesense_port = \"{S}\""),
            format!("typesense_timeout = \"{S}\""),
            format!("exporter_bind_port = \"{S}\""),
            format!("probe_max_concurrency = \"{S}\""),
            format!("metric_names = \"{S}\""),
            format!("typesense_protocol = \"{S}\""),
            format!("typesense_host = [\"{S}\"]"),
            format!("probe_allowed_targets = \"{S}\""),
            format!("[clusters.p]\ntargets = \"{S}\""),
            format!("[clusters.p]\ntargets = [\"a\"]\napi_key_env = \"K\"\nprotocol = \"{S}\""),
            format!("typesense_api_key_file = [\"{S}\"]"),
        ];
        for toml in &cases {
            let e = load(&[], toml, &[("K", "k")]).unwrap_err();
            assert!(!e.contains(S), "{toml}: {e}");
        }
        // A valid name that is unset is fine to name.
        let e = load(&[], "typesense_api_key_env = \"MISSING_VAR\"", &[]).unwrap_err();
        assert!(e.contains("MISSING_VAR"), "{e}");
        let e = load(&[], "typesense_api_key_env = \"a b\"", &[]).unwrap_err();
        assert!(e.contains("must be an environment variable name"), "{e}");
    }

    #[test]
    fn the_protocol_is_validated_and_lowercased_from_every_source() {
        for toml in ["typesense_protocol = \"ftp\"", "typesense_protocol = \"\""] {
            let e = load(
                &[],
                &format!("{toml}\ntypesense_api_key_env = \"K\""),
                &[("K", "k")],
            );
            assert!(toml.contains("ftp") == e.is_err(), "{toml}: {e:?}");
        }
        let toml =
            "typesense_protocol = \"HTTPS\"\n[clusters.p]\ntargets = [\"a\"]\napi_key_env = \"K\"";
        let a = load(&[], toml, &[("K", "k")]).unwrap();
        assert_eq!(a.typesense_protocol, "https");
        assert_eq!(a.clusters.clusters[0].protocol, "https");
        assert!(!a.sends_key_in_plaintext());

        let a = load(&["--typesense-protocol=HTTP"], "", &[]).unwrap();
        assert_eq!(a.typesense_protocol, "http");
        let e = load(&["--typesense-protocol=ftp"], "", &[]).unwrap_err();
        assert!(e.contains("http or https") && !e.contains("ftp\""), "{e}");

        // Without a config file, too.
        let argv = ["exporter", "--typesense-protocol=gopher"];
        let matches = CliArgs::command().try_get_matches_from(argv).unwrap();
        let mut args = CliArgs::from_arg_matches(&matches).unwrap();
        assert!(merge(&mut args, &matches, |_| None).is_err());
    }

    #[test]
    fn the_example_file_loads_once_a_key_is_given() {
        let example = include_str!("../deploy/exporter.example.toml");
        let a = load(
            &["--typesense-api-key=k", "--typesense-host=h"],
            example,
            &[],
        )
        .unwrap();
        assert!(a.clusters.clusters.is_empty(), "clusters are commented out");
        assert_eq!(a.typesense_port, 8108);
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
