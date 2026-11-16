//! Integration tests: the real exporter router, served over HTTP, scraping a
//! mock Typesense server.

use std::time::{Duration, Instant};

use serde_json::json;
use typesense_prometheus_exporter::{
    cli::{CliArgs, MetricNames},
    server,
};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

const API_KEY: &str = "test-key";
const METRICS: &str = include_str!("fixtures/metrics.json");
const STATS: &str = include_str!("fixtures/stats.json");
const COLLECTIONS: &str = include_str!("fixtures/collections.json");

fn args_for(host: &str, port: u16, timeout: u64) -> CliArgs {
    CliArgs {
        typesense_host: Some(host.to_string()),
        typesense_protocol: "http".to_string(),
        typesense_api_key: Some(API_KEY.to_string()),
        typesense_api_key_file: None,
        typesense_port: port,
        exporter_bind_address: "127.0.0.1".to_string(),
        exporter_bind_port: 0,
        typesense_timeout: timeout,
        metric_names: MetricNames::default(),
        probe_allowed_targets: Vec::new(),
    }
}

/// Serve the exporter on an ephemeral port and return its base URL.
async fn spawn_exporter(args: CliArgs) -> String {
    let app = server::router(&args).expect("router");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn exporter_for(mock: &MockServer, timeout: u64) -> String {
    exporter_with_names(mock, timeout, MetricNames::default()).await
}

async fn exporter_with_names(mock: &MockServer, timeout: u64, names: MetricNames) -> String {
    let addr = mock.address();
    let mut args = args_for(&addr.ip().to_string(), addr.port(), timeout);
    args.metric_names = names;
    spawn_exporter(args).await
}

async fn scrape(base: &str) -> (u16, String) {
    let res = reqwest::get(format!("{base}/metrics")).await.unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

/// Mount a JSON response for `route` that only matches requests carrying the API key.
async fn mount(mock: &MockServer, route: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(route))
        .and(header("x-typesense-api-key", API_KEY))
        .respond_with(response)
        .mount(mock)
        .await;
}

fn json_body(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "application/json")
}

/// A healthy Typesense node with the given raft state.
async fn healthy_node(raft_state: i32) -> MockServer {
    let mock = MockServer::start().await;
    mount(&mock, "/metrics.json", json_body(METRICS)).await;
    mount(&mock, "/stats.json", json_body(STATS)).await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
    )
    .await;
    mount(
        &mock,
        "/debug",
        ResponseTemplate::new(200).set_body_json(json!({"state": raft_state, "version": "30.1"})),
    )
    .await;
    mount(
        &mock,
        "/status",
        ResponseTemplate::new(200)
            .set_body_json(json!({"committed_index": 1042, "queued_writes": 3, "state": "LEADER"})),
    )
    .await;
    mount(&mock, "/collections", json_body(COLLECTIONS)).await;
    mock
}

fn gauge(out: &str, prefix: &str) -> Option<f64> {
    out.lines()
        .find(|l| l.starts_with(prefix))
        .and_then(|l| l.rsplit(' ').next())
        .and_then(|v| v.parse().ok())
}

/// Whether any `/metrics.json` value was exported under its 1.0 name.
fn has_metrics_json(out: &str) -> bool {
    out.contains("typesense_system_") || out.contains("typesense_process_memory_")
}

/// Whether any `/stats.json` value was exported under its 1.0 name.
fn has_stats_json(out: &str) -> bool {
    [
        "typesense_all_requests_per_second{",
        "typesense_requests_per_second{",
        "typesense_route_",
    ]
    .iter()
    .any(|family| out.contains(family))
}

fn health(out: &str) -> Option<f64> {
    gauge(out, "typesense_health{")
}

fn raft(out: &str) -> Option<f64> {
    gauge(out, "typesense_raft_state{")
}

