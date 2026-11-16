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

fn args_for(host: &str, port: u16, timeout: i64) -> CliArgs {
    CliArgs {
        typesense_host: host.to_string(),
        typesense_protocol: "http".to_string(),
        typesense_api_key: API_KEY.to_string(),
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

async fn exporter_for(mock: &MockServer, timeout: i64) -> String {
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
        ResponseTemplate::new(200).set_body_json(json!({"state": raft_state, "version": "27.1"})),
    )
    .await;
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
    let base = exporter_for(&mock, -1).await;

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
    let base = exporter_for(&mock, -1).await;

    let (_, out) = scrape(&base).await;

    assert_eq!(raft(&out), Some(4.0));
}

#[tokio::test]
async fn sends_api_key_to_typesense() {
    // Every mock route requires the API key; a missing header would 404 and
    // leave typesense_metrics out of the response.
    let mock = healthy_node(1).await;
    let base = exporter_for(&mock, -1).await;

    let (_, out) = scrape(&base).await;

    assert!(out.contains("typesense_metrics{"));
    let requests = mock.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4, "one request per Typesense endpoint");
    assert!(requests
        .iter()
        .all(|r| r.headers.get("x-typesense-api-key").unwrap() == API_KEY));
}

#[tokio::test]
async fn unreachable_typesense_reports_unhealthy_and_omits_metrics() {
    // Bind then drop a listener to get a port nothing is listening on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let base = spawn_exporter(args_for("127.0.0.1", port, -1)).await;

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
    let base = exporter_for(&mock, -1).await;

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
    let base = exporter_for(&mock, -1).await;

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
    let base = exporter_for(&mock, -1).await;

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
    let base = exporter_for(&mock, -1).await;

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
    let base = exporter_for(&mock, -1).await;

    let body = reqwest::get(&base).await.unwrap().text().await.unwrap();

    assert!(body.contains("/metrics"));
}

#[tokio::test]
async fn run_serves_on_the_configured_address() {
    let mock = healthy_node(1).await;
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut args = args_for(&mock.address().ip().to_string(), mock.address().port(), 2);
    args.exporter_bind_port = port;
    let task = tokio::spawn(server::run(args));

    let base = format!("http://127.0.0.1:{port}");
    let mut out = None;
    for _ in 0..50 {
        if let Ok(res) = reqwest::get(format!("{base}/metrics")).await {
            out = Some(res.text().await.unwrap());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    task.abort();

    assert_eq!(health(&out.expect("exporter never came up")), Some(1.0));
}

#[tokio::test]
async fn run_fails_when_the_port_is_taken() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut args = args_for("127.0.0.1", 1, -1);
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
