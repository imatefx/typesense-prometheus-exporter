//! Probe targets (`/probe?target=...`) and the allow list that guards them.
//!
//! The exporter sends its Typesense API key to whatever node it scrapes, so a
//! target is parsed strictly and only contacted when the allow list says so.

use std::net::Ipv6Addr;

/// A Typesense node to scrape, as named by a `/probe` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `http` or `https`.
    pub scheme: String,
    /// Lowercase host name or address, without IPv6 brackets.
    pub host: String,
    pub port: u16,
}

impl Target {
    /// Parse `host`, `host:port`, `http(s)://host[:port]` or an IPv6 form such
    /// as `[::1]:8108`. A missing scheme or port takes the given default.
    /// Paths, queries, fragments, userinfo and other schemes are rejected.
    pub fn parse(input: &str, default_scheme: &str, default_port: u16) -> Result<Self, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("target is empty".to_string());
        }
        let (scheme, rest) = match input.split_once("://") {
            Some((scheme, rest)) => {
                let scheme = scheme.to_ascii_lowercase();
                if scheme != "http" && scheme != "https" {
                    return Err(format!("unsupported scheme {scheme:?}; use http or https"));
                }
                (scheme, rest)
            }
            None => (default_scheme.to_ascii_lowercase(), input),
        };
        let (host, port) = parse_authority(rest, default_port)?;
        Ok(Self { scheme, host, port })
    }

    /// `scheme://host:port`, with IPv6 hosts in brackets.
    pub fn base_url(&self) -> String {
        format!("{}://{}:{}", self.scheme, url_host(&self.host), self.port)
    }
}

/// `host[:port]` or `[v6][:port]`, normalised to a lowercase host and a port.
pub(crate) fn parse_authority(s: &str, default_port: u16) -> Result<(String, u16), String> {
    if s.contains(['/', '?', '#']) {
        return Err("target must not contain a path, query or fragment".to_string());
    }
    if s.contains('@') {
        return Err("target must not contain credentials".to_string());
    }
    if s.contains(char::is_whitespace) {
        return Err("target must not contain whitespace".to_string());
    }

    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (addr, after) = rest
            .split_once(']')
            .ok_or("missing closing ] in IPv6 target")?;
        let addr: Ipv6Addr = addr
            .parse()
            .map_err(|_| "invalid IPv6 address".to_string())?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or("unexpected text after ]")?),
        };
        (addr.to_string(), port)
    } else {
        match s.split_once(':') {
            None => (s.to_ascii_lowercase(), None),
            Some((_, port)) if port.contains(':') => {
                return Err("IPv6 addresses must be written in brackets, e.g. [::1]:8108".into())
            }
            Some((host, port)) => (host.to_ascii_lowercase(), Some(port)),
        }
    };
    // `ts.example.` is the same node as `ts.example`.
    let host = host.strip_suffix('.').map_or(host.clone(), str::to_string);

    if !valid_host(&host) {
        return Err("invalid host".to_string());
    }
    let port = match port {
        None => default_port,
        Some(p) => match p.parse::<u16>() {
            Ok(p) if p > 0 => p,
            _ => return Err(format!("invalid port {p:?}")),
        },
    };
    Ok((host, port))
}

fn valid_host(host: &str) -> bool {
    host.parse::<Ipv6Addr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
}

fn url_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// Lowercase a configured host, drop IPv6 brackets, write IPv6 addresses
/// canonically and drop one trailing dot, so it compares equal to a parsed
/// target's host.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host.parse::<Ipv6Addr>() {
        Ok(addr) => addr.to_string(),
        Err(_) => {
            let host = host.to_ascii_lowercase();
            host.strip_suffix('.').map_or(host.clone(), str::to_string)
        }
    }
}

