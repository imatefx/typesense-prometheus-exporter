use std::{collections::BTreeMap, sync::OnceLock};

use prometheus::{GaugeVec, Opts, Registry, TextEncoder};
use regex::Regex;
use serde_json::{Map, Value};

use crate::typesense::models::{
    typesense_collection_model::TypesenseCollection, typesense_metrics_model::TypesenseMetrics,
    typesense_stats_model::TypesenseStats, typesense_status_model::TypesenseStatus,
};

const BASE_LABELS: &[&str] = &["host", "port", "key"];
const COLLECTION_LABELS: &[&str] = &["host", "port", "method", "collection_name", "action"];
const TARGET_LABELS: &[&str] = &["host", "port"];

/// Results of one scrape of a Typesense node. Endpoints that failed are `None`
/// and their metric families are omitted rather than reported as zeros.
#[derive(Debug, Default)]
pub struct Scrape {
    pub metrics: Option<TypesenseMetrics>,
    pub stats: Option<TypesenseStats>,
    pub healthy: bool,
    /// Raft role from `/debug`: 1 = leader, 4 = follower, 0 = other/unknown.
    pub raft_state: i32,
    /// Typesense version from `/debug`.
    pub version: Option<String>,
    pub status: Option<TypesenseStatus>,
    pub collections: Option<Vec<TypesenseCollection>>,
    /// Whether each Typesense endpoint was fetched and parsed successfully.
    pub endpoints: Vec<(&'static str, bool)>,
    /// Wall time of the whole scrape of Typesense, in seconds.
    pub scrape_duration: Option<f64>,
}

/// Render a scrape in the Prometheus text exposition format.
pub fn generate_metrics(scrape: &Scrape, host: &str, port: u16) -> String {
    let registry = Registry::new();
    let port = port.to_string();

    if let Some(metrics) = &scrape.metrics {
        let ts_metrics = gauge(
            &registry,
            "typesense_metrics",
            "Data received through the metrics api of typesense",
            BASE_LABELS,
        );
        // Values Typesense reports but that aren't numbers are left out, not
        // exported as 0.
        for (key, raw) in metrics.entries() {
            if let Ok(value) = raw.parse::<f64>() {
                ts_metrics.with_label_values(&[host, &port, key]).set(value);
            }
        }
    }

    if let Some(stats) = &scrape.stats {
        export_stats(&registry, stats, host, &port);
    }

    gauge(
        &registry,
        "typesense_health",
        "Health status of Typesense instance (1 = healthy, 0 = unhealthy)",
        TARGET_LABELS,
    )
    .with_label_values(&[host, &port])
    .set(if scrape.healthy { 1.0 } else { 0.0 });

    let raft_state = match scrape.raft_state {
        state @ (1 | 4) => f64::from(state),
        _ => 0.0,
    };
    gauge(
        &registry,
        "typesense_raft_state",
        "Raft state of Typesense node (1 = leader, 4 = follower, 0 = other)",
        TARGET_LABELS,
    )
    .with_label_values(&[host, &port])
    .set(raft_state);

    if let Some(version) = &scrape.version {
        gauge(
            &registry,
            "typesense_info",
            "Typesense build information; always 1",
            &["host", "port", "version"],
        )
        .with_label_values(&[host, &port, version])
        .set(1.0);
    }

    if let Some(status) = &scrape.status {
        let target = [host, port.as_str()];
        for (name, help, value) in [
            (
                "typesense_raft_committed_index",
                "Last raft log index committed on this node; compare across nodes for replication lag",
                status.committed_index,
            ),
            (
                "typesense_raft_queued_writes",
                "Writes queued on this node waiting to be applied",
                status.queued_writes,
            ),
        ] {
            if let Some(value) = value {
                gauge(&registry, name, help, TARGET_LABELS)
                    .with_label_values(&target)
                    .set(value);
            }
        }
    }

    if let Some(collections) = &scrape.collections {
        let documents = gauge(
            &registry,
            "typesense_collection_documents",
            "Number of documents in each collection",
            &["host", "port", "collection"],
        );
        for c in collections {
            if let Some(n) = c.num_documents {
                documents.with_label_values(&[host, &port, &c.name]).set(n);
            }
        }
    }

    if !scrape.endpoints.is_empty() {
        let success = gauge(
            &registry,
            "typesense_scrape_success",
            "1 if the Typesense endpoint was fetched and parsed on this scrape, otherwise 0",
            &["host", "port", "endpoint"],
        );
        for (endpoint, ok) in &scrape.endpoints {
            success
                .with_label_values(&[host, &port, endpoint])
                .set(if *ok { 1.0 } else { 0.0 });
        }
    }
    if let Some(seconds) = scrape.scrape_duration {
        gauge(
            &registry,
            "typesense_scrape_duration_seconds",
            "Time taken to query all Typesense endpoints for this scrape",
            TARGET_LABELS,
        )
        .with_label_values(&[host, &port])
        .set(seconds);
    }

    TextEncoder::new()
        .encode_to_string(&registry.gather())
        .expect("prometheus text encoding failed")
}

fn export_stats(registry: &Registry, stats: &TypesenseStats, host: &str, port: &str) {
    let ts_stats = gauge(
        registry,
        "typesense_stats",
        "Data received through the stats api of typesense",
        BASE_LABELS,
    );
    for (key, value) in stats.entries() {
        ts_stats.with_label_values(&[host, port, key]).set(value);
    }

    // Typesense reports one entry per request path, including document and
    // other resource IDs. Normalise those to `:id` so the number of series
    // stays bounded, merging entries that collapse to the same endpoint.
    let endpoints = merge_endpoints(&stats.latency_ms, &stats.requests_per_second);

    let latency = gauge(
        registry,
        "typesense_stats_latency_ms",
        "Each endpoint latency in ms from stats api",
        BASE_LABELS,
    );
    let rps = gauge(
        registry,
        "typesense_stats_requests_per_second",
        "Each endpoint rps from stats api",
        BASE_LABELS,
    );
    let latency_by_collection = gauge(
        registry,
        "typesense_stats_latency_ms_by_collection",
        "Each endpoint latency in ms from stats api",
        COLLECTION_LABELS,
    );
    let rps_by_collection = gauge(
        registry,
        "typesense_stats_requests_per_second_by_collection",
        "Each endpoint rps from stats api",
        COLLECTION_LABELS,
    );

    for (endpoint, e) in &endpoints {
        let parts = parse_collection_action(endpoint);
        for (value, by_endpoint, by_collection) in [
            (e.latency_ms(), &latency, &latency_by_collection),
            (e.rps, &rps, &rps_by_collection),
        ] {
            let Some(value) = value else { continue };
            by_endpoint
                .with_label_values(&[host, port, endpoint])
                .set(value);
            if let Some((method, collection, action)) = parts {
                by_collection
                    .with_label_values(&[host, port, method, collection, action])
                    .set(value);
            }
        }
    }
}

/// Latency and request rate of one normalised endpoint, possibly merged from
/// several raw Typesense entries.
#[derive(Debug, Default, PartialEq)]
struct Endpoint {
    rps: Option<f64>,
    latencies: Vec<(f64, f64)>, // (latency_ms, rps of that raw entry)
}

impl Endpoint {
    /// Request-weighted average latency; a plain average when no entry has
    /// traffic.
    fn latency_ms(&self) -> Option<f64> {
        if self.latencies.is_empty() {
            return None;
        }
        let weight: f64 = self.latencies.iter().map(|(_, w)| w).sum();
        Some(if weight > 0.0 {
            self.latencies.iter().map(|(l, w)| l * w).sum::<f64>() / weight
        } else {
            self.latencies.iter().map(|(l, _)| l).sum::<f64>() / self.latencies.len() as f64
        })
    }
}

fn merge_endpoints(
    latency_ms: &Map<String, Value>,
    requests_per_second: &Map<String, Value>,
) -> BTreeMap<String, Endpoint> {
    let mut merged: BTreeMap<String, Endpoint> = BTreeMap::new();
    for (raw, value) in requests_per_second {
        if let Some(rps) = value.as_f64() {
            let e = merged.entry(normalize_endpoint(raw)).or_default();
            e.rps = Some(e.rps.unwrap_or(0.0) + rps);
        }
    }
    for (raw, value) in latency_ms {
        if let Some(latency) = value.as_f64() {
            let weight = requests_per_second
                .get(raw)
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            merged
                .entry(normalize_endpoint(raw))
                .or_default()
                .latencies
                .push((latency, weight));
        }
    }
    merged
}

/// Path segments followed by a caller-chosen ID or name in Typesense's API.
const ID_PARENTS: &[&str] = &[
    "documents",
    "synonyms",
    "overrides",
    "keys",
    "aliases",
    "presets",
    "stopwords",
    "rules",
    "models",
    "nl_search_models",
    "dictionaries",
    "synonym_sets",
    "curation_sets",
    "items",
];
/// Fixed actions that look like IDs but aren't.
const DOCUMENT_ACTIONS: &[&str] = &["search", "import", "export"];

/// Replace resource IDs in a Typesense stats key with `:id`, e.g.
/// `GET /collections/books/documents/42` → `GET /collections/books/documents/:id`.
/// Collection names are kept.
fn normalize_endpoint(key: &str) -> String {
    let (method, path) = key.split_once(' ').unwrap_or(("", key));
    let path = path.split('?').next().unwrap_or(path);
    let mut segments: Vec<&str> = path.split('/').collect();
    for i in 1..segments.len() {
        let parent = segments[i - 1];
        let is_action = parent == "documents" && DOCUMENT_ACTIONS.contains(&segments[i]);
        if ID_PARENTS.contains(&parent) && !segments[i].is_empty() && !is_action {
            segments[i] = ":id";
        }
    }
    let path = segments.join("/");
    if method.is_empty() {
        path
    } else {
        format!("{method} {path}")
    }
}

fn gauge(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> GaugeVec {
    let gauge = GaugeVec::new(Opts::new(name, help), labels).expect("invalid metric definition");
    registry
        .register(Box::new(gauge.clone()))
        .expect("duplicate metric registration");
    gauge
}

/// Split `"GET /collections/books/documents/search"` into
/// `("GET", "books", "documents/search")`. Anything below the collection
/// counts as the action, e.g. `documents/:id` or `documents`.
fn parse_collection_action(input: &str) -> Option<(&str, &str, &str)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"^(\w+)\s+/collections/([^/]+)/(.+)$").expect("valid regex"));

    let caps = re.captures(input)?;
    Some((
        caps.get(1)?.as_str(),
        caps.get(2)?.as_str(),
        caps.get(3)?.as_str(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_collection_endpoint() {
        assert_eq!(
            parse_collection_action("GET /collections/books/documents/search"),
            Some(("GET", "books", "documents/search"))
        );
        assert_eq!(parse_collection_action("GET /health"), None);
    }

    #[test]
    fn exports_all_metric_families() {
        let stats: TypesenseStats = serde_json::from_value(serde_json::json!({
            "cache_hit_ratio": 0.75, "delete_latency_ms": 1.5, "delete_requests_per_second": 0, "import_latency_ms": 0,
            "import_requests_per_second": 0, "overloaded_requests_per_second": 0,
            "pending_write_batches": 0, "search_latency_ms": 0, "search_requests_per_second": 0,
            "total_requests_per_second": 0, "write_latency_ms": 0, "write_requests_per_second": 0,
            "latency_ms": { "GET /collections/books/documents/search": 9.5 },
            "requests_per_second": { "GET /health": 0.4 }
        }))
        .unwrap();
        let metrics: TypesenseMetrics = serde_json::from_str(
            r#"{"system_cpu1_active_percentage": "12.5", "system_cpu16_active_percentage": "3",
                "system_cpuX_active_percentage": "9", "system_memory_used_swap_bytes": "42",
                "some_future_field": "1"}"#,
        )
        .unwrap();

        let scrape = Scrape {
            metrics: Some(metrics),
            stats: Some(stats),
            healthy: true,
            raft_state: 4,
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "ts.local", 8108);

        assert!(out.contains(
            r#"typesense_metrics{host="ts.local",key="system_cpu1_active_percentage",port="8108"} 12.5"#
        ));
        assert!(out.contains(
            r#"typesense_metrics{host="ts.local",key="system_cpu16_active_percentage",port="8108"} 3"#
        ));
        assert!(
            !out.contains("system_cpu4_active_percentage"),
            "absent cores are not exported"
        );
        assert!(
            !out.contains("system_memory_total_swap_bytes"),
            "metrics Typesense did not report are not exported"
        );
        assert!(
            !out.contains("system_cpuX_"),
            "only numbered cores are exported"
        );
        assert!(!out.contains("some_future_field"));
        assert!(out.contains(
            r#"typesense_metrics{host="ts.local",key="system_memory_used_swap_bytes",port="8108"} 42"#
        ));
        assert!(out.contains(
            r#"typesense_stats{host="ts.local",key="cache_hit_ratio",port="8108"} 0.75"#
        ));
        assert!(out.contains(
            r#"typesense_stats{host="ts.local",key="delete_latency_ms",port="8108"} 1.5"#
        ));
        assert!(out.contains(r#"collection_name="books""#));
        assert!(out.contains(r#"typesense_stats_requests_per_second{host="ts.local",key="GET /health",port="8108"} 0.4"#));
        assert!(out.contains(r#"typesense_health{host="ts.local",port="8108"} 1"#));
        assert!(out.contains(r#"typesense_raft_state{host="ts.local",port="8108"} 4"#));
    }

    #[test]
    fn failed_endpoints_are_omitted_not_zeroed() {
        let out = generate_metrics(&Scrape::default(), "ts.local", 8108);

        assert!(!out.contains("typesense_metrics{"));
        assert!(!out.contains("typesense_stats{"));
        assert!(out.contains(r#"typesense_health{host="ts.local",port="8108"} 0"#));
        assert!(out.contains(r#"typesense_raft_state{host="ts.local",port="8108"} 0"#));
    }

    #[test]
    fn unknown_raft_states_are_reported_as_zero() {
        for (state, expected) in [(1, "1"), (4, "4"), (2, "0"), (-1, "0")] {
            let scrape = Scrape {
                raft_state: state,
                ..Scrape::default()
            };
            let out = generate_metrics(&scrape, "h", 1);
            assert!(
                out.contains(&format!(
                    r#"typesense_raft_state{{host="h",port="1"}} {expected}"#
                )),
                "state {state}"
            );
        }
    }

    #[test]
    fn unparseable_values_are_omitted() {
        let metrics: TypesenseMetrics = serde_json::from_str(
            r#"{"system_disk_used_bytes": "n/a", "system_disk_total_bytes": "100"}"#,
        )
        .unwrap();
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"latency_ms": {"POST /collections/a/documents/import": "slow",
                               "GET /collections/a/documents/search": 3}}"#,
        )
        .unwrap();
        let scrape = Scrape {
            metrics: Some(metrics),
            stats: Some(stats),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(
            !out.contains("system_disk_used_bytes"),
            "\"n/a\" is not a 0"
        );
        assert!(out.contains(r#"key="system_disk_total_bytes",port="1"} 100"#));
        assert!(
            !out.contains("documents/import"),
            "non-numeric latency is left out"
        );
        assert!(out.contains(r#"action="documents/search""#));
    }

    #[test]
    fn non_collection_paths_skip_by_collection_metrics() {
        for path in ["GET /health", "GET /collections", "GET /collections/books"] {
            assert_eq!(parse_collection_action(path), None, "{path}");
        }
        assert_eq!(
            parse_collection_action("DELETE /collections/books/documents/:id"),
            Some(("DELETE", "books", "documents/:id"))
        );
        assert_eq!(
            parse_collection_action("POST /collections/books/documents"),
            Some(("POST", "books", "documents"))
        );
    }

    #[test]
    fn output_has_help_and_type_lines() {
        let out = generate_metrics(&Scrape::default(), "h", 1);

        assert!(out.contains("# HELP typesense_health "));
        assert!(out.contains("# TYPE typesense_health gauge"));
        assert!(out.contains("# TYPE typesense_raft_state gauge"));
    }

    #[test]
    fn exports_version_replication_and_collection_documents() {
        let scrape = Scrape {
            version: Some("30.1".into()),
            status: Some(
                serde_json::from_str(r#"{"committed_index": 18, "queued_writes": 2}"#).unwrap(),
            ),
            collections: Some(
                serde_json::from_str(
                    r#"[{"name": "products", "num_documents": 42}, {"name": "unknown"}]"#,
                )
                .unwrap(),
            ),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(out.contains(r#"typesense_info{host="h",port="1",version="30.1"} 1"#));
        assert!(out.contains(r#"typesense_raft_committed_index{host="h",port="1"} 18"#));
        assert!(out.contains(r#"typesense_raft_queued_writes{host="h",port="1"} 2"#));
        assert!(out.contains(
            r#"typesense_collection_documents{collection="products",host="h",port="1"} 42"#
        ));
        assert!(
            !out.contains(r#"collection="unknown""#),
            "collections without a count are skipped"
        );
    }

    #[test]
    fn new_families_are_omitted_when_their_endpoint_failed() {
        let out = generate_metrics(&Scrape::default(), "h", 1);

        for family in [
            "typesense_info",
            "typesense_raft_committed_index",
            "typesense_raft_queued_writes",
            "typesense_collection_documents",
        ] {
            assert!(
                !out.contains(&format!("{family}{{")),
                "{family} should be absent"
            );
        }
    }

    #[test]
    fn status_fields_missing_from_older_typesense_are_omitted() {
        let scrape = Scrape {
            status: Some(serde_json::from_str(r#"{"state": "LEADER"}"#).unwrap()),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(!out.contains("typesense_raft_committed_index{"));
        assert!(!out.contains("typesense_raft_queued_writes{"));
    }

    #[test]
    fn exports_latency_percentiles_and_cache_counts() {
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"search_95Percentile_latency_ms": 4.5, "search_99Percentile_latency_ms": 7,
                "write_max_latency_ms": 12, "cache_hit_count": 9, "cache_miss_count": 3}"#,
        )
        .unwrap();
        let scrape = Scrape {
            stats: Some(stats),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        for line in [
            r#"typesense_stats{host="h",key="search_95Percentile_latency_ms",port="1"} 4.5"#,
            r#"typesense_stats{host="h",key="search_99Percentile_latency_ms",port="1"} 7"#,
            r#"typesense_stats{host="h",key="write_max_latency_ms",port="1"} 12"#,
            r#"typesense_stats{host="h",key="cache_hit_count",port="1"} 9"#,
            r#"typesense_stats{host="h",key="cache_miss_count",port="1"} 3"#,
        ] {
            assert!(out.contains(line), "missing {line}");
        }
    }

    #[test]
    fn normalizes_resource_ids_but_keeps_collections_and_actions() {
        for (raw, normalized) in [
            (
                "GET /collections/books/documents/42",
                "GET /collections/books/documents/:id",
            ),
            (
                "DELETE /collections/books/documents/sku-9?x=1",
                "DELETE /collections/books/documents/:id",
            ),
            (
                "GET /collections/books/documents/search",
                "GET /collections/books/documents/search",
            ),
            (
                "POST /collections/books/documents/import",
                "POST /collections/books/documents/import",
            ),
            (
                "GET /collections/books/documents/export",
                "GET /collections/books/documents/export",
            ),
            (
                "POST /collections/books/documents",
                "POST /collections/books/documents",
            ),
            (
                "PUT /collections/books/synonyms/colours",
                "PUT /collections/books/synonyms/:id",
            ),
            ("DELETE /keys/17", "DELETE /keys/:id"),
            ("PUT /aliases/live", "PUT /aliases/:id"),
            (
                "GET /synonym_sets/s1/items/i2",
                "GET /synonym_sets/:id/items/:id",
            ),
            ("GET /health", "GET /health"),
            ("POST /multi_search", "POST /multi_search"),
        ] {
            assert_eq!(normalize_endpoint(raw), normalized, "{raw}");
        }
    }

    #[test]
    fn merged_endpoints_sum_rates_and_weight_latency() {
        let latency = serde_json::from_str(
            r#"{"GET /collections/a/documents/1": 10, "GET /collections/a/documents/2": 40,
                "GET /collections/a/documents/3": 7, "GET /health": 1}"#,
        )
        .unwrap();
        let rps = serde_json::from_str(
            r#"{"GET /collections/a/documents/1": 3, "GET /collections/a/documents/2": 1,
                "GET /health": 0}"#,
        )
        .unwrap();
        let merged = merge_endpoints(&latency, &rps);

        let doc = &merged["GET /collections/a/documents/:id"];
        assert_eq!(doc.rps, Some(4.0));
        // (10*3 + 40*1 + 7*0) / (3 + 1 + 0)
        assert_eq!(doc.latency_ms(), Some(17.5));
        assert_eq!(
            merged["GET /health"].latency_ms(),
            Some(1.0),
            "no traffic: plain average"
        );
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn exports_scrape_health() {
        let scrape = Scrape {
            endpoints: vec![("/metrics.json", true), ("/status", false)],
            scrape_duration: Some(0.25),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(out
            .contains(r#"typesense_scrape_success{endpoint="/metrics.json",host="h",port="1"} 1"#));
        assert!(out.contains(r#"typesense_scrape_success{endpoint="/status",host="h",port="1"} 0"#));
        assert!(out.contains(r#"typesense_scrape_duration_seconds{host="h",port="1"} 0.25"#));
    }
}
