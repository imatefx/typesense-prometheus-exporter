//! End-to-end tests against the docker compose stack: a 3-node Typesense
//! cluster, one exporter per node, Prometheus and Grafana.
//!
//! Run with `make e2e` (or `cargo test --features e2e --test e2e`, then
//! `docker compose down -v`). The tests start the stack themselves and leave it
//! running; tear it down afterwards.
//!
//! Each scenario is its own test. Scenarios that stop or pause nodes restore
//! the cluster before finishing, and every test first waits for a healthy
//! cluster, so tests do not depend on each other's order. A lock keeps them
//! from running concurrently within one process; nextest runs them in a
//! single-threaded test group (see `.config/nextest.toml`).

use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    process::Command,
    sync::OnceLock,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tokio::sync::{Mutex, MutexGuard};

const API_KEY: &str = "local-dev-key";
const PROMETHEUS: &str = "http://localhost:9090";
const GRAFANA: &str = "http://localhost:3000";
const NODES: [u8; 3] = [1, 2, 3];
const RAFT_LEADER: f64 = 1.0;
const RAFT_FOLLOWER: f64 = 4.0;

fn typesense_url(node: u8) -> String {
    format!("http://localhost:{}", 8107 + u16::from(node))
}

fn exporter_url(node: u8) -> String {
    format!("http://localhost:{}", 8887 + u16::from(node))
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Serialize scenarios and bring the stack up (once per process).
async fn stack() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::const_new(());
    static UP: OnceLock<()> = OnceLock::new();

    let guard = LOCK.lock().await;
    UP.get_or_init(|| compose(&["up", "-d", "--build", "--wait", "--wait-timeout", "240"]));
    guard
}

fn compose(args: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("docker")
        .arg("compose")
        .args(args)
        .current_dir(root)
        .status()
        .expect("failed to run docker compose");
    assert!(status.success(), "docker compose {args:?} failed: {status}");
}

/// Retry `check` until it returns `Ok` or `timeout` passes, then panic with the
/// last error.
async fn eventually<F, Fut>(desc: &str, timeout: Duration, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        match check().await {
            Ok(()) => return,
            Err(err) if Instant::now() >= deadline => {
                panic!("{desc}: still failing after {timeout:?}: {err}")
            }
            Err(_) => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
}

fn ensure(cond: bool, msg: impl Into<String>) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(msg.into())
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Sample {
    name: String,
    labels: BTreeMap<String, String>,
    value: f64,
}

/// Parse the Prometheus text format (enough of it for this exporter's output).
fn parse_samples(text: &str) -> Vec<Sample> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|line| {
            let (series, value) = line.rsplit_once(' ')?;
            let (name, labels) = match series.split_once('{') {
                Some((name, rest)) => (name, parse_labels(rest.strip_suffix('}')?)),
                None => (series, BTreeMap::new()),
            };
            Some(Sample {
                name: name.to_string(),
                labels,
                value: value.parse().ok()?,
            })
        })
        .collect()
}

fn parse_labels(s: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let mut rest = s;
    while let Some((key, after)) = rest.split_once("=\"") {
        let end = after.find('"').unwrap_or(after.len());
        labels.insert(
            key.trim_start_matches(',').to_string(),
            after[..end].to_string(),
        );
        rest = after.get(end + 1..).unwrap_or("");
    }
    labels
}

async fn scrape(node: u8) -> Result<Vec<Sample>, String> {
    let res = client()
        .get(format!("{}/metrics", exporter_url(node)))
        .send()
        .await
        .map_err(|e| format!("exporter-{node}: {e}"))?;
    ensure(
        res.status().is_success(),
        format!("exporter-{node}: HTTP {}", res.status()),
    )?;
    Ok(parse_samples(&res.text().await.map_err(|e| e.to_string())?))
}

fn gauge(samples: &[Sample], name: &str) -> Option<f64> {
    samples.iter().find(|s| s.name == name).map(|s| s.value)
}

