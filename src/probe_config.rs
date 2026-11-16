//! `[clusters.<name>]` in the config file: Typesense clusters that each have their own API key.
//!
//! A `/probe` target belongs to the cluster that lists it and is scraped with
//! that cluster's key and protocol, so a key can only reach its own nodes. The
//! file names where a key lives (a file or an environment variable), never the
//! key itself, and keys are read once at startup.

use std::{collections::BTreeMap, fmt, path::Path};

use serde::Deserialize;

use crate::target::{normalize_host, parse_authority, Target};

/// An inline `api_key` is deliberately not a field: `deny_unknown_fields`
/// turns one into an error, keeping secrets out of config files.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterFile {
    pub targets: Vec<String>,
    pub api_key_file: Option<std::path::PathBuf>,
    pub api_key_env: Option<String>,
    pub protocol: Option<String>,
}

/// A cluster ready to probe: its nodes, protocol and API key.
#[derive(Clone)]
pub struct Cluster {
    pub name: String,
    /// `http` or `https`: the default scheme for this cluster's targets.
    pub protocol: String,
    /// Normalised host and port of each node.
    pub nodes: Vec<(String, u16)>,
    api_key: String,
}

impl Cluster {
    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}

/// Like the derived `Debug`, but never prints the API key.
impl fmt::Debug for Cluster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cluster")
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("nodes", &self.nodes)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ProbeConfig {
    pub clusters: Vec<Cluster>,
}

