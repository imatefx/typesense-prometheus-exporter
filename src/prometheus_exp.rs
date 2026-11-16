use std::{collections::BTreeMap, sync::OnceLock};

use prometheus::{CounterVec, GaugeVec, Opts, Registry, TextEncoder};
use regex::Regex;
use serde_json::{Map, Value};

use crate::{
    cli::MetricNames,
    typesense::models::{
        typesense_collection_model::TypesenseCollection,
        typesense_metrics_model::{core_number, TypesenseMetrics},
        typesense_stats_model::TypesenseStats,
        typesense_status_model::TypesenseStatus,
    },
};

const BASE_LABELS: &[&str] = &["host", "port", "key"];
const COLLECTION_LABELS: &[&str] = &["host", "port", "method", "collection_name", "action"];
const TARGET_LABELS: &[&str] = &["host", "port"];

const OPERATION_LABELS: &[&str] = &["host", "port", "operation"];
const QUANTILE_LABELS: &[&str] = &["host", "port", "operation", "quantile"];
const ROUTE_LABELS: &[&str] = &["host", "port", "route"];
const V1_COLLECTION_LABELS: &[&str] = &["host", "port", "method", "collection", "action"];
const UNMAPPED_LABELS: &[&str] = &["host", "port", "source", "key"];

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
    /// Which names the `/metrics.json` and `/stats.json` values are exported
    /// under. Families outside those two endpoints are the same in every mode.
    pub metric_names: MetricNames,
}