#[tokio::test]
async fn healthy_leader_exports_everything() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
    assert_eq!(raft(&out), Some(1.0));
    let port = mock.address().port();
    assert!(out.contains(&format!(
        r#"typesense_system_swap_total_bytes{{host="127.0.0.1",port="{port}"}} 1073741824"#
    )));
    assert!(out.contains(r#"typesense_system_cpu_core_active_ratio{core="2","#));
    assert!(out.contains("typesense_cache_hit_ratio{"));
    assert!(out.contains(
        r#"typesense_collection_requests_per_second{action="documents/search",collection="products""#
    ));
    assert!(
        !out.contains("typesense_metrics{") && !out.contains("typesense_stats"),
        "v1 is the default, so no 0.4 names"
    );
}

#[tokio::test]
async fn follower_reports_raft_state_four() {
    let mock = healthy_node(4).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(raft(&out), Some(4.0));
}

#[tokio::test]
async fn sends_api_key_to_typesense() {
    // Every mock route requires the API key; a missing header would 404 and
    // leave the /metrics.json families out of the response.
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert!(out.contains("typesense_system_memory_total_bytes{"));
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6, "one request per Typesense endpoint");
    assert!(requests
        .iter()
        .all(|r| r.headers.get("x-typesense-api-key").unwrap() == API_KEY));
    assert!(
        requests
            .iter()
            .any(|r| r.url.path() == "/collections"
                && r.url.query() == Some("exclude_fields=fields")),
        "collection schemas are not downloaded"
    );
}

#[tokio::test]
async fn unreachable_typesense_reports_unhealthy_and_omits_metrics() {
    // Bind then drop a listener to get a port nothing is listening on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let base = spawn_exporter(args_for("127.0.0.1", port, 0)).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(0.0));
    assert_eq!(raft(&out), Some(0.0));
    assert!(!has_metrics_json(&out));
    assert!(!has_stats_json(&out));
}

#[tokio::test]
async fn unauthorized_endpoints_are_omitted() {
    let mock = MockServer::start().await;
    for route in ["/metrics.json", "/stats.json", "/debug"] {
        mount(&mock, route, ResponseTemplate::new(401)).await;
    }
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
    assert_eq!(raft(&out), Some(0.0));
    assert!(!has_metrics_json(&out));
    assert!(!has_stats_json(&out));
}

#[tokio::test]
async fn unhealthy_node_reports_zero_health() {
    let mock = MockServer::start().await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(503)
            .set_body_json(json!({"ok": false, "resource_error": "OUT_OF_DISK"})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(health(&out), Some(0.0));
}

#[tokio::test]
async fn health_ok_false_with_200_is_unhealthy() {
    let mock = MockServer::start().await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": false})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(health(&out), Some(0.0));
}

#[tokio::test]
async fn malformed_response_only_drops_that_endpoint() {
    let mock = healthy_node(1).await;
    mock.reset().await;
    mount(&mock, "/metrics.json", json_body("{not json")).await;
    mount(&mock, "/stats.json", json_body(STATS)).await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
    )
    .await;
    mount(
        &mock,
        "/debug",
        ResponseTemplate::new(200).set_body_json(json!({"state": 1})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert!(!has_metrics_json(&out));
    assert!(has_stats_json(&out));
    assert_eq!(health(&out), Some(1.0));
}

#[tokio::test]
async fn timeout_bounds_a_hung_typesense() {
    let mock = MockServer::start().await;
    let slow = json_body(METRICS).set_delay(Duration::from_secs(10));
    for route in ["/metrics.json", "/stats.json", "/health", "/debug"] {
        mount(&mock, route, slow.clone()).await;
    }
    let base = exporter_for(&mock, 1).await;

    let started = Instant::now();
    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(health(&out), Some(0.0));
    assert!(!has_metrics_json(&out));
}

#[tokio::test]
async fn root_points_to_metrics() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let body = reqwest::get(&base).await.unwrap().text().await.unwrap();

    assert!(body.contains("/metrics"));
}

#[tokio::test]
async fn serve_runs_the_exporter_on_a_given_listener() {
    let mock = healthy_node(1).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let args = args_for(&mock.address().ip().to_string(), mock.address().port(), 2);
    let task = tokio::spawn(server::serve(listener, args));

    let (status, out) = scrape(&base).await;
    task.abort();

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
}

#[tokio::test]
async fn run_fails_when_the_port_is_taken() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut args = args_for("127.0.0.1", 1, 0);
    args.exporter_bind_port = taken.local_addr().unwrap().port();

    let err = server::run(args).await.unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
}