impl ProbeConfig {
    /// Validate the `[clusters]` read from the config file `path` (which only
    /// appears in errors) and read every cluster's key. `env` looks up
    /// `api_key_env` variables. Errors never contain key values.
    pub fn resolve(
        parsed: BTreeMap<String, ClusterFile>,
        path: &Path,
        default_port: u16,
        default_protocol: &str,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let file = path.display();
        let mut clusters: Vec<Cluster> = Vec::new();
        for (name, c) in parsed {
            let at = format!("config file {file}: cluster {name:?}");
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            {
                return Err(format!("{at}: name must match [A-Za-z0-9_-]+"));
            }

            let protocol = match c.protocol.as_deref() {
                None => None,
                Some(p) if p.eq_ignore_ascii_case("http") || p.eq_ignore_ascii_case("https") => {
                    Some(p.to_ascii_lowercase())
                }
                Some(_) => return Err(format!("{at}: protocol must be http or https")),
            };

            if c.targets.is_empty() {
                return Err(format!("{at}: targets is empty"));
            }
            let mut nodes: Vec<(String, u16)> = Vec::new();
            for t in &c.targets {
                let node = parse_authority(t.trim(), default_port)
                    .map_err(|e| format!("{at}: invalid target {t:?}: {e}"))?;
                if let Some(other) = clusters.iter().find(|o| o.nodes.contains(&node)) {
                    return Err(format!(
                        "{file}: {}:{} is listed in both cluster {:?} and cluster {name:?}",
                        node.0, node.1, other.name
                    ));
                }
                if !nodes.contains(&node) {
                    nodes.push(node);
                }
            }

            let api_key = match (c.api_key_file, c.api_key_env) {
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "{at}: set only one of api_key_file and api_key_env"
                    ))
                }
                (None, None) => {
                    return Err(format!("{at}: set api_key_file or api_key_env"));
                }
                (Some(key_file), None) => {
                    let key_file = crate::config::relative_to(path, key_file);
                    crate::cli::read_key_file(&key_file).map_err(|e| {
                        format!("{at}: cannot read api_key_file {}: {e}", key_file.display())
                    })?
                }
                (None, Some(var)) => crate::config::key_from_env(&var, "api_key_env", &at, &env)?,
            };

            clusters.push(Cluster {
                name,
                protocol: protocol.unwrap_or_else(|| default_protocol.to_ascii_lowercase()),
                nodes,
                api_key,
            });
        }
        Ok(Self { clusters })
    }

    /// The cluster that lists this target's host and port.
    pub fn find(&self, target: &Target) -> Option<&Cluster> {
        self.clusters.iter().find(|c| {
            c.nodes
                .iter()
                .any(|(h, p)| *h == target.host && *p == target.port)
        })
    }

    /// The cluster listing this node, if any.
    pub fn find_node(&self, host: &str, port: u16) -> Option<&Cluster> {
        let host = normalize_host(host);
        self.clusters
            .iter()
            .find(|c| c.nodes.iter().any(|(h, p)| *h == host && *p == port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_with(text: &str, env: &[(&str, &str)]) -> Result<ProbeConfig, String> {
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            clusters: BTreeMap<String, ClusterFile>,
        }
        let wrap: Wrap = toml::from_str(text).map_err(|e| e.message().to_string())?;
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ProbeConfig::resolve(
            wrap.clusters,
            Path::new("/etc/probe.toml"),
            8108,
            "http",
            |name| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
        )
    }

    fn key_file(contents: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "ts-probe-key-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn target(s: &str) -> Target {
        Target::parse(s, "http", 8108).unwrap()
    }

    #[test]
    fn reads_keys_from_a_file_and_from_the_environment() {
        let f = key_file("  prod-key\n");
        let text = format!(
            r#"
            [clusters.prod]
            targets = ["ts-prod-1:8108", "TS-PROD-2"]
            api_key_file = "{}"

            [clusters.staging]
            targets = ["ts-staging-1:9000"]
            api_key_env = "STAGING_KEY"
            "#,
            f.display()
        );
        let cfg = parse_with(&text, &[("STAGING_KEY", " staging-key ")]).unwrap();
        std::fs::remove_file(&f).unwrap();

        assert_eq!(cfg.clusters.len(), 2);
        let prod = cfg.find(&target("ts-prod-2")).unwrap();
        assert_eq!((prod.name.as_str(), prod.api_key()), ("prod", "prod-key"));
        let staging = cfg.find(&target("ts-staging-1:9000")).unwrap();
        assert_eq!(staging.api_key(), "staging-key");
        assert!(cfg.find(&target("ts-staging-1")).is_none(), "other port");
        assert!(cfg.find(&target("other:8108")).is_none());
        assert!(cfg.find_node("TS-PROD-1", 8108).is_some());
        assert!(cfg.find_node("TS-PROD-1", 1).is_none());
    }

    #[test]
    fn protocol_overrides_the_default_and_ports_default() {
        let cfg = parse_with(
            r#"
            [clusters.a]
            targets = ["h1", "[::1]"]
            api_key_env = "K"
            protocol = "HTTPS"
            [clusters.b]
            targets = ["h2:1"]
            api_key_env = "K"
            "#,
            &[("K", "k")],
        )
        .unwrap();
        let a = cfg.find(&target("h1:8108")).unwrap();
        assert_eq!(a.protocol, "https");
        assert!(cfg.find(&target("[::1]:8108")).is_some());
        assert_eq!(cfg.find(&target("h2:1")).unwrap().protocol, "http");
    }

    fn err(text: &str, env: &[(&str, &str)]) -> String {
        parse_with(text, env).unwrap_err()
    }

    #[test]
    fn rejects_missing_empty_or_doubled_keys_naming_the_cluster() {
        let none = err("[clusters.p]\ntargets=[\"a\"]", &[]);
        assert!(
            none.contains("\"p\"") && none.contains("/etc/probe.toml"),
            "{none}"
        );

        let unset = err("[clusters.p]\ntargets=[\"a\"]\napi_key_env=\"NOPE\"", &[]);
        assert!(unset.contains("NOPE") && unset.contains("\"p\""), "{unset}");
        let blank = err(
            "[clusters.p]\ntargets=[\"a\"]\napi_key_env=\"E\"",
            &[("E", "  ")],
        );
        assert!(blank.contains("not set or is empty"), "{blank}");

        let empty_file = key_file("\n");
        let e = err(
            &format!(
                "[clusters.p]\ntargets=[\"a\"]\napi_key_file=\"{}\"",
                empty_file.display()
            ),
            &[],
        );
        std::fs::remove_file(&empty_file).unwrap();
        assert!(e.contains("\"p\"") && e.contains("is empty"), "{e}");

        let e = err(
            "[clusters.p]\ntargets=[\"a\"]\napi_key_file=\"/nonexistent/key\"",
            &[],
        );
        assert!(e.contains("/nonexistent/key"), "{e}");

        let both = err(
            "[clusters.p]\ntargets=[\"a\"]\napi_key_file=\"/k\"\napi_key_env=\"E\"",
            &[("E", "k")],
        );
        assert!(both.contains("only one"), "{both}");
    }

    #[test]
    fn rejects_bad_names_targets_protocols_and_empty_configs() {
        let base = |name: &str, targets: &str, extra: &str| {
            format!("[clusters.\"{name}\"]\ntargets={targets}\napi_key_env=\"K\"\n{extra}")
        };
        let env = [("K", "k")];
        for bad in ["", "a b", "a.b", "pröd", "a/b"] {
            let e = err(&base(bad, "[\"a\"]", ""), &env);
            assert!(e.contains("[A-Za-z0-9_-]+"), "{bad:?}: {e}");
        }
        assert!(parse_with(&base("Prod_1-b", "[\"a\"]", ""), &env).is_ok());

        for bad in ["http://a", "a/b", "a:0", "user@a", "::1", ""] {
            let e = err(&base("p", &format!("[\"{bad}\"]"), ""), &env);
            assert!(
                e.contains("invalid target") && e.contains("\"p\""),
                "{bad:?}: {e}"
            );
        }
        assert!(err(&base("p", "[]", ""), &env).contains("targets is empty"));
        assert!(err(&base("p", "[\"a\"]", "protocol=\"ftp\""), &env).contains("http or https"));
    }

    #[test]
    fn rejects_a_node_in_two_clusters_but_not_twice_in_one() {
        let one = parse_with(
            "[clusters.a]\ntargets=[\"x:1\",\"X:1\"]\napi_key_env=\"K\"",
            &[("K", "k")],
        )
        .unwrap();
        assert_eq!(one.clusters[0].nodes.len(), 1);

        let e = err(
            "[clusters.a]\ntargets=[\"x:1\"]\napi_key_env=\"K\"\n[clusters.b]\ntargets=[\"y\",\"X:1\"]\napi_key_env=\"K\"",
            &[("K", "k")],
        );
        assert!(
            e.contains("x:1") && e.contains("\"a\"") && e.contains("\"b\""),
            "{e}"
        );
    }

    #[test]
    fn debug_never_prints_the_key() {
        let cfg = parse_with(
            "[clusters.a]\ntargets=[\"x\"]\napi_key_env=\"K\"",
            &[("K", "topsecret")],
        )
        .unwrap();
        assert!(!format!("{cfg:?}").contains("topsecret"));
    }
}
