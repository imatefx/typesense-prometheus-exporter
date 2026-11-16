//! Integration tests: the real exporter router, served over HTTP, scraping a
//! mock Typesense server.

use std::time::{Duration, Instant};

use serde_json::json;
use typesense_prometheus_exporter::{cli::CliArgs, server};
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
        typesense_host: host.to_string(),
        typesense_protocol: "http".to_string(),
        typesense_api_key: Some(API_KEY.to_string()),
        typesense_api_key_file: None,
        typesense_port: port,
        exporter_bind_address: "127.0.0.1".to_string(),
        exporter_bind_port: 0,
        typesense_timeout: timeout,
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
    let addr = mock.address();
    spawn_exporter(args_for(&addr.ip().to_string(), addr.port(), timeout)).await
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
        r#"typesense_metrics{{host="127.0.0.1",key="system_memory_total_swap_bytes",port="{port}"}} 1073741824"#
    )));
    assert!(out.contains(r#"key="system_cpu2_active_percentage""#));
    assert!(out.contains(r#"key="cache_hit_ratio""#));
    assert!(out.contains(
        r#"typesense_stats_requests_per_second_by_collection{action="documents/search",collection_name="products""#
    ));
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
    // leave typesense_metrics out of the response.
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, 0).await;

    let (_, out) = scrape(&base).await;

    assert!(out.contains("typesense_metrics{"));
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
    assert!(!out.contains("typesense_metrics{"));
    assert!(!out.contains("typesense_stats{"));
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
    assert!(!out.contains("typesense_metrics{"));
    assert!(!out.contains("typesense_stats{"));
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

    assert!(!out.contains("typesense_metrics{"));
    assert!(out.contains("typesense_stats{"));
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
    assert!(!out.contains("typesense_metrics{"));
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
    assert!(out.contains("typesense_metrics{"));
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
            r#"typesense_stats{{host="127.0.0.1",key="search_99Percentile_latency_ms",port="{port}"}} 4"#
        ),
        format!(
            r#"typesense_stats{{host="127.0.0.1",key="import_max_latency_ms",port="{port}"}} 7"#
        ),
        format!(r#"typesense_stats{{host="127.0.0.1",key="cache_miss_count",port="{port}"}} 15"#),
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
    assert!(out.contains("typesense_metrics{"));
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
        .filter(|l| l.starts_with("typesense_stats_requests_per_second{"))
        .collect();
    assert_eq!(rps_lines.len(), 2, "{rps_lines:#?}");
    assert!(out.contains(r#"key="GET /collections/products/documents/:id",port="#));
    assert!(
        rps_lines
            .iter()
            .any(|l| l.contains("documents/:id") && l.ends_with(" 50")),
        "rates are summed"
    );
    let by_collection: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("typesense_stats_requests_per_second_by_collection{"))
        .collect();
    assert_eq!(by_collection.len(), 2, "{by_collection:#?}");
    assert!(
        by_collection.iter().all(|l| !l.contains("key=")),
        "no duplicated key label"
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
    assert!(out.contains("typesense_metrics{"));
    assert_eq!(scrape_success(&out, "/stats.json"), Some(1.0));
}

#[tokio::test]
async fn startup_fails_without_a_readable_key_file() {
    let mut args = args_for("127.0.0.1", 1, 0);
    args.typesense_api_key = None;
    args.typesense_api_key_file = Some("/nonexistent/typesense-key".into());

    assert!(server::router(&args).is_err());
}