#[tokio::test]
async fn zero_timeout_means_no_timeout() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(health(&out), Some(1.0));
    assert!(has_metrics_json(&out));
}

#[tokio::test]
async fn exports_version_replication_documents_and_percentiles() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    let port = mock.address().port();
    for line in [
        format!(r#"typesense_info{{host="127.0.0.1",port="{port}",version="30.1"}} 1"#),
        format!(r#"typesense_raft_committed_index{{host="127.0.0.1",port="{port}"}} 1042"#),
        format!(r#"typesense_raft_queued_writes{{host="127.0.0.1",port="{port}"}} 3"#),
        format!(
            r#"typesense_collection_documents{{collection="products",host="127.0.0.1",port="{port}"}} 1200"#
        ),
        format!(
            r#"typesense_collection_documents{{collection="articles",host="127.0.0.1",port="{port}"}} 37"#
        ),
        format!(
            r#"typesense_request_latency_seconds{{host="127.0.0.1",operation="search",port="{port}",quantile="0.99"}} 0.004"#
        ),
        format!(
            r#"typesense_request_latency_seconds{{host="127.0.0.1",operation="import",port="{port}",quantile="1"}} 0.007"#
        ),
        format!(r#"typesense_cache_misses{{host="127.0.0.1",port="{port}"}} 15"#),
    ] {
        assert!(out.contains(&line), "missing {line}");
    }
}

#[tokio::test]
async fn older_typesense_without_status_still_exports_the_rest() {
    // Typesense versions without /status answer 404; only that endpoint's
    // metrics should disappear.
    let mock = healthy_node(1).await;
    mock.reset().await;
    mount(&mock, "/metrics.json", json_body(METRICS)).await;
    mount(&mock, "/stats.json", json_body(STATS)).await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
    )
    .await;
    mount(
        &mock,
        "/debug",
        ResponseTemplate::new(200).set_body_json(json!({"state": 1})),
    )
    .await;
    mount(&mock, "/collections", json_body(COLLECTIONS)).await;
    let base = exporter_for(&mock, 0).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert!(!out.contains("typesense_raft_committed_index{"));
    assert!(
        !out.contains("typesense_info{"),
        "no version reported, so no info metric"
    );
    assert!(out.contains("typesense_collection_documents{"));
    assert!(has_metrics_json(&out));
    assert_eq!(health(&out), Some(1.0));
}

fn scrape_success(out: &str, endpoint: &str) -> Option<f64> {
    out.lines()
        .find(|l| {
            l.starts_with("typesense_scrape_success{")
                && l.contains(&format!(r#"endpoint="{endpoint}""#))
        })
        .and_then(|l| l.rsplit(' ').next())
        .and_then(|v| v.parse().ok())
}

#[tokio::test]
async fn scrape_health_reports_each_endpoint_and_the_duration() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    for endpoint in [
        "/metrics.json",
        "/stats.json",
        "/health",
        "/debug",
        "/status",
        "/collections",
    ] {
        assert_eq!(scrape_success(&out, endpoint), Some(1.0), "{endpoint}");
    }
    let duration = gauge(&out, "typesense_scrape_duration_seconds{");
    assert!(
        duration.is_some_and(|d| (0.0..5.0).contains(&d)),
        "{duration:?}"
    );
}

#[tokio::test]
async fn scrape_health_flags_the_failing_endpoints() {
    let mock = MockServer::start().await;
    mount(&mock, "/metrics.json", ResponseTemplate::new(401)).await;
    mount(&mock, "/stats.json", json_body("{broken")).await;
    mount(
        &mock,
        "/health",
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(scrape_success(&out, "/metrics.json"), Some(0.0), "401");
    assert_eq!(
        scrape_success(&out, "/stats.json"),
        Some(0.0),
        "malformed JSON"
    );
    assert_eq!(scrape_success(&out, "/status"), Some(0.0), "not served");
    assert_eq!(scrape_success(&out, "/health"), Some(1.0));
}

#[tokio::test]
async fn document_ids_do_not_create_new_series() {
    // Typesense names stats entries after the request path, document IDs
    // included. Every ID must fold into one bounded `documents/:id` series.
    let mock = healthy_node(1).await;
    mock.reset().await;
    let mut latency = serde_json::Map::new();
    let mut rps = serde_json::Map::new();
    for id in 0..50 {
        latency.insert(
            format!("GET /collections/products/documents/{id}"),
            json!(2.0),
        );
        rps.insert(
            format!("GET /collections/products/documents/{id}"),
            json!(1.0),
        );
    }
    latency.insert(
        "GET /collections/products/documents/search".into(),
        json!(4.0),
    );
    rps.insert(
        "GET /collections/products/documents/search".into(),
        json!(10.0),
    );
    mount(
        &mock,
        "/stats.json",
        ResponseTemplate::new(200)
            .set_body_json(json!({"latency_ms": latency, "requests_per_second": rps})),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    let rps_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("typesense_route_requests_per_second{"))
        .collect();
    assert_eq!(rps_lines.len(), 2, "{rps_lines:#?}");
    assert!(out.contains(r#"route="GET /collections/products/documents/:id"}"#));
    assert!(
        rps_lines
            .iter()
            .any(|l| l.contains("documents/:id") && l.ends_with(" 50")),
        "rates are summed"
    );
    let by_collection: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("typesense_collection_requests_per_second{"))
        .collect();
    assert_eq!(by_collection.len(), 2, "{by_collection:#?}");
    assert!(
        by_collection.iter().all(|l| !l.contains("route=")),
        "no duplicated route label"
    );
    assert!(by_collection
        .iter()
        .any(|l| l.contains(r#"action="documents/:id""#)));
}

#[tokio::test]
async fn reads_the_api_key_from_a_file() {
    let mock = healthy_node(1).await;
    let path = std::env::temp_dir().join(format!("exporter-key-{}", std::process::id()));
    std::fs::write(&path, format!("{API_KEY}\n")).unwrap();
    let mut args = args_for(&mock.address().ip().to_string(), mock.address().port(), 0);
    args.typesense_api_key = None;
    args.typesense_api_key_file = Some(path.clone());
    let base = spawn_exporter(args).await;

    let (_, out) = scrape(&base).await;
    std::fs::remove_file(&path).unwrap();

    // The mocks only answer requests carrying the key.
    assert!(has_metrics_json(&out));
    assert_eq!(scrape_success(&out, "/stats.json"), Some(1.0));
}

#[tokio::test]
async fn startup_fails_without_a_readable_key_file() {
    let mut args = args_for("127.0.0.1", 1, 0);
    args.typesense_api_key = None;
    args.typesense_api_key_file = Some("/nonexistent/typesense-key".into());

    assert!(server::router(&args).is_err());
}

/// Families that only exist under the 0.4 names.
const LEGACY_FAMILIES: &[&str] = &[
    "typesense_metrics",
    "typesense_stats",
    "typesense_stats_latency_ms",
    "typesense_stats_requests_per_second",
    "typesense_stats_latency_ms_by_collection",
    "typesense_stats_requests_per_second_by_collection",
];

/// A sample of families that only exist under the 1.0 names.
const V1_FAMILIES: &[&str] = &[
    "typesense_system_cpu_active_ratio",
    "typesense_system_cpu_core_active_ratio",
    "typesense_system_network_received_bytes_total",
    "typesense_process_memory_resident_bytes",
    "typesense_requests_per_second",
    "typesense_all_requests_per_second",
    "typesense_request_latency_avg_seconds",
    "typesense_request_latency_seconds",
    "typesense_cache_hit_ratio",
    "typesense_route_latency_seconds",
    "typesense_collection_requests_per_second",
];

fn has_family(out: &str, family: &str) -> bool {
    out.contains(&format!("# TYPE {family} "))
}

fn assert_close(out: &str, series: &str, expected: f64) {
    let got = gauge(out, &format!("{series} ")).unwrap_or_else(|| panic!("missing {series}"));
    assert!(
        (got - expected).abs() < 1e-12,
        "{series}: got {got}, want {expected}"
    );
}

#[tokio::test]
async fn v1_names_are_the_default_with_base_units_and_counters() {
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    let port = mock.address().port();
    let t = format!(r#"host="127.0.0.1",port="{port}""#);
    for family in V1_FAMILIES {
        assert!(has_family(&out, family), "missing {family}");
    }
    for family in LEGACY_FAMILIES {
        assert!(!has_family(&out, family), "unexpected {family}");
    }
    assert!(out.contains("# TYPE typesense_system_network_received_bytes_total counter"));
    assert!(out.contains("# TYPE typesense_system_network_sent_bytes_total counter"));
    assert_close(
        &out,
        &format!("typesense_system_network_received_bytes_total{{{t}}}"),
        1466.0,
    );
    assert_close(
        &out,
        &format!("typesense_system_cpu_active_ratio{{{t}}}"),
        0.0455,
    );
    assert_close(
        &out,
        &format!(r#"typesense_system_cpu_core_active_ratio{{core="1",{t}}}"#),
        0.0909,
    );
    assert_close(
        &out,
        &format!(
            r#"typesense_request_latency_avg_seconds{{host="127.0.0.1",operation="import",port="{port}"}}"#
        ),
        0.0025,
    );
    assert_close(
        &out,
        &format!(
            r#"typesense_requests_per_second{{host="127.0.0.1",operation="search",port="{port}"}}"#
        ),
        3.4,
    );
    assert_close(
        &out,
        &format!("typesense_all_requests_per_second{{{t}}}"),
        3.6,
    );
    assert_close(
        &out,
        &format!(r#"typesense_route_latency_seconds{{{t},route="GET /health"}}"#),
        0.0001,
    );
    assert!(
        !out.contains("typesense_unmapped"),
        "every fixture field has a 1.0 name"
    );
}

#[tokio::test]
async fn legacy_names_reproduce_the_0_4_output() {
    let mock = healthy_node(1).await;
    let base = exporter_with_names(&mock, 0, MetricNames::Legacy).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    for family in LEGACY_FAMILIES {
        assert!(has_family(&out, family), "missing {family}");
    }
    for family in V1_FAMILIES {
        assert!(!has_family(&out, family), "unexpected {family}");
    }
    assert!(!out.contains("typesense_unmapped"));
    assert!(!out.contains(" counter\n"), "0.4 exported only gauges");
    let port = mock.address().port();
    for line in [
        format!(
            r#"typesense_metrics{{host="127.0.0.1",key="system_memory_total_swap_bytes",port="{port}"}} 1073741824"#
        ),
        format!(
            r#"typesense_metrics{{host="127.0.0.1",key="system_cpu2_active_percentage",port="{port}"}} 0"#
        ),
        format!(r#"typesense_stats{{host="127.0.0.1",key="cache_hit_ratio",port="{port}"}} 0.25"#),
        format!(
            r#"typesense_stats{{host="127.0.0.1",key="search_99Percentile_latency_ms",port="{port}"}} 4"#
        ),
        format!(
            r#"typesense_stats_requests_per_second_by_collection{{action="documents/search",collection_name="products",host="127.0.0.1",method="GET",port="{port}"}} 3.4"#
        ),
    ] {
        assert!(out.contains(&line), "missing {line}");
    }
    assert_eq!(health(&out), Some(1.0));
    assert!(out.contains("typesense_collection_documents{"));
}

#[tokio::test]
async fn both_names_emit_old_and_new_and_shared_families_once() {
    let mock = healthy_node(1).await;
    let both = exporter_with_names(&mock, 0, MetricNames::Both).await;
    let legacy = exporter_with_names(&mock, 0, MetricNames::Legacy).await;

    let (status, out) = scrape(&both).await;
    let (_, legacy_out) = scrape(&legacy).await;

    assert_eq!(status, 200);
    for family in LEGACY_FAMILIES.iter().chain(V1_FAMILIES) {
        assert!(has_family(&out, family), "missing {family}");
    }
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
            "{family}"
        );
    }
    // The 0.4 series are unchanged next to the new ones.
    for line in legacy_out.lines().filter(|l| {
        LEGACY_FAMILIES
            .iter()
            .any(|f| l.starts_with(&format!("{f}{{")))
    }) {
        assert!(out.lines().any(|l| l == line), "both lacks {line}");
    }
}

#[tokio::test]
async fn v1_unknown_fields_fall_back_to_unmapped() {
    let mock = MockServer::start().await;
    mount(
        &mock,
        "/metrics.json",
        json_body(r#"{"system_memory_total_bytes": "1024", "brand_new_gauge": "5"}"#),
    )
    .await;
    mount(
        &mock,
        "/stats.json",
        json_body(r#"{"search_latency_ms": 2, "brand_new_stat": 0.5}"#),
    )
    .await;
    let base = exporter_for(&mock, 0).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    let port = mock.address().port();
    assert_close(
        &out,
        &format!(
            r#"typesense_unmapped{{host="127.0.0.1",key="brand_new_gauge",port="{port}",source="metrics.json"}}"#
        ),
        5.0,
    );
    assert_close(
        &out,
        &format!(
            r#"typesense_unmapped{{host="127.0.0.1",key="brand_new_stat",port="{port}",source="stats.json"}}"#
        ),
        0.5,
    );
    assert_close(
        &out,
        &format!(
            r#"typesense_request_latency_avg_seconds{{host="127.0.0.1",operation="search",port="{port}"}}"#
        ),
        0.002,
    );
    assert!(
        !out.contains("typesense_system_cpu_active_ratio"),
        "fields Typesense didn't send are omitted, not zeroed"
    );
    assert_eq!(health(&out), Some(0.0), "/health not served");
}

// ---- /probe -----------------------------------------------------------------

/// An exporter with no `--typesense-host` that may probe `allowed`.
async fn probe_exporter(allowed: &[String], timeout: u64) -> String {
    let mut args = args_for("unused", 1, timeout);
    args.typesense_host = None;
    args.probe_allowed_targets = allowed.to_vec();
    spawn_exporter(args).await
}

fn node_addr(mock: &MockServer) -> String {
    let addr = mock.address();
    format!("{}:{}", addr.ip(), addr.port())
}

async fn probe(base: &str, target: &str) -> (u16, String) {
    let res = reqwest::Client::new()
        .get(format!("{base}/probe"))
        .query(&[("target", target)])
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

#[tokio::test]
async fn probe_scrapes_each_node_with_its_own_labels() {
    let leader = healthy_node(1).await;
    let follower = healthy_node(4).await;
    let (a, b) = (node_addr(&leader), node_addr(&follower));
    let base = probe_exporter(&[a.clone(), b.clone()], 0).await;

    let (status_a, out_a) = probe(&base, &a).await;
    let (status_b, out_b) = probe(&base, &format!("http://{b}")).await;

    assert_eq!((status_a, status_b), (200, 200));
    for (out, mock, raft_state) in [(&out_a, &leader, 1.0), (&out_b, &follower, 4.0)] {
        let addr = mock.address();
        let labels = format!(r#"host="{}",port="{}""#, addr.ip(), addr.port());
        assert_eq!(raft(out), Some(raft_state), "{out}");
        assert_eq!(health(out), Some(1.0));
        assert!(out.contains(&labels), "labelled with the node: {labels}");
        assert!(has_metrics_json(out) && has_stats_json(out));
    }
    assert!(
        !out_a.contains(&format!("port=\"{}\"", follower.address().port())),
        "no cross-talk between nodes"
    );
    for mock in [&leader, &follower] {
        let requests = mock.received_requests().await.unwrap();
        assert_eq!(requests.len(), 6, "exactly one scrape per probe");
        assert!(requests
            .iter()
            .all(|r| r.headers.get("x-typesense-api-key").unwrap() == API_KEY));
    }
}

#[tokio::test]
async fn probe_output_matches_metrics_for_the_same_node() {
    let mock = healthy_node(1).await;
    let addr = node_addr(&mock);
    let mut args = args_for(&mock.address().ip().to_string(), mock.address().port(), 0);
    args.probe_allowed_targets = vec![addr.clone()];
    let base = spawn_exporter(args).await;

    let (_, metrics) = scrape(&base).await;
    let (status, probed) = probe(&base, &addr).await;

    assert_eq!(status, 200);
    let stable = |out: &str| -> Vec<String> {
        out.lines()
            .filter(|l| !l.contains("scrape_duration"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(stable(&metrics), stable(&probed));
}

#[tokio::test]
async fn a_hung_node_only_affects_its_own_probe() {
    let healthy = healthy_node(1).await;
    let hung = MockServer::start().await;
    let slow = json_body(METRICS).set_delay(Duration::from_secs(10));
    for route in ["/metrics.json", "/stats.json", "/health", "/debug"] {
        mount(&hung, route, slow.clone()).await;
    }
    let dead_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dead = format!("127.0.0.1:{dead_port}");
    let (ok, stuck) = (node_addr(&healthy), node_addr(&hung));
    let base = probe_exporter(&[ok.clone(), stuck.clone(), dead.clone()], 1).await;

    let (healthy_res, hung_res, dead_res) = tokio::join!(
        async {
            let started = Instant::now();
            (probe(&base, &ok).await, started.elapsed())
        },
        probe(&base, &stuck),
        probe(&base, &dead),
    );

    let ((status, out), took) = healthy_res;
    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
    assert!(took < Duration::from_millis(900), "not held up: {took:?}");
    for (status, out) in [hung_res, dead_res] {
        assert_eq!(status, 200, "a down node is still a successful scrape");
        assert_eq!(health(&out), Some(0.0));
        assert_eq!(raft(&out), Some(0.0));
        assert!(!has_metrics_json(&out));
    }
}

#[tokio::test]
async fn probe_refuses_targets_that_are_not_allowed_without_contacting_them() {
    let allowed = healthy_node(1).await;
    let forbidden = healthy_node(1).await;
    let base = probe_exporter(&[node_addr(&allowed)], 0).await;

    for target in [
        node_addr(&forbidden),
        format!("https://{}", node_addr(&forbidden)),
        format!("127.0.0.1:{}", allowed.address().port() + 1),
    ] {
        let (status, body) = probe(&base, &target).await;
        assert_eq!(status, 403, "{target}");
        assert!(body.contains("not allowed"), "{body}");
    }

    let received = forbidden.received_requests().await.unwrap();
    assert!(
        received.is_empty(),
        "the API key must not leave: {received:?}"
    );
}

#[tokio::test]
async fn probe_matches_allowed_targets_case_and_scheme_insensitively() {
    let mock = healthy_node(1).await;
    let port = mock.address().port();
    let base = probe_exporter(&[format!("LocalHost:{port}")], 0).await;

    // `localhost` may resolve to ::1 first; only the allow decision matters.
    for target in [
        format!("localhost:{port}"),
        format!("LOCALHOST:{port}"),
        format!("http://LocalHost:{port}"),
    ] {
        let (status, _) = probe(&base, &target).await;
        assert_eq!(status, 200, "{target}");
    }
}

#[tokio::test]
async fn probe_always_allows_the_configured_host() {
    let mock = healthy_node(1).await;
    let mut args = args_for(&mock.address().ip().to_string(), mock.address().port(), 0);
    args.probe_allowed_targets = vec!["other.example:1".to_string()];
    let base = spawn_exporter(args).await;

    let (status, out) = probe(&base, &node_addr(&mock)).await;

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
}

#[tokio::test]
async fn a_wildcard_allows_any_target() {
    let a = healthy_node(1).await;
    let b = healthy_node(4).await;
    let base = probe_exporter(&["*".to_string()], 0).await;

    let (status_a, out_a) = probe(&base, &node_addr(&a)).await;
    let (status_b, out_b) = probe(&base, &node_addr(&b)).await;

    assert_eq!((status_a, status_b), (200, 200));
    assert_eq!((raft(&out_a), raft(&out_b)), (Some(1.0), Some(4.0)));
}

#[tokio::test]
async fn probe_rejects_malformed_targets() {
    let mock = healthy_node(1).await;
    let addr = node_addr(&mock);
    let base = probe_exporter(&["*".to_string()], 0).await;

    for target in [
        "".to_string(),
        "   ".to_string(),
        "ftp://host".to_string(),
        format!("http://{addr}/metrics"),
        format!("{addr}/x"),
        format!("user:pw@{addr}"),
        "host:notaport".to_string(),
        "::1".to_string(),
    ] {
        let (status, body) = probe(&base, &target).await;
        assert_eq!(status, 400, "{target:?}: {body}");
        assert!(!body.is_empty());
    }

    let missing = reqwest::get(format!("{base}/probe")).await.unwrap();
    assert_eq!(missing.status().as_u16(), 400);
    assert!(missing.text().await.unwrap().contains("target"));

    assert!(
        mock.received_requests().await.unwrap().is_empty(),
        "nothing was scraped"
    );
}

#[tokio::test]
async fn metrics_is_404_without_a_host_and_points_to_probe() {
    let base = probe_exporter(&["ts".to_string()], 0).await;

    let res = reqwest::get(format!("{base}/metrics")).await.unwrap();

    assert_eq!(res.status().as_u16(), 404);
    assert!(res.text().await.unwrap().contains("/probe?target="));
}

#[tokio::test]
async fn metrics_is_unchanged_when_a_host_is_set() {
    let mock = healthy_node(1).await;
    let addr = mock.address();
    let mut args = args_for(&addr.ip().to_string(), addr.port(), 0);
    args.probe_allowed_targets = vec!["other:1".to_string()];
    let base = spawn_exporter(args).await;

    let (status, out) = scrape(&base).await;

    assert_eq!(status, 200);
    assert_eq!(health(&out), Some(1.0));
    assert!(out.contains(&format!(r#"host="{}",port="{}""#, addr.ip(), addr.port())));
}

#[tokio::test]
async fn root_mentions_probe() {
    let base = probe_exporter(&["ts".to_string()], 0).await;

    let body = reqwest::get(&base).await.unwrap().text().await.unwrap();

    assert!(body.contains("/metrics") && body.contains("/probe"));
}

#[test]
fn router_rejects_a_malformed_allowed_target() {
    let mut args = args_for("127.0.0.1", 1, 0);
    args.probe_allowed_targets = vec!["http://ts/path".to_string()];

    assert!(server::router(&args).is_err());
}

#[test]
fn router_needs_a_host_or_a_non_empty_allow_list() {
    let mut args = args_for("unused", 1, 0);
    args.typesense_host = None;
    args.probe_allowed_targets = vec![String::new()];

    assert!(server::router(&args).is_err());
}
