use std::sync::OnceLock;

use prometheus::{GaugeVec, Opts, Registry, TextEncoder};
use regex::Regex;
use serde_json::{Map, Value};

use crate::typesense::models::{
    typesense_collection_model::TypesenseCollection, typesense_metrics_model::TypesenseMetrics,
    typesense_stats_model::TypesenseStats, typesense_status_model::TypesenseStatus,
};

const BASE_LABELS: &[&str] = &["host", "port", "key"];
const COLLECTION_LABELS: &[&str] = &["host", "port", "key", "method", "collection_name", "action"];
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
        for (key, raw) in metrics.entries() {
            ts_metrics
                .with_label_values(&[host, &port, key])
                .set(raw.parse().unwrap_or(0.0));
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

    let per_endpoint = [
        (
            "typesense_stats_latency_ms",
            "Each endpoint latency in ms from stats api",
            &stats.latency_ms,
        ),
        (
            "typesense_stats_requests_per_second",
            "Each endpoint rps from stats api",
            &stats.requests_per_second,
        ),
    ];
    for (name, help, values) in per_endpoint {
        let by_endpoint = gauge(registry, name, help, BASE_LABELS);
        let by_collection = gauge(
            registry,
            &format!("{name}_by_collection"),
            help,
            COLLECTION_LABELS,
        );
        export_per_endpoint(values, host, port, &by_endpoint, &by_collection);
    }
}

fn export_per_endpoint(
    values: &Map<String, Value>,
    host: &str,
    port: &str,
    by_endpoint: &GaugeVec,
    by_collection: &GaugeVec,
) {
    for (key, value) in values {
        let value = value.as_f64().unwrap_or(0.0);
        by_endpoint.with_label_values(&[host, port, key]).set(value);

        if let Some((method, collection, action)) = parse_collection_action(key) {
            by_collection
                .with_label_values(&[host, port, key, method, collection, action])
                .set(value);
        }
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
/// `("GET", "books", "documents/search")`.
fn parse_collection_action(input: &str) -> Option<(&str, &str, &str)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(\w+)\s+/collections/([^/]+)/([^/]+/[^/]+)").expect("valid regex")
    });

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
    fn unparseable_values_are_exported_as_zero() {
        let metrics: TypesenseMetrics =
            serde_json::from_str(r#"{"system_disk_used_bytes": "n/a"}"#).unwrap();
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"latency_ms": {"POST /collections/a/documents/import": "slow"}}"#,
        )
        .unwrap();
        let scrape = Scrape {
            metrics: Some(metrics),
            stats: Some(stats),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(
            out.contains(r#"typesense_metrics{host="h",key="system_disk_used_bytes",port="1"} 0"#)
        );
        assert!(out.contains(r#"action="documents/import",collection_name="a""#));
    }

    #[test]
    fn non_collection_paths_skip_by_collection_metrics() {
        for path in ["GET /health", "GET /collections", "GET /collections/books"] {
            assert_eq!(parse_collection_action(path), None, "{path}");
        }
        assert_eq!(
            parse_collection_action("DELETE /collections/books/documents/42"),
            Some(("DELETE", "books", "documents/42"))
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
}