async fn node_gauge_is(node: u8, name: &str, expected: f64) -> Result<(), String> {
    let actual = gauge(&scrape(node).await?, name);
    ensure(
        actual == Some(expected),
        format!("exporter-{node} {name} = {actual:?}, want {expected}"),
    )
}

/// The node whose exporter currently reports raft leader.
async fn leader() -> Option<u8> {
    for node in NODES {
        if node_gauge_is(node, "typesense_raft_state", RAFT_LEADER)
            .await
            .is_ok()
        {
            return Some(node);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Prometheus, Grafana, Typesense
// ---------------------------------------------------------------------------

/// Instant query; returns each sample's labels and value.
async fn prom_query(query: &str) -> Result<Vec<(Value, f64)>, String> {
    let body: Value = client()
        .get(format!("{PROMETHEUS}/api/v1/query"))
        .query(&[("query", query)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let results = body["data"]["result"]
        .as_array()
        .ok_or(format!("bad response: {body}"))?;
    Ok(results
        .iter()
        .filter_map(|r| {
            let v = r["value"][1].as_str()?.parse().ok()?;
            Some((r["metric"].clone(), v))
        })
        .collect())
}

/// The query returns exactly one sample, equal to `expected`.
async fn prom_is(query: &str, expected: f64) -> Result<(), String> {
    let result = prom_query(query).await?;
    let values: Vec<f64> = result.iter().map(|(_, v)| *v).collect();
    ensure(
        values == [expected],
        format!("{query} = {values:?}, want [{expected}]"),
    )
}

async fn alerts() -> Result<Vec<Value>, String> {
    let body: Value = client()
        .get(format!("{PROMETHEUS}/api/v1/alerts"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(body["data"]["alerts"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

async fn alert_active(name: &str) -> Result<bool, String> {
    Ok(alerts()
        .await?
        .iter()
        .any(|a| a["labels"]["alertname"] == name))
}

fn grafana(path: &str) -> reqwest::RequestBuilder {
    client()
        .get(format!("{GRAFANA}{path}"))
        .basic_auth("admin", Some("admin"))
}

async fn grafana_json(req: reqwest::RequestBuilder) -> Result<Value, String> {
    let res = req.send().await.map_err(|e| e.to_string())?;
    ensure(
        res.status().is_success(),
        format!("Grafana HTTP {}", res.status()),
    )?;
    res.json().await.map_err(|e| e.to_string())
}

/// Hosts with data for `expr`, queried through Grafana's Prometheus datasource
/// (the same path the dashboard panels use).
async fn grafana_hosts(expr: &str) -> Result<Vec<String>, String> {
    let req = client()
        .post(format!("{GRAFANA}/api/ds/query"))
        .basic_auth("admin", Some("admin"))
        .json(&json!({
            "from": "now-5m", "to": "now",
            "queries": [{"refId": "A", "datasource": {"uid": "prometheus"}, "expr": expr, "instant": true}]
        }));
    let body = grafana_json(req).await?;
    let mut hosts: Vec<String> = body["results"]["A"]["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| {
            f["data"]["values"][1]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        })
        .filter_map(|f| {
            f["schema"]["fields"][1]["labels"]["host"]
                .as_str()
                .map(String::from)
        })
        .collect();
    hosts.sort();
    hosts.dedup();
    Ok(hosts)
}

fn typesense(node: u8, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    client()
        .request(method, format!("{}{path}", typesense_url(node)))
        .header("X-TYPESENSE-API-KEY", API_KEY)
}

async fn search(node: u8, collection: &str, q: &str, cache: bool) -> Result<Value, String> {
    let path = format!(
        "/collections/{collection}/documents/search?q={q}&query_by=title{}",
        if cache { "&use_cache=true" } else { "" }
    );
    let res = typesense(node, reqwest::Method::GET, &path)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    ensure(
        res.status().is_success(),
        format!("search on node {node}: HTTP {}", res.status()),
    )?;
    res.json().await.map_err(|e| e.to_string())
}

/// A collection with a few documents, written through node 1.
async fn seed_collection(name: &str) {
    let _ = typesense(1, reqwest::Method::DELETE, &format!("/collections/{name}"))
        .send()
        .await;
    let created = typesense(1, reqwest::Method::POST, "/collections")
        .json(&json!({"name": name, "fields": [
            {"name": "title", "type": "string"}, {"name": "price", "type": "float"}
        ]}))
        .send()
        .await
        .unwrap();
    assert!(
        created.status().is_success(),
        "create collection: {}",
        created.status()
    );
    let docs = [
        json!({"id": "1", "title": "Blue running shoes", "price": 79.0}),
        json!({"id": "2", "title": "Red rain jacket", "price": 120.0}),
        json!({"id": "3", "title": "Green hiking boots", "price": 149.0}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    let imported = typesense(
        1,
        reqwest::Method::POST,
        &format!("/collections/{name}/documents/import"),
    )
    .body(docs)
    .send()
    .await
    .unwrap();
    assert!(
        imported.status().is_success(),
        "import: {}",
        imported.status()
    );
}

// ---------------------------------------------------------------------------
// Cluster state
// ---------------------------------------------------------------------------

/// Wait until all three nodes are healthy with exactly one leader, as seen by
/// both the exporters and Prometheus.
async fn wait_for_healthy_cluster() {
    eventually(
        "all exporters report a healthy node",
        Duration::from_secs(120),
        || async {
            for node in NODES {
                node_gauge_is(node, "typesense_health", 1.0).await?;
            }
            Ok(())
        },
    )
    .await;
    eventually(
        "Prometheus sees three healthy nodes",
        Duration::from_secs(60),
        || prom_is("count(typesense_health == 1)", 3.0),
    )
    .await;
    eventually(
        "Prometheus sees exactly one leader",
        Duration::from_secs(60),
        || prom_is("count(typesense_raft_state == 1)", 1.0),
    )
    .await;
}

/// Restores the cluster when dropped, even if the test panicked.
struct Restore(Vec<&'static [&'static str]>);

impl Drop for Restore {
    fn drop(&mut self) {
        for args in &self.0 {
            compose(args);
        }
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cluster_has_one_leader_and_two_followers() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;

    let mut roles = Vec::new();
    for node in NODES {
        let samples = scrape(node).await.unwrap();
        assert_eq!(
            gauge(&samples, "typesense_health"),
            Some(1.0),
            "node {node} health"
        );
        roles.push(gauge(&samples, "typesense_raft_state").unwrap());

        let host = format!("typesense-{node}");
        assert!(
            samples
                .iter()
                .all(|s| s.labels.get("host").is_none_or(|h| *h == host)),
            "exporter-{node} only reports its own node"
        );
        assert!(
            samples
                .iter()
                .any(|s| s.labels.get("key").map(String::as_str)
                    == Some("system_memory_total_bytes")),
            "exporter-{node} reports system metrics"
        );
    }
    roles.sort_by(f64::total_cmp);
    assert_eq!(roles, [RAFT_LEADER, RAFT_FOLLOWER, RAFT_FOLLOWER]);

    prom_is(r#"sum(up{job="typesense-exporter"})"#, 3.0)
        .await
        .unwrap();
    prom_is(
        r#"count(count by (host) (typesense_metrics{key="system_memory_total_bytes"}))"#,
        3.0,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn traffic_is_reported_by_the_node_that_served_it() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;
    seed_collection("e2e_products").await;

    eventually(
        "documents written to node 1 replicate to every node",
        Duration::from_secs(30),
        || async {
            for node in NODES {
                let found = search(node, "e2e_products", "boots", false).await?["found"].clone();
                ensure(found == 1, format!("node {node} found {found}"))?;
            }
            Ok(())
        },
    )
    .await;

    // Each node's /stats.json counts only the requests that node served.
    for node in NODES {
        eventually(
            &format!("exporter-{node} reports searches served by typesense-{node}"),
            Duration::from_secs(60),
            || async move {
                for q in ["shoes", "jacket", "boots"] {
                    search(node, "e2e_products", q, false).await?;
                }
                let samples = scrape(node).await?;
                ensure(
                    samples.iter().any(|s| {
                        s.name == "typesense_stats_requests_per_second_by_collection"
                            && s.labels["collection_name"] == "e2e_products"
                            && s.labels["action"] == "documents/search"
                            && s.labels["host"] == format!("typesense-{node}")
                    }),
                    "no per-collection search metric yet",
                )
            },
        )
        .await;
    }

    eventually(
        "cached searches produce a non-zero cache_hit_ratio",
        Duration::from_secs(60),
        || async {
            for _ in 0..5 {
                search(1, "e2e_products", "boots", true).await?;
            }
            let ratio = scrape(1)
                .await?
                .iter()
                .find(|s| s.name == "typesense_stats" && s.labels["key"] == "cache_hit_ratio")
                .map(|s| s.value);
            ensure(
                ratio.is_some_and(|r| r > 0.0),
                format!("cache_hit_ratio = {ratio:?}"),
            )
        },
    )
    .await;
}

#[tokio::test]
async fn prometheus_loads_the_alert_rules() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;

    let body: Value = client()
        .get(format!("{PROMETHEUS}/api/v1/rules"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut rules: Vec<&str> = body["data"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["rules"].as_array().unwrap())
        .filter_map(|r| r["name"].as_str())
        .collect();
    rules.sort();
    assert_eq!(
        rules,
        [
            "TypesenseExporterDown",
            "TypesenseNoLeader",
            "TypesenseUnhealthy"
        ]
    );
    // Alerts from an earlier scenario resolve on Prometheus' next rule
    // evaluation, a few seconds after the cluster recovers.
    eventually(
        "no alerts while the cluster is healthy",
        Duration::from_secs(30),
        || async {
            let active = alerts().await?;
            ensure(active.is_empty(), format!("still active: {active:?}"))
        },
    )
    .await;
}

#[tokio::test]
async fn grafana_dashboards_show_every_node() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;

    eventually(
        "Grafana datasource is healthy",
        Duration::from_secs(60),
        || async {
            let health = grafana_json(grafana("/api/datasources/uid/prometheus/health")).await?;
            ensure(health["status"] == "OK", format!("{health}"))
        },
    )
    .await;

    let found = grafana_json(grafana("/api/search?query=Typesense&type=dash-db"))
        .await
        .unwrap();
    let mut titles: Vec<&str> = found
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["title"].as_str())
        .collect();
    titles.sort();
    assert_eq!(
        titles,
        [
            "Typesense By Collection",
            "Typesense Metrics",
            "Typesense Stats"
        ]
    );

    let all = ["typesense-1", "typesense-2", "typesense-3"];
    for expr in [
        r#"typesense_metrics{key=~"system_cpu[0-9]+_active_percentage"}"#,
        "typesense_raft_state",
        "typesense_health",
    ] {
        eventually(
            &format!("Grafana returns {expr} for every node"),
            Duration::from_secs(30),
            || async {
                let hosts = grafana_hosts(expr).await?;
                ensure(hosts == all, format!("hosts with data: {hosts:?}"))
            },
        )
        .await;
    }
}

#[tokio::test]
async fn leader_failure_is_reported_and_a_new_leader_is_elected() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;

    let old = leader().await.expect("a leader");
    let service: &'static str = ["typesense-1", "typesense-2", "typesense-3"][usize::from(old - 1)];
    compose(&["stop", service]);
    let _restore = Restore(vec![match old {
        1 => &["start", "typesense-1"],
        2 => &["start", "typesense-2"],
        _ => &["start", "typesense-3"],
    }]);

    eventually(
        "stopped node reported unhealthy",
        Duration::from_secs(30),
        || node_gauge_is(old, "typesense_health", 0.0),
    )
    .await;
    eventually(
        "stopped node reports no raft role",
        Duration::from_secs(30),
        || node_gauge_is(old, "typesense_raft_state", 0.0),
    )
    .await;
    eventually("a new leader is elected", Duration::from_secs(60), || {
        prom_is("count(typesense_raft_state == 1)", 1.0)
    })
    .await;
    let new = leader().await.expect("a new leader");
    assert_ne!(new, old, "leadership moved off the stopped node");

    eventually(
        "TypesenseUnhealthy is pending",
        Duration::from_secs(60),
        || async {
            let pending = alerts().await?.iter().any(|a| {
                a["labels"]["alertname"] == "TypesenseUnhealthy"
                    && a["labels"]["host"] == service
                    && a["state"] == "pending"
            });
            ensure(pending, "not pending yet")
        },
    )
    .await;
    // The election leaves a few seconds with no leader, so TypesenseNoLeader
    // may briefly go pending; its `for: 1m` must keep it from firing, and it
    // must clear now that a leader exists.
    eventually(
        "TypesenseNoLeader clears once a leader exists",
        Duration::from_secs(30),
        || async {
            let no_leader: Vec<Value> = alerts()
                .await?
                .into_iter()
                .filter(|a| a["labels"]["alertname"] == "TypesenseNoLeader")
                .collect();
            assert!(
                no_leader.iter().all(|a| a["state"] != "firing"),
                "TypesenseNoLeader fired during a normal failover: {no_leader:?}"
            );
            ensure(no_leader.is_empty(), "still pending")
        },
    )
    .await;

    drop(_restore);
    wait_for_healthy_cluster().await;
}

#[tokio::test]
async fn hung_node_and_quorum_loss_raise_no_leader() {
    let _stack = stack().await;
    wait_for_healthy_cluster().await;

    // Stop one follower and freeze the other: the leader loses quorum and the
    // frozen node's exporter must answer within its timeout.
    let leader = leader().await.expect("a leader");
    let followers: Vec<u8> = NODES.into_iter().filter(|n| *n != leader).collect();
    let (stopped, hung) = (followers[0], followers[1]);
    let name = |n: u8| -> &'static str {
        ["typesense-1", "typesense-2", "typesense-3"][usize::from(n - 1)]
    };
    compose(&["stop", name(stopped)]);
    compose(&["pause", name(hung)]);
    let restore: Vec<&'static [&'static str]> = vec![
        match hung {
            1 => &["unpause", "typesense-1"],
            2 => &["unpause", "typesense-2"],
            _ => &["unpause", "typesense-3"],
        },
        match stopped {
            1 => &["start", "typesense-1"],
            2 => &["start", "typesense-2"],
            _ => &["start", "typesense-3"],
        },
    ];
    let _restore = Restore(restore);

    let started = Instant::now();
    let samples = scrape(hung)
        .await
        .expect("exporter answers for a hung node");
    let took = started.elapsed();
    assert!(
        took < Duration::from_secs(4),
        "scrape of hung node took {took:?} (scrape timeout 4s)"
    );
    assert_eq!(gauge(&samples, "typesense_health"), Some(0.0));

    eventually(
        "all exporters still scraped",
        Duration::from_secs(30),
        || prom_is(r#"sum(up{job="typesense-exporter"})"#, 3.0),
    )
    .await;
    eventually("no node reports leader", Duration::from_secs(60), || {
        prom_is("max(typesense_raft_state == bool 1)", 0.0)
    })
    .await;
    eventually(
        "TypesenseNoLeader is pending",
        Duration::from_secs(60),
        || async { ensure(alert_active("TypesenseNoLeader").await?, "not active yet") },
    )
    .await;

    drop(_restore);
    wait_for_healthy_cluster().await;
}

// ---------------------------------------------------------------------------
// Harness self-tests (no Docker needed for the parser, but they live in this
// target so they run with the suite)
// ---------------------------------------------------------------------------

#[test]
fn parses_exporter_output() {
    let samples = parse_samples(
        "# HELP x y\n\
         typesense_health{host=\"typesense-1\",port=\"8108\"} 1\n\
         typesense_stats_latency_ms{host=\"h\",key=\"GET /collections/a/documents/search\",port=\"1\"} 2.5\n",
    );

    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].labels["host"], "typesense-1");
    assert_eq!(
        samples[1].labels["key"],
        "GET /collections/a/documents/search"
    );
    assert_eq!(samples[1].value, 2.5);
}