/// Render a scrape in the Prometheus text exposition format.
pub fn generate_metrics(scrape: &Scrape, host: &str, port: u16) -> String {
    let registry = Registry::new();
    let port = port.to_string();
    let names = scrape.metric_names;

    // Shared by both endpoints. Families without series are left out of the
    // output, so this only shows up when a field actually is unmapped.
    let unmapped = names.v1().then(|| {
        gauge(
            &registry,
            "typesense_unmapped",
            "Numeric Typesense field this exporter has no name for yet, raw value without unit conversion; don't build dashboards on it",
            UNMAPPED_LABELS,
        )
    });

    if let Some(metrics) = &scrape.metrics {
        if names.legacy() {
            export_legacy_metrics(&registry, metrics, host, &port);
        }
        if let Some(unmapped) = &unmapped {
            export_v1_metrics(&registry, metrics, unmapped, host, &port);
        }
    }

    if let Some(stats) = &scrape.stats {
        // Typesense reports one entry per request path, including document
        // and other resource IDs. Normalise those to `:id` so the number of
        // series stays bounded, merging entries that collapse to the same
        // route.
        let routes = merge_endpoints(&stats.latency_ms, &stats.requests_per_second);
        if names.legacy() {
            export_legacy_stats(&registry, stats, &routes, host, &port);
        }
        if let Some(unmapped) = &unmapped {
            export_v1_stats(&registry, stats, &routes, unmapped, host, &port);
        }
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

/// 0.4 names for `/metrics.json`: one `typesense_metrics{key}` family.
fn export_legacy_metrics(registry: &Registry, metrics: &TypesenseMetrics, host: &str, port: &str) {
    let ts_metrics = gauge(
        registry,
        "typesense_metrics",
        "Data received through the metrics api of typesense",
        BASE_LABELS,
    );
    // Values Typesense reports but that aren't numbers are left out, not
    // exported as 0.
    for (key, raw) in metrics.entries() {
        if let Ok(value) = raw.parse::<f64>() {
            ts_metrics.with_label_values(&[host, port, key]).set(value);
        }
    }
}

/// How a `/metrics.json` field is exported under its 1.0 name.
enum Kind {
    /// A gauge holding the raw value divided by this number.
    Gauge(f64),
    /// A counter holding the raw value.
    Counter,
}

/// `/metrics.json` field → (1.0 family, help, kind). Percentages are divided
/// by 100 into ratios; everything else is already in base units.
const METRICS_V1: &[(&str, &str, &str, Kind)] = &[
    (
        "system_cpu_active_percentage",
        "typesense_system_cpu_active_ratio",
        "Share of time all CPUs of the machine were busy (0-1)",
        Kind::Gauge(100.0),
    ),
    (
        "system_memory_total_bytes",
        "typesense_system_memory_total_bytes",
        "Total memory of the machine in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_memory_used_bytes",
        "typesense_system_memory_used_bytes",
        "Memory in use on the machine in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_memory_total_swap_bytes",
        "typesense_system_swap_total_bytes",
        "Total swap space of the machine in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_memory_used_swap_bytes",
        "typesense_system_swap_used_bytes",
        "Swap space in use on the machine in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_disk_total_bytes",
        "typesense_system_disk_total_bytes",
        "Total size of the disk holding the Typesense data directory in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_disk_used_bytes",
        "typesense_system_disk_used_bytes",
        "Used space on the disk holding the Typesense data directory in bytes",
        Kind::Gauge(1.0),
    ),
    (
        "system_network_received_bytes",
        "typesense_system_network_received_bytes_total",
        "Bytes received over the network by the machine",
        Kind::Counter,
    ),
    (
        "system_network_sent_bytes",
        "typesense_system_network_sent_bytes_total",
        "Bytes sent over the network by the machine",
        Kind::Counter,
    ),
    (
        "typesense_memory_active_bytes",
        "typesense_process_memory_active_bytes",
        "Bytes in active pages allocated by Typesense (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_allocated_bytes",
        "typesense_process_memory_allocated_bytes",
        "Bytes allocated by Typesense (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_mapped_bytes",
        "typesense_process_memory_mapped_bytes",
        "Bytes in chunks mapped by the Typesense allocator (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_metadata_bytes",
        "typesense_process_memory_metadata_bytes",
        "Bytes of allocator metadata in Typesense (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_resident_bytes",
        "typesense_process_memory_resident_bytes",
        "Bytes in physically resident pages mapped by the Typesense allocator (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_retained_bytes",
        "typesense_process_memory_retained_bytes",
        "Bytes retained by the Typesense allocator instead of returned to the OS (jemalloc)",
        Kind::Gauge(1.0),
    ),
    (
        "typesense_memory_fragmentation_ratio",
        "typesense_process_memory_fragmentation_ratio",
        "Memory fragmentation of the Typesense allocator, as Typesense reports it",
        Kind::Gauge(1.0),
    ),
];

/// 1.0 names for `/metrics.json`.
fn export_v1_metrics(
    registry: &Registry,
    metrics: &TypesenseMetrics,
    unmapped: &GaugeVec,
    host: &str,
    port: &str,
) {
    let target = [host, port];
    for (key, raw) in metrics.fixed_entries() {
        let Ok(value) = raw.parse::<f64>() else {
            continue;
        };
        match METRICS_V1.iter().find(|(field, ..)| *field == key) {
            Some((_, name, help, Kind::Gauge(divisor))) => {
                gauge(registry, name, help, TARGET_LABELS)
                    .with_label_values(&target)
                    .set(value / divisor);
            }
            Some((_, name, help, Kind::Counter)) => {
                // The registry is rebuilt on every scrape, so a fresh counter
                // incremented by Typesense's running total exports that total.
                // A counter can't go down; a negative value is left out.
                if value >= 0.0 {
                    counter(registry, name, help, TARGET_LABELS)
                        .with_label_values(&target)
                        .inc_by(value);
                }
            }
            None => unmapped
                .with_label_values(&[host, port, "metrics.json", key])
                .set(value),
        }
    }

    let cores = gauge(
        registry,
        "typesense_system_cpu_core_active_ratio",
        "Share of time each CPU core of the machine was busy (0-1)",
        &["host", "port", "core"],
    );
    for (key, raw) in &metrics.other {
        let Some(value) = number(raw) else { continue };
        match core_number(key) {
            Some(core) => cores
                .with_label_values(&[host, port, core])
                .set(value / 100.0),
            None => unmapped
                .with_label_values(&[host, port, "metrics.json", key])
                .set(value),
        }
    }
}

/// A JSON number, or a string holding one (Typesense reports `/metrics.json`
/// values as strings).
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::String(s) => s.parse().ok(),
        other => other.as_f64(),
    }
}