/// Whether the host is this machine, where a plaintext key never leaves it.
pub fn is_loopback(host: &str) -> bool {
    matches!(
        normalize_host(host).as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// The nodes `/probe` may contact: normalised host plus port, any scheme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowList {
    any: bool,
    nodes: Vec<(String, u16)>,
}

impl AllowList {
    /// Build from `host[:port]` entries (a missing port is `default_port`).
    /// A lone `*` allows every target. Blank entries are skipped.
    pub fn parse(entries: &[String], default_port: u16) -> Result<Self, String> {
        let mut list = Self::default();
        for entry in entries.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            if entry == "*" {
                list.any = true;
            } else {
                let node = parse_authority(entry, default_port)
                    .map_err(|e| format!("invalid probe allowed target {entry:?}: {e}"))?;
                list.nodes.push(node);
            }
        }
        Ok(list)
    }

    /// Always allow this node (the configured `--typesense-host`).
    pub fn allow(&mut self, host: &str, port: u16) {
        self.nodes.push((normalize_host(host), port));
    }

    pub fn allows(&self, target: &Target) -> bool {
        self.any
            || self
                .nodes
                .iter()
                .any(|(host, port)| *host == target.host && *port == target.port)
    }

    /// Whether the list permits any node but this machine.
    pub fn has_remote(&self) -> bool {
        self.any || self.nodes.iter().any(|(host, _)| !is_loopback(host))
    }

    pub fn is_empty(&self) -> bool {
        !self.any && self.nodes.is_empty()
    }

    /// Whether this exact node is listed (not counting `*`).
    pub fn lists(&self, host: &str, port: u16) -> bool {
        let host = normalize_host(host);
        self.nodes.iter().any(|(h, p)| *h == host && *p == port)
    }

    pub fn allows_any(&self) -> bool {
        self.any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<Target, String> {
        Target::parse(input, "http", 8108)
    }

    fn ok(input: &str) -> (String, String, u16) {
        let t = parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
        (t.scheme, t.host, t.port)
    }

    fn tuple(scheme: &str, host: &str, port: u16) -> (String, String, u16) {
        (scheme.into(), host.into(), port)
    }

    #[test]
    fn bare_host_takes_defaults() {
        assert_eq!(ok("typesense-1"), tuple("http", "typesense-1", 8108));
        assert_eq!(
            Target::parse("ts", "https", 443).unwrap(),
            Target {
                scheme: "https".into(),
                host: "ts".into(),
                port: 443
            }
        );
    }

    #[test]
    fn host_with_port() {
        assert_eq!(
            ok("ts.example.com:9000"),
            tuple("http", "ts.example.com", 9000)
        );
        assert_eq!(ok("10.0.0.5:8109"), tuple("http", "10.0.0.5", 8109));
    }

    #[test]
    fn explicit_schemes() {
        assert_eq!(ok("http://ts"), tuple("http", "ts", 8108));
        assert_eq!(ok("https://ts:443"), tuple("https", "ts", 443));
        assert_eq!(ok("HTTPS://ts"), tuple("https", "ts", 8108));
    }

    #[test]
    fn hosts_are_lowercased() {
        assert_eq!(
            ok("TS-1.Example.COM:81"),
            tuple("http", "ts-1.example.com", 81)
        );
        assert_eq!(
            ok("https://Node.Example.com"),
            tuple("https", "node.example.com", 8108)
        );
    }

    #[test]
    fn ipv6_in_brackets() {
        assert_eq!(ok("[::1]:8108"), tuple("http", "::1", 8108));
        assert_eq!(ok("[::1]"), tuple("http", "::1", 8108));
        assert_eq!(
            ok("https://[2001:DB8::1]:443"),
            tuple("https", "2001:db8::1", 443)
        );
        assert_eq!(ok("[0:0:0:0:0:0:0:1]"), tuple("http", "::1", 8108));
    }

    #[test]
    fn base_url_brackets_ipv6() {
        assert_eq!(parse("[::1]:9").unwrap().base_url(), "http://[::1]:9");
        assert_eq!(parse("TS:9").unwrap().base_url(), "http://ts:9");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(ok("  ts:1 "), tuple("http", "ts", 1));
    }

    #[test]
    fn rejects_bad_targets() {
        for bad in [
            "",
            "   ",
            "ftp://ts",
            "file:///etc/passwd",
            "ts/path",
            "http://ts/",
            "http://ts:8108/metrics",
            "ts?x=1",
            "ts#frag",
            "user@ts",
            "http://user:pw@ts",
            "ts:0",
            "ts:65536",
            "ts:abc",
            "ts:",
            ":8108",
            "http://",
            "a b",
            "::1",
            "2001:db8::1",
            "[::1",
            "[::1]x",
            "[::1]8108",
            "[nonsense]:1",
            "[]",
            "ts:1:2",
            "ex$ample",
            "tö.example",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn normalizes_and_detects_loopback() {
        assert_eq!(normalize_host("[::1]"), "::1");
        assert_eq!(normalize_host("LocalHost"), "localhost");
        assert_eq!(normalize_host("0:0::1"), "::1");
        assert_eq!(normalize_host("[2001:DB8:0::1]"), "2001:db8::1");
        assert_eq!(normalize_host("Ts-Prod-1."), "ts-prod-1");
        assert!(is_loopback("0:0::1"));
        for local in ["localhost", "127.0.0.1", "::1", "[::1]", "LOCALHOST"] {
            assert!(is_loopback(local), "{local}");
        }
        assert!(!is_loopback("ts"));
        assert!(!is_loopback("127.0.0.2"));
    }

    #[test]
    fn a_trailing_dot_is_the_same_host() {
        assert_eq!(ok("ts-prod-1.:9000"), tuple("http", "ts-prod-1", 9000));
        assert_eq!(ok("http://ts."), tuple("http", "ts", 8108));
        assert!(parse(".").is_err());
    }

    fn list(entries: &[&str]) -> AllowList {
        let entries: Vec<String> = entries.iter().map(|s| s.to_string()).collect();
        AllowList::parse(&entries, 8108).unwrap()
    }

    fn target(s: &str) -> Target {
        parse(s).unwrap()
    }

    #[test]
    fn allow_list_matches_host_and_port_ignoring_scheme_and_case() {
        let l = list(&["Typesense-1", "ts2:9000", "[::1]:8108"]);
        assert!(l.allows(&target("typesense-1")));
        assert!(l.allows(&target("https://TYPESENSE-1:8108")));
        assert!(l.allows(&target("ts2:9000")));
        assert!(l.allows(&target("[::1]")));
        assert!(!l.allows(&target("ts2")), "default port differs");
        assert!(!l.allows(&target("typesense-1:8109")));
        assert!(!l.allows(&target("other")));
        assert!(!l.allows_any());
    }

    #[test]
    fn allow_list_wildcard_allows_everything() {
        let l = list(&["*"]);
        assert!(l.allows(&target("anything:1")));
        assert!(l.allows_any());
        assert!(l.has_remote());
    }

    #[test]
    fn empty_allow_list_allows_nothing_until_a_node_is_added() {
        let mut l = list(&["", "  "]);
        assert!(!l.allows(&target("ts")));
        l.allow("[::1]", 8108);
        l.allow("TS", 8108);
        assert!(l.allows(&target("[::1]")));
        assert!(l.allows(&target("ts")));
    }

    #[test]
    fn allow_list_rejects_bad_entries() {
        for bad in ["http://ts", "ts/x", "ts:0", "user@ts", "::1"] {
            let err = AllowList::parse(&[bad.to_string()], 8108).unwrap_err();
            assert!(err.contains(bad), "{err}");
        }
    }

    #[test]
    fn lists_matches_exact_nodes_only() {
        let l = list(&["Ts:9", "*"]);
        assert!(l.lists("TS", 9));
        assert!(!l.lists("ts", 8108));
        assert!(!l.lists("other", 9), "* is not a listed node");
    }

    #[test]
    fn has_remote_ignores_loopback_entries() {
        assert!(!list(&["localhost", "127.0.0.1:9", "[::1]"]).has_remote());
        assert!(list(&["localhost", "ts"]).has_remote());
        assert!(!list(&[]).has_remote());
    }
}
