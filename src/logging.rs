//! Log level, format and the subscriber the binary installs.
//!
//! `RUST_LOG`, when set, wins over `--log-level` and accepts full
//! `tracing_subscriber::EnvFilter` directives for finer control.

use clap::ValueEnum;
use serde::Deserialize;
use tracing::Subscriber;
use tracing_subscriber::{fmt::MakeWriter, EnvFilter};

use crate::cli::CliArgs;

/// How much to log. `info` is the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// How log lines are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable lines.
    #[default]
    Text,
    /// One JSON object per line.
    Json,
}

/// The filter for `level`, or for the `rust_log` directives when they are
/// set and not blank. `--log-level` applies to this crate only: HTTP client
/// internals stay at `warn` unless `RUST_LOG` asks for them.
pub fn filter(level: LogLevel, rust_log: Option<&str>) -> Result<EnvFilter, String> {
    match rust_log.map(str::trim).filter(|s| !s.is_empty()) {
        Some(directives) => EnvFilter::try_new(directives)
            .map_err(|e| format!("invalid RUST_LOG {directives:?}: {e}")),
        None => Ok(EnvFilter::new(format!(
            "warn,{}={}",
            env!("CARGO_CRATE_NAME"),
            level.as_str()
        ))),
    }
}

/// A subscriber writing to `writer`.
pub fn subscriber<W>(
    filter: EnvFilter,
    format: LogFormat,
    writer: W,
    ansi: bool,
) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(ansi);
    match format {
        LogFormat::Text => Box::new(builder.finish()),
        LogFormat::Json => Box::new(builder.json().finish()),
    }
}

/// Install the global subscriber from the loaded options and `RUST_LOG`.
/// Call after [`CliArgs::load`], since the level can come from the config file.
pub fn init(args: &CliArgs) -> Result<(), String> {
    let filter = filter(args.log_level, std::env::var("RUST_LOG").ok().as_deref())?;
    tracing::subscriber::set_global_default(subscriber(
        filter,
        args.log_format,
        std::io::stdout,
        true,
    ))
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buf {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    /// Emit one event per level from this crate and one `warn` and one
    /// `debug` from `reqwest`, and return what the filter let through.
    fn emitted(filter: EnvFilter, format: LogFormat) -> String {
        let buf = Buf::default();
        tracing::subscriber::with_default(subscriber(filter, format, buf.clone(), false), || {
            tracing::error!("ours-error");
            tracing::warn!("ours-warn");
            tracing::info!("ours-info");
            tracing::debug!("ours-debug");
            tracing::trace!("ours-trace");
            tracing::warn!(target: "reqwest", "dep-warn");
            tracing::debug!(target: "reqwest", "dep-debug");
        });
        let out = buf.0.lock().unwrap().clone();
        String::from_utf8(out).unwrap()
    }

    fn seen(out: &str) -> Vec<&'static str> {
        [
            "ours-error",
            "ours-warn",
            "ours-info",
            "ours-debug",
            "ours-trace",
            "dep-warn",
            "dep-debug",
        ]
        .into_iter()
        .filter(|m| out.contains(m))
        .collect()
    }

    #[test]
    fn each_level_shows_itself_and_everything_above_it_for_this_crate_only() {
        let at = |level| seen(&emitted(filter(level, None).unwrap(), LogFormat::Text));
        assert_eq!(at(LogLevel::Error), ["ours-error", "dep-warn"]);
        assert_eq!(at(LogLevel::Warn), ["ours-error", "ours-warn", "dep-warn"]);
        assert_eq!(
            at(LogLevel::Info),
            ["ours-error", "ours-warn", "ours-info", "dep-warn"]
        );
        assert_eq!(
            at(LogLevel::Debug),
            [
                "ours-error",
                "ours-warn",
                "ours-info",
                "ours-debug",
                "dep-warn"
            ]
        );
        assert_eq!(
            at(LogLevel::Trace),
            [
                "ours-error",
                "ours-warn",
                "ours-info",
                "ours-debug",
                "ours-trace",
                "dep-warn"
            ]
        );
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }

    #[test]
    fn rust_log_beats_the_log_level() {
        // --log-level=error alone would hide everything but errors.
        let f = filter(
            LogLevel::Error,
            Some("typesense_prometheus_exporter=debug,reqwest=debug"),
        );
        assert_eq!(
            seen(&emitted(f.unwrap(), LogFormat::Text)),
            [
                "ours-error",
                "ours-warn",
                "ours-info",
                "ours-debug",
                "dep-warn",
                "dep-debug"
            ]
        );
        // And it can be quieter than the level.
        let f = filter(LogLevel::Trace, Some("error"));
        assert_eq!(seen(&emitted(f.unwrap(), LogFormat::Text)), ["ours-error"]);
    }

    #[test]
    fn a_blank_rust_log_is_ignored_and_a_bad_one_is_an_error() {
        for blank in [None, Some(""), Some("  ")] {
            let f = filter(LogLevel::Warn, blank).unwrap();
            assert_eq!(
                seen(&emitted(f, LogFormat::Text)),
                ["ours-error", "ours-warn", "dep-warn"],
                "{blank:?}"
            );
        }
        let e = filter(LogLevel::Info, Some("typesense=bogus")).unwrap_err();
        assert!(e.contains("RUST_LOG"), "{e}");
    }

    #[test]
    fn json_format_writes_one_object_per_line() {
        let out = emitted(filter(LogLevel::Info, None).unwrap(), LogFormat::Json);
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).expect(l))
            .collect();
        assert!(!lines.is_empty());
        assert!(lines
            .iter()
            .any(|l| l["fields"]["message"] == "ours-info" && l["level"] == "INFO"));
    }
}