/// 0.4 names for `/stats.json`.
fn export_legacy_stats(
    registry: &Registry,
    stats: &TypesenseStats,
    routes: &BTreeMap<String, Endpoint>,
    host: &str,
    port: &str,
) {
    let ts_stats = gauge(
        registry,
        "typesense_stats",
        "Data received through the stats api of typesense",
        BASE_LABELS,
    );
    for (key, value) in stats.entries() {
        ts_stats.with_label_values(&[host, port, key]).set(value);
    }

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

    for (endpoint, e) in routes {
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

/// Operations Typesense reports rates and latencies for.
const OPERATIONS: &[&str] = &["search", "import", "write", "delete"];

/// `<op>_…` latency field suffix → `quantile` label.
const QUANTILES: &[(&str, &str)] = &[
    ("min_latency_ms", "0"),
    ("70Percentile_latency_ms", "0.7"),
    ("95Percentile_latency_ms", "0.95"),
    ("99Percentile_latency_ms", "0.99"),
    ("max_latency_ms", "1"),
];

/// Top-level `/stats.json` field without an operation → (1.0 family, help).
/// These need no unit conversion.
const STATS_V1: &[(&str, &str, &str)] = &[
    (
        "total_requests_per_second",
        "typesense_all_requests_per_second",
        "Requests per second over all routes, including ones outside the operation breakdown such as health checks",
    ),
    (
        "overloaded_requests_per_second",
        "typesense_overloaded_requests_per_second",
        "Requests per second rejected because Typesense was overloaded",
    ),
    (
        "pending_write_batches",
        "typesense_pending_write_batches",
        "Write batches waiting to be applied",
    ),
    (
        "cache_hit_count",
        "typesense_cache_hits",
        "Search cache hits in Typesense's rolling stats window",
    ),
    (
        "cache_miss_count",
        "typesense_cache_misses",
        "Search cache misses in Typesense's rolling stats window",
    ),
    (
        "cache_hit_ratio",
        "typesense_cache_hit_ratio",
        "Share of searches answered from the cache (0-1)",
    ),
];

/// 1.0 names for `/stats.json`.
fn export_v1_stats(
    registry: &Registry,
    stats: &TypesenseStats,
    routes: &BTreeMap<String, Endpoint>,
    unmapped: &GaugeVec,
    host: &str,
    port: &str,
) {
    let rps = gauge(
        registry,
        "typesense_requests_per_second",
        "Requests per second for each operation",
        OPERATION_LABELS,
    );
    let latency_avg = gauge(
        registry,
        "typesense_request_latency_avg_seconds",
        "Average request latency for each operation in seconds",
        OPERATION_LABELS,
    );
    let latency = gauge(
        registry,
        "typesense_request_latency_seconds",
        "Request latency quantiles for each operation in seconds; quantile 0 is the minimum and 1 the maximum",
        QUANTILE_LABELS,
    );

    for (key, value) in stats.entries() {
        if let Some((_, name, help)) = STATS_V1.iter().find(|(field, ..)| *field == key) {
            gauge(registry, name, help, TARGET_LABELS)
                .with_label_values(&[host, port])
                .set(value);
            continue;
        }
        let Some((operation, field)) = OPERATIONS.iter().find_map(|op| {
            let field = key.strip_prefix(op)?.strip_prefix('_')?;
            Some((*op, field))
        }) else {
            unmapped
                .with_label_values(&[host, port, "stats.json", key])
                .set(value);
            continue;
        };
        if field == "requests_per_second" {
            rps.with_label_values(&[host, port, operation]).set(value);
        } else if field == "latency_ms" {
            latency_avg
                .with_label_values(&[host, port, operation])
                .set(value / 1000.0);
        } else if let Some((_, quantile)) = QUANTILES.iter().find(|(suffix, _)| *suffix == field) {
            latency
                .with_label_values(&[host, port, operation, quantile])
                .set(value / 1000.0);
        } else {
            unmapped
                .with_label_values(&[host, port, "stats.json", key])
                .set(value);
        }
    }

    let route_latency = gauge(
        registry,
        "typesense_route_latency_seconds",
        "Average latency of each API route in seconds",
        ROUTE_LABELS,
    );
    let route_rps = gauge(
        registry,
        "typesense_route_requests_per_second",
        "Requests per second for each API route",
        ROUTE_LABELS,
    );
    let collection_latency = gauge(
        registry,
        "typesense_collection_latency_seconds",
        "Average latency of each collection route in seconds",
        V1_COLLECTION_LABELS,
    );
    let collection_rps = gauge(
        registry,
        "typesense_collection_requests_per_second",
        "Requests per second for each collection route",
        V1_COLLECTION_LABELS,
    );

    for (route, e) in routes {
        let parts = parse_collection_action(route);
        for (value, by_route, by_collection) in [
            (
                e.latency_ms().map(|ms| ms / 1000.0),
                &route_latency,
                &collection_latency,
            ),
            (e.rps, &route_rps, &collection_rps),
        ] {
            let Some(value) = value else { continue };
            by_route.with_label_values(&[host, port, route]).set(value);
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

fn counter(registry: &Registry, name: &str, help: &str, labels: &[&str]) -> CounterVec {
    let counter =
        CounterVec::new(Opts::new(name, help), labels).expect("invalid metric definition");
    registry
        .register(Box::new(counter.clone()))
        .expect("duplicate metric registration");
    counter
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
            metric_names: MetricNames::Legacy,
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
        let scrape = Scrape {
            metric_names: MetricNames::Both,
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "ts.local", 8108);

        for family in LEGACY_FAMILIES.iter().chain(V1_FAMILIES) {
            assert!(!out.contains(&format!("{family}{{")), "{family}");
        }
        assert!(!out.contains("typesense_unmapped"));
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
            metric_names: MetricNames::Legacy,
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
            metric_names: MetricNames::Legacy,
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

    // -----------------------------------------------------------------------
    // 1.0 names
    // -----------------------------------------------------------------------

    const LEGACY_FAMILIES: &[&str] = &[
        "typesense_metrics",
        "typesense_stats",
        "typesense_stats_latency_ms",
        "typesense_stats_requests_per_second",
        "typesense_stats_latency_ms_by_collection",
        "typesense_stats_requests_per_second_by_collection",
    ];

    const V1_FAMILIES: &[&str] = &[
        "typesense_requests_per_second",
        "typesense_all_requests_per_second",
        "typesense_overloaded_requests_per_second",
        "typesense_request_latency_avg_seconds",
        "typesense_request_latency_seconds",
        "typesense_pending_write_batches",
        "typesense_cache_hits",
        "typesense_cache_misses",
        "typesense_cache_hit_ratio",
        "typesense_route_latency_seconds",
        "typesense_route_requests_per_second",
        "typesense_collection_latency_seconds",
        "typesense_collection_requests_per_second",
        "typesense_system_cpu_active_ratio",
        "typesense_system_cpu_core_active_ratio",
        "typesense_system_memory_total_bytes",
        "typesense_system_memory_used_bytes",
        "typesense_system_swap_total_bytes",
        "typesense_system_swap_used_bytes",
        "typesense_system_disk_total_bytes",
        "typesense_system_disk_used_bytes",
        "typesense_system_network_received_bytes_total",
        "typesense_system_network_sent_bytes_total",
        "typesense_process_memory_active_bytes",
        "typesense_process_memory_allocated_bytes",
        "typesense_process_memory_mapped_bytes",
        "typesense_process_memory_metadata_bytes",
        "typesense_process_memory_resident_bytes",
        "typesense_process_memory_retained_bytes",
        "typesense_process_memory_fragmentation_ratio",
        "typesense_unmapped",
    ];

    /// Every field of `/metrics.json`, one per-core value and one unknown field.
    const FULL_METRICS: &str = r#"{
        "system_cpu_active_percentage": "12.5", "system_cpu1_active_percentage": "50",
        "system_cpu2_active_percentage": "0",
        "system_memory_total_bytes": "2000", "system_memory_used_bytes": "1000",
        "system_memory_total_swap_bytes": "400", "system_memory_used_swap_bytes": "100",
        "system_disk_total_bytes": "9000", "system_disk_used_bytes": "3000",
        "system_network_received_bytes": "1466", "system_network_sent_bytes": "182",
        "typesense_memory_active_bytes": "11", "typesense_memory_allocated_bytes": "12",
        "typesense_memory_mapped_bytes": "13", "typesense_memory_metadata_bytes": "14",
        "typesense_memory_resident_bytes": "15", "typesense_memory_retained_bytes": "16",
        "typesense_memory_fragmentation_ratio": "0.08",
        "some_future_field": "7"
    }"#;

    /// Every scalar of `/stats.json`, per-route entries and one unknown field.
    const FULL_STATS: &str = r#"{
        "search_requests_per_second": 3.5, "import_requests_per_second": 0.5,
        "write_requests_per_second": 2, "delete_requests_per_second": 0,
        "total_requests_per_second": 6.25, "overloaded_requests_per_second": 0.25,
        "search_latency_ms": 1.5, "import_latency_ms": 250, "write_latency_ms": 4, "delete_latency_ms": 0,
        "search_min_latency_ms": 0.5, "search_70Percentile_latency_ms": 1,
        "search_95Percentile_latency_ms": 3, "search_99Percentile_latency_ms": 4,
        "search_max_latency_ms": 6,
        "pending_write_batches": 2, "cache_hit_count": 5, "cache_miss_count": 15,
        "cache_hit_ratio": 0.25, "some_future_stat": 42,
        "latency_ms": {"GET /collections/products/documents/search": 2.5, "GET /health": 0.5},
        "requests_per_second": {"GET /collections/products/documents/search": 3.4, "GET /health": 0.2}
    }"#;

    fn scrape_with(names: MetricNames) -> Scrape {
        Scrape {
            metrics: Some(serde_json::from_str(FULL_METRICS).unwrap()),
            stats: Some(serde_json::from_str(FULL_STATS).unwrap()),
            healthy: true,
            raft_state: 1,
            version: Some("30.1".into()),
            status: Some(
                serde_json::from_str(r#"{"committed_index": 18, "queued_writes": 2}"#).unwrap(),
            ),
            collections: Some(
                serde_json::from_str(r#"[{"name": "products", "num_documents": 42}]"#).unwrap(),
            ),
            endpoints: vec![("/metrics.json", true)],
            scrape_duration: Some(0.25),
            metric_names: names,
        }
    }

    /// Value of the series written exactly as `series` (name and labels).
    fn value(out: &str, series: &str) -> Option<f64> {
        out.lines()
            .find_map(|l| l.strip_prefix(series)?.strip_prefix(' '))
            .map(|v| v.parse().unwrap())
    }

    fn assert_value(out: &str, series: &str, expected: f64) {
        let got = value(out, series).unwrap_or_else(|| panic!("missing {series}\n{out}"));
        assert!(
            (got - expected).abs() <= expected.abs() * 1e-12,
            "{series}: got {got}, want {expected}"
        );
    }

    fn has_family(out: &str, family: &str) -> bool {
        out.contains(&format!("# TYPE {family} "))
    }

    #[test]
    fn default_mode_is_v1() {
        assert_eq!(Scrape::default().metric_names, MetricNames::V1);
    }

    #[test]
    fn maps_every_metrics_json_field() {
        let out = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);

        for (series, expected) in [
            (
                r#"typesense_system_cpu_active_ratio{host="h",port="1"}"#,
                0.125,
            ),
            (
                r#"typesense_system_cpu_core_active_ratio{core="1",host="h",port="1"}"#,
                0.5,
            ),
            (
                r#"typesense_system_cpu_core_active_ratio{core="2",host="h",port="1"}"#,
                0.0,
            ),
            (
                r#"typesense_system_memory_total_bytes{host="h",port="1"}"#,
                2000.0,
            ),
            (
                r#"typesense_system_memory_used_bytes{host="h",port="1"}"#,
                1000.0,
            ),
            (
                r#"typesense_system_swap_total_bytes{host="h",port="1"}"#,
                400.0,
            ),
            (
                r#"typesense_system_swap_used_bytes{host="h",port="1"}"#,
                100.0,
            ),
            (
                r#"typesense_system_disk_total_bytes{host="h",port="1"}"#,
                9000.0,
            ),
            (
                r#"typesense_system_disk_used_bytes{host="h",port="1"}"#,
                3000.0,
            ),
            (
                r#"typesense_system_network_received_bytes_total{host="h",port="1"}"#,
                1466.0,
            ),
            (
                r#"typesense_system_network_sent_bytes_total{host="h",port="1"}"#,
                182.0,
            ),
            (
                r#"typesense_process_memory_active_bytes{host="h",port="1"}"#,
                11.0,
            ),
            (
                r#"typesense_process_memory_allocated_bytes{host="h",port="1"}"#,
                12.0,
            ),
            (
                r#"typesense_process_memory_mapped_bytes{host="h",port="1"}"#,
                13.0,
            ),
            (
                r#"typesense_process_memory_metadata_bytes{host="h",port="1"}"#,
                14.0,
            ),
            (
                r#"typesense_process_memory_resident_bytes{host="h",port="1"}"#,
                15.0,
            ),
            (
                r#"typesense_process_memory_retained_bytes{host="h",port="1"}"#,
                16.0,
            ),
            (
                r#"typesense_process_memory_fragmentation_ratio{host="h",port="1"}"#,
                0.08,
            ),
        ] {
            assert_value(&out, series, expected);
        }
        // Every fixed field has a 1.0 name, so none lands in the fallback.
        assert_eq!(
            out.lines()
                .filter(|l| l.starts_with("typesense_unmapped{"))
                .count(),
            2,
            "only the two unknown fields are unmapped:\n{out}"
        );
        assert_eq!(METRICS_V1.len(), 16);
    }

    #[test]
    fn maps_every_stats_json_field() {
        let out = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);

        for (series, expected) in [
            (
                r#"typesense_requests_per_second{host="h",operation="search",port="1"}"#,
                3.5,
            ),
            (
                r#"typesense_requests_per_second{host="h",operation="import",port="1"}"#,
                0.5,
            ),
            (
                r#"typesense_requests_per_second{host="h",operation="write",port="1"}"#,
                2.0,
            ),
            (
                r#"typesense_requests_per_second{host="h",operation="delete",port="1"}"#,
                0.0,
            ),
            (
                r#"typesense_all_requests_per_second{host="h",port="1"}"#,
                6.25,
            ),
            (
                r#"typesense_overloaded_requests_per_second{host="h",port="1"}"#,
                0.25,
            ),
            (
                r#"typesense_request_latency_avg_seconds{host="h",operation="search",port="1"}"#,
                0.0015,
            ),
            (
                r#"typesense_request_latency_avg_seconds{host="h",operation="import",port="1"}"#,
                0.25,
            ),
            (
                r#"typesense_request_latency_avg_seconds{host="h",operation="write",port="1"}"#,
                0.004,
            ),
            (
                r#"typesense_request_latency_avg_seconds{host="h",operation="delete",port="1"}"#,
                0.0,
            ),
            (
                r#"typesense_request_latency_seconds{host="h",operation="search",port="1",quantile="0"}"#,
                0.0005,
            ),
            (
                r#"typesense_request_latency_seconds{host="h",operation="search",port="1",quantile="0.7"}"#,
                0.001,
            ),
            (
                r#"typesense_request_latency_seconds{host="h",operation="search",port="1",quantile="0.95"}"#,
                0.003,
            ),
            (
                r#"typesense_request_latency_seconds{host="h",operation="search",port="1",quantile="0.99"}"#,
                0.004,
            ),
            (
                r#"typesense_request_latency_seconds{host="h",operation="search",port="1",quantile="1"}"#,
                0.006,
            ),
            (r#"typesense_pending_write_batches{host="h",port="1"}"#, 2.0),
            (r#"typesense_cache_hits{host="h",port="1"}"#, 5.0),
            (r#"typesense_cache_misses{host="h",port="1"}"#, 15.0),
            (r#"typesense_cache_hit_ratio{host="h",port="1"}"#, 0.25),
        ] {
            assert_value(&out, series, expected);
        }
        assert!(
            !out.contains(r#"operation="total""#),
            "total has its own family"
        );
    }

    #[test]
    fn maps_every_quantile_for_every_operation() {
        let mut fields = serde_json::Map::new();
        for op in OPERATIONS {
            for (i, (suffix, _)) in QUANTILES.iter().enumerate() {
                fields.insert(format!("{op}_{suffix}"), serde_json::json!(i + 1));
            }
        }
        let scrape = Scrape {
            stats: Some(serde_json::from_value(fields.into()).unwrap()),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        for op in OPERATIONS {
            for (i, (_, quantile)) in QUANTILES.iter().enumerate() {
                assert_value(
                    &out,
                    &format!(
                        r#"typesense_request_latency_seconds{{host="h",operation="{op}",port="1",quantile="{quantile}"}}"#
                    ),
                    (i + 1) as f64 / 1000.0,
                );
            }
        }
        assert!(!out.contains("typesense_unmapped{"), "{out}");
    }

    #[test]
    fn maps_routes_and_collections() {
        let out = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);

        for (series, expected) in [
            (
                r#"typesense_route_latency_seconds{host="h",port="1",route="GET /collections/products/documents/search"}"#,
                0.0025,
            ),
            (
                r#"typesense_route_latency_seconds{host="h",port="1",route="GET /health"}"#,
                0.0005,
            ),
            (
                r#"typesense_route_requests_per_second{host="h",port="1",route="GET /collections/products/documents/search"}"#,
                3.4,
            ),
            (
                r#"typesense_route_requests_per_second{host="h",port="1",route="GET /health"}"#,
                0.2,
            ),
            (
                r#"typesense_collection_latency_seconds{action="documents/search",collection="products",host="h",method="GET",port="1"}"#,
                0.0025,
            ),
            (
                r#"typesense_collection_requests_per_second{action="documents/search",collection="products",host="h",method="GET",port="1"}"#,
                3.4,
            ),
        ] {
            assert_value(&out, series, expected);
        }
        assert!(
            !out.contains(r#"collection="health""#),
            "non-collection routes stay out of the collection families"
        );
        assert!(!out.contains("collection_name="));
        assert!(!out.contains("endpoint=\"GET"));
    }

    #[test]
    fn merged_routes_keep_the_normalisation_in_v1() {
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"latency_ms": {"GET /collections/a/documents/1": 10, "GET /collections/a/documents/2": 40},
                "requests_per_second": {"GET /collections/a/documents/1": 3, "GET /collections/a/documents/2": 1}}"#,
        )
        .unwrap();
        let scrape = Scrape {
            stats: Some(stats),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert_value(
            &out,
            r#"typesense_route_latency_seconds{host="h",port="1",route="GET /collections/a/documents/:id"}"#,
            0.0175,
        );
        assert_value(
            &out,
            r#"typesense_route_requests_per_second{host="h",port="1",route="GET /collections/a/documents/:id"}"#,
            4.0,
        );
    }

    #[test]
    fn converts_to_base_units() {
        let out = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);

        // ms -> s
        assert!(out.contains(
            r#"typesense_request_latency_avg_seconds{host="h",operation="search",port="1"} 0.0015"#
        ));
        // percent -> ratio
        assert!(out.contains(r#"typesense_system_cpu_active_ratio{host="h",port="1"} 0.125"#));
        // already a ratio, bytes, and rates are left alone
        assert!(out.contains(r#"typesense_cache_hit_ratio{host="h",port="1"} 0.25"#));
        assert!(
            out.contains(r#"typesense_process_memory_fragmentation_ratio{host="h",port="1"} 0.08"#)
        );
        assert!(out.contains(r#"typesense_system_memory_total_bytes{host="h",port="1"} 2000"#));
    }

    #[test]
    fn network_bytes_are_counters_and_the_rest_gauges() {
        let out = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);

        for family in [
            "typesense_system_network_received_bytes_total",
            "typesense_system_network_sent_bytes_total",
        ] {
            assert!(
                out.contains(&format!("# TYPE {family} counter")),
                "{family}"
            );
        }
        for family in V1_FAMILIES.iter().filter(|f| !f.ends_with("_bytes_total")) {
            assert!(
                out.contains(&format!("# TYPE {family} gauge")),
                "{family} should be a gauge"
            );
        }
        assert_eq!(out.matches(" counter\n").count(), 2);
    }

    #[test]
    fn negative_or_unparseable_counters_are_omitted() {
        let metrics: TypesenseMetrics = serde_json::from_str(
            r#"{"system_network_received_bytes": "-1", "system_network_sent_bytes": "n/a"}"#,
        )
        .unwrap();
        let scrape = Scrape {
            metrics: Some(metrics),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        assert!(!out.contains("typesense_system_network_"), "{out}");
        assert!(!out.contains("typesense_unmapped"), "{out}");
    }

    #[test]
    fn unknown_numeric_fields_fall_back_to_unmapped() {
        let metrics: TypesenseMetrics = serde_json::from_str(
            r#"{"some_future_field": "7", "numeric_field": 3.5, "system_cpuX_active_percentage": "9",
                "text_field": "abc", "object_field": {"a": 1}}"#,
        )
        .unwrap();
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"some_future_stat": 42, "search_50Percentile_latency_ms": 2,
                "export_requests_per_second": 1, "text_stat": "x", "null_stat": null}"#,
        )
        .unwrap();
        let scrape = Scrape {
            metrics: Some(metrics),
            stats: Some(stats),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);

        for (source, key, expected) in [
            ("metrics.json", "some_future_field", 7.0),
            ("metrics.json", "numeric_field", 3.5),
            ("metrics.json", "system_cpuX_active_percentage", 9.0),
            ("stats.json", "some_future_stat", 42.0),
            // raw value, no ms -> s conversion
            ("stats.json", "search_50Percentile_latency_ms", 2.0),
            ("stats.json", "export_requests_per_second", 1.0),
        ] {
            assert_value(
                &out,
                &format!(
                    r#"typesense_unmapped{{host="h",key="{key}",port="1",source="{source}"}}"#
                ),
                expected,
            );
        }
        for key in ["text_field", "object_field", "text_stat", "null_stat"] {
            assert!(!out.contains(key), "{key} is not a number");
        }
        assert!(!out.contains("typesense_request_latency_seconds{"));
    }

    #[test]
    fn unmapped_is_only_emitted_with_v1_names() {
        let out = generate_metrics(&scrape_with(MetricNames::Legacy), "h", 1);
        assert!(!out.contains("typesense_unmapped"));

        let out = generate_metrics(&scrape_with(MetricNames::Both), "h", 1);
        assert!(out.contains(
            r#"typesense_unmapped{host="h",key="some_future_stat",port="1",source="stats.json"} 42"#
        ));
    }

    #[test]
    fn v1_families_are_omitted_not_zeroed() {
        // Endpoints that failed.
        let out = generate_metrics(&Scrape::default(), "h", 1);
        for family in V1_FAMILIES {
            assert!(!has_family(&out, family), "{family} should be absent");
        }
        assert!(out.contains(r#"typesense_health{host="h",port="1"} 0"#));
        assert!(out.contains(r#"typesense_raft_state{host="h",port="1"} 0"#));

        // Endpoints that answered but left fields out, or sent non-numbers.
        let scrape = Scrape {
            metrics: Some(
                serde_json::from_str(
                    r#"{"system_disk_total_bytes": "100", "system_disk_used_bytes": "n/a",
                        "system_cpu3_active_percentage": "busy"}"#,
                )
                .unwrap(),
            ),
            stats: Some(
                serde_json::from_str(r#"{"search_latency_ms": "slow", "cache_hit_ratio": null}"#)
                    .unwrap(),
            ),
            ..Scrape::default()
        };
        let out = generate_metrics(&scrape, "h", 1);
        assert!(out.contains(r#"typesense_system_disk_total_bytes{host="h",port="1"} 100"#));
        for family in V1_FAMILIES
            .iter()
            .filter(|f| **f != "typesense_system_disk_total_bytes")
        {
            assert!(
                !has_family(&out, family),
                "{family} should be absent\n{out}"
            );
        }
    }

    #[test]
    fn each_mode_emits_only_its_names() {
        let v1 = generate_metrics(&scrape_with(MetricNames::V1), "h", 1);
        let legacy = generate_metrics(&scrape_with(MetricNames::Legacy), "h", 1);
        let both = generate_metrics(&scrape_with(MetricNames::Both), "h", 1);

        for family in V1_FAMILIES {
            assert!(has_family(&v1, family), "v1 lacks {family}");
            assert!(has_family(&both, family), "both lacks {family}");
            assert!(!has_family(&legacy, family), "legacy has {family}");
        }
        for family in LEGACY_FAMILIES {
            assert!(!has_family(&v1, family), "v1 has {family}");
            assert!(has_family(&both, family), "both lacks {family}");
            assert!(has_family(&legacy, family), "legacy lacks {family}");
        }
    }

    #[test]
    fn unchanged_families_are_emitted_once_in_every_mode() {
        for names in [MetricNames::V1, MetricNames::Both, MetricNames::Legacy] {
            let out = generate_metrics(&scrape_with(names), "h", 1);
            for family in [
                "typesense_health",
                "typesense_raft_state",
                "typesense_info",
                "typesense_raft_committed_index",
                "typesense_raft_queued_writes",
                "typesense_collection_documents",
                "typesense_scrape_success",
                "typesense_scrape_duration_seconds",
            ] {
                assert_eq!(
                    out.matches(&format!("# TYPE {family} gauge\n")).count(),
                    1,
                    "{family} in {names:?}"
                );
            }
        }
    }

    #[test]
    fn both_mode_keeps_legacy_output_byte_for_byte() {
        let legacy = generate_metrics(&scrape_with(MetricNames::Legacy), "h", 1);
        let both = generate_metrics(&scrape_with(MetricNames::Both), "h", 1);

        // Every legacy line, HELP and TYPE included, appears unchanged.
        for line in legacy.lines() {
            assert!(both.lines().any(|l| l == line), "both lacks {line:?}");
        }
        // And legacy has exactly the 0.4 shape.
        assert!(legacy.contains(
            "# HELP typesense_metrics Data received through the metrics api of typesense\n# TYPE typesense_metrics gauge\n"
        ));
        assert!(legacy.contains(
            r#"typesense_metrics{host="h",key="system_network_received_bytes",port="1"} 1466"#
        ));
        assert!(
            legacy.contains(r#"typesense_stats{host="h",key="search_latency_ms",port="1"} 1.5"#)
        );
        assert!(legacy.contains(
            r#"typesense_stats_latency_ms_by_collection{action="documents/search",collection_name="products",host="h",method="GET",port="1"} 2.5"#
        ));
        assert!(!legacy.contains("some_future_field"));
    }
}
