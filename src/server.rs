use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Router,
};
use serde::Deserialize;
use tokio::signal;

use crate::{
    cli::{CliArgs, MetricNames},
    exporter_metrics::{ExporterMetrics, Handler, RequestResult, ScrapeResult},
    key::KeySource,
    prometheus_exp::{self, Scrape},
    target::{is_loopback, normalize_host, AllowList, Target},
    typesense::TypesenseClient,
};

/// The node `--typesense-host` names, served at `/metrics`.
struct Node {
    base_url: String,
    key: KeySource,
    host: String,
    port: u16,
}

struct AppState {
    /// Shared by every target so connections are pooled.
    http: reqwest::Client,
    /// For `--typesense-host` and `--probe-allowed-targets`; none when only
    /// the config file's clusters are used.
    api_key: Option<KeySource>,
    node: Option<Node>,
    allowed: AllowList,
    /// Clusters with their own key; checked before `allowed`.
    clusters: crate::probe_config::ProbeConfig,
    default_scheme: String,
    default_port: u16,
    metric_names: MetricNames,
    /// `--typesense-timeout`, for log messages.
    timeout: u64,
    /// Probe targets already warned about for a plaintext API key.
    plaintext_warned: Mutex<HashSet<(String, u16)>>,
    /// Bounds concurrent /probe scrapes (`--probe-max-concurrency`).
    probe_slots: tokio::sync::Semaphore,
    /// The exporter's own metrics, served at `/exporter/metrics`.
    exporter: ExporterMetrics,
}

/// Build the exporter's HTTP router for the node(s) described by `args`.
/// Fails if a key can't be read, the probe config or an allowed target is
/// invalid or overlaps another, or the HTTP client can't be built.
pub fn router(args: &CliArgs) -> std::io::Result<Router> {
    if args
        .typesense_host
        .as_deref()
        .is_some_and(|h| h.trim().is_empty())
    {
        return Err(std::io::Error::other("--typesense-host must not be empty"));
    }
    let api_key = args.has_key().then(|| args.key_source()).transpose()?;
    // Never follow redirects: reqwest would resend the API key header to
    // wherever a node (or something impersonating it) redirects.
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    if args.typesense_timeout > 0 {
        builder = builder.timeout(Duration::from_secs(args.typesense_timeout));
    }
    let http = builder.build().map_err(std::io::Error::other)?;

    let mut allowed = AllowList::parse(&args.probe_allowed_targets, args.typesense_port)
        .map_err(std::io::Error::other)?;
    let clusters = args.clusters.clone();
    if args.typesense_host.is_none() && allowed.is_empty() && clusters.clusters.is_empty() {
        // clap accepts an empty TYPESENSE_PROBE_ALLOWED_TARGETS as "set".
        return Err(std::io::Error::other(
            "set --typesense-host, a non-empty --probe-allowed-targets or [clusters] in the --config file",
        ));
    }
    if api_key.is_none() && (args.typesense_host.is_some() || !allowed.is_empty()) {
        return Err(std::io::Error::other(
            "--typesense-api-key or --typesense-api-key-file is required with --typesense-host or --probe-allowed-targets",
        ));
    }
    // A key must only ever reach its own cluster, so a node can have one home.
    {
        for cluster in &clusters.clusters {
            for (host, port) in &cluster.nodes {
                let elsewhere = if allowed.lists(host, *port) {
                    Some("--probe-allowed-targets")
                } else if args
                    .typesense_host
                    .as_ref()
                    .is_some_and(|h| normalize_host(h) == *host && args.typesense_port == *port)
                {
                    Some("--typesense-host")
                } else {
                    None
                };
                if let Some(elsewhere) = elsewhere {
                    return Err(std::io::Error::other(format!(
                        "{host}:{port} is in cluster {:?} of the config file and also in {elsewhere}; a node can have only one API key",
                        cluster.name
                    )));
                }
            }
        }
    }
    let plaintext: Vec<&str> = clusters
        .clusters
        .iter()
        .filter(|c| c.protocol == "http" && c.nodes.iter().any(|(h, _)| !is_loopback(h)))
        .map(|c| c.name.as_str())
        .collect();
    if !plaintext.is_empty() {
        tracing::warn!(
            "/probe sends the API key of cluster(s) {} over unencrypted http to non-local hosts; set protocol = \"https\" in the probe config",
            plaintext.join(", ")
        );
    }
    let node = args
        .typesense_host
        .as_ref()
        .zip(api_key.as_ref())
        .map(|(host, key)| {
            allowed.allow(host, args.typesense_port);
            Node {
                base_url: format!(
                    "{}://{}:{}",
                    args.typesense_protocol, host, args.typesense_port
                ),
                key: key.clone(),
                host: host.clone(),
                port: args.typesense_port,
            }
        });

    let exporter = ExporterMetrics::new();
    if let Some(key) = &api_key {
        key.observe_reloads("default", &exporter);
    }
    for cluster in &clusters.clusters {
        cluster
            .key_source()
            .observe_reloads(&cluster.name, &exporter);
    }

    let state = Arc::new(AppState {
        http,
        api_key,
        node,
        allowed,
        clusters,
        default_scheme: args.typesense_protocol.clone(),
        default_port: args.typesense_port,
        metric_names: args.metric_names,
        timeout: args.typesense_timeout,
        plaintext_warned: Mutex::default(),
        probe_slots: tokio::sync::Semaphore::new(args.probe_max_concurrency as usize),
        exporter,
    });

    Ok(Router::new()
        .route("/", get(root))
        .route("/metrics", get(metrics_handler))
        .route("/probe", get(probe_handler))
        .route("/exporter/metrics", get(exporter_metrics_handler))
        .with_state(state))
}

/// Bind to the configured address and serve until SIGINT/SIGTERM.
pub async fn run(args: CliArgs) -> std::io::Result<()> {
    let bind_address = format!("{}:{}", args.exporter_bind_address, args.exporter_bind_port);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    serve(listener, args).await
}

/// Serve on an already-bound listener until SIGINT/SIGTERM.
pub async fn serve(listener: tokio::net::TcpListener, args: CliArgs) -> std::io::Result<()> {
    let app = router(&args)?;
    log_startup(&args, listener.local_addr()?);
    if let (true, Some(host)) = (args.sends_key_in_plaintext(), &args.typesense_host) {
        tracing::warn!(
            "sending the Typesense API key over unencrypted http to {host}; use --typesense-protocol https"
        );
    }
    if args.probe_sends_key_in_plaintext() {
        tracing::warn!(
            "/probe may send the Typesense API key over unencrypted http to non-local hosts in --probe-allowed-targets; use --typesense-protocol https"
        );
    }
    if args.probe_allowed_targets.iter().any(|t| t.trim() == "*") {
        tracing::warn!(
            "--probe-allowed-targets=* lets anyone who can reach this exporter make it send the Typesense API key to any host; list the nodes instead"
        );
    }
    if args.metric_names.legacy() {
        tracing::warn!(
            "--metric-names={} exports the deprecated 0.4 metric names, which will be removed in 2.0.0; see docs/migrating-to-1.0.md",
            if args.metric_names.v1() { "both" } else { "legacy" }
        );
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// What the exporter was started with, for the first lines of the log. Never
/// includes an API key.
fn log_startup(args: &CliArgs, bind: std::net::SocketAddr) {
    fn name<T: clap::ValueEnum>(value: T) -> String {
        value
            .to_possible_value()
            .map_or_else(String::new, |v| v.get_name().to_string())
    }
    let timeout = match args.typesense_timeout {
        0 => "disabled".to_string(),
        secs => format!("{secs}s"),
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        bind = %bind,
        metric_names = %name(args.metric_names),
        timeout = %timeout,
        "exporter listening on {bind}"
    );
    if let Some(host) = &args.typesense_host {
        tracing::info!(
            "/metrics scrapes {}://{}:{}",
            args.typesense_protocol,
            host,
            args.typesense_port
        );
    }
    let allowed: Vec<&str> = args
        .probe_allowed_targets
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if !allowed.is_empty() {
        tracing::info!("/probe allows {}", allowed.join(", "));
    }
    for cluster in &args.clusters.clusters {
        tracing::info!(
            "/probe cluster {}: {} node(s) over {}",
            cluster.name,
            cluster.nodes.len(),
            cluster.protocol
        );
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn root() -> &'static str {
    "Typesense Prometheus Exporter. Metrics are served at /metrics (the configured node) and /probe?target=host[:port] (any allowed node); the exporter's own metrics are at /exporter/metrics\n"
}

/// `/metrics`: scrape the node set with `--typesense-host`.
async fn metrics_handler(State(state): State<Arc<AppState>>) -> (StatusCode, String) {
    match &state.node {
        Some(node) => {
            let body = scrape(
                &TypesenseClient::new(
                    state.http.clone(),
                    node.base_url.clone(),
                    node.key.current(),
                ),
                &node.host,
                node.port,
                state.metric_names,
                state.timeout,
                &state.exporter,
            )
            .await;
            state
                .exporter
                .record_scrape(Handler::Metrics, ScrapeResult::Ok);
            (StatusCode::OK, body)
        }
        None => {
            state
                .exporter
                .record_scrape(Handler::Metrics, ScrapeResult::NotFound);
            (
                StatusCode::NOT_FOUND,
                "No --typesense-host is configured; scrape /probe?target=host[:port] instead\n"
                    .to_string(),
            )
        }
    }
}

/// `/exporter/metrics`: the exporter's own health, never mixed into a node's
/// output.
async fn exporter_metrics_handler(State(state): State<Arc<AppState>>) -> String {
    state.exporter.render()
}

#[derive(Deserialize)]
struct ProbeParams {
    target: Option<String>,
}

/// `/probe?target=...`: scrape one allowed node, with the same output as
/// `/metrics`. The exporter sends its API key to the target, so anything not in
/// the allow list is refused before any request is made.
async fn probe_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ProbeParams>,
) -> (StatusCode, String) {
    let raw = params.target.unwrap_or_default();
    if raw.trim().is_empty() {
        state
            .exporter
            .record_scrape(Handler::Probe, ScrapeResult::Invalid);
        return (
            StatusCode::BAD_REQUEST,
            "missing target parameter, e.g. /probe?target=host:8108\n".to_string(),
        );
    }
    let mut target = match Target::parse(&raw, &state.default_scheme, state.default_port) {
        Ok(target) => target,
        Err(reason) => {
            state
                .exporter
                .record_scrape(Handler::Probe, ScrapeResult::Invalid);
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid target: {reason}\n"),
            );
        }
    };
    // A node's cluster decides its key and default protocol; otherwise only
    // the allow list (and the default key) applies. Nothing is sent before this.
    let cluster = state.clusters.find(&target);
    let (api_key, protocol) = match (cluster, &state.api_key) {
        (Some(cluster), _) => {
            // Re-parse so a bare host takes the cluster's protocol.
            if let Ok(t) = Target::parse(&raw, &cluster.protocol, state.default_port) {
                target = t;
            }
            (cluster.key_source().clone(), cluster.protocol.as_str())
        }
        (None, Some(key)) if state.allowed.allows(&target) => {
            (key.clone(), state.default_scheme.as_str())
        }
        _ => {
            // debug, not warn: refusals are triggered by unauthenticated callers.
            tracing::debug!(
                "refused probe of {}:{}: not in --probe-allowed-targets or the config file clusters",
                target.host,
                target.port
            );
            state
                .exporter
                .record_scrape(Handler::Probe, ScrapeResult::Refused);
            return (
                StatusCode::FORBIDDEN,
                format!(
                    "target {}:{} is not allowed; see --probe-allowed-targets and the config file's clusters\n",
                    target.host, target.port
                ),
            );
        }
    };

    if target.scheme == "http" && protocol.eq_ignore_ascii_case("https") {
        state
            .exporter
            .record_scrape(Handler::Probe, ScrapeResult::Refused);
        return (
            StatusCode::FORBIDDEN,
            "target scheme must be https, like the protocol of its cluster or --typesense-protocol; http would send the API key unencrypted\n"
                .to_string(),
        );
    }
    // With `*` the startup warning covers it, except for cluster nodes, which
    // `*` doesn't cover; don't grow the set per caller.
    if target.scheme == "http"
        && !is_loopback(&target.host)
        && (cluster.is_some() || !state.allowed.allows_any())
        && state
            .plaintext_warned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((target.host.clone(), target.port))
    {
        tracing::warn!(
            "sending the Typesense API key over unencrypted http to {}:{}; probe https:// targets or use protocol https",
            target.host,
            target.port
        );
    }

    let waiting = Instant::now();
    let Ok(_slot) = state.probe_slots.acquire().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "exporter shutting down\n".to_string(),
        );
    };
    state
        .exporter
        .observe_slot_wait(waiting.elapsed().as_secs_f64());
    let _in_flight = state.exporter.probe_started();
    let client = TypesenseClient::new(state.http.clone(), target.base_url(), api_key.current());
    let body = scrape(
        &client,
        &target.host,
        target.port,
        state.metric_names,
        state.timeout,
        &state.exporter,
    )
    .await;
    state
        .exporter
        .record_scrape(Handler::Probe, ScrapeResult::Ok);
    (StatusCode::OK, body)
}

/// Scrape one Typesense node and render metrics. Each endpoint is fetched
/// concurrently; a failed endpoint is logged, its metrics are omitted and its
/// `typesense_scrape_success` is 0, while `typesense_health` and
/// `typesense_raft_state` drop to 0.
async fn scrape(
    client: &TypesenseClient,
    host: &str,
    port: u16,
    metric_names: MetricNames,
    timeout: u64,
    exporter: &ExporterMetrics,
) -> String {
    let started = Instant::now();
    let (metrics, stats, health, debug, status, collections) = tokio::join!(
        timed(exporter, "/metrics.json", client.metrics()),
        timed(exporter, "/stats.json", client.stats()),
        timed(exporter, "/health", client.health()),
        timed(exporter, "/debug", client.debug()),
        timed(exporter, "/status", client.status()),
        timed(exporter, "/collections", client.collections()),
    );
    let scrape_duration = started.elapsed().as_secs_f64();

    let mut endpoints = Vec::with_capacity(6);
    let mut ok = |endpoint: &'static str, succeeded: bool| endpoints.push((endpoint, succeeded));
    let metrics = log_failure(host, port, "/metrics.json", timeout, metrics);
    ok("/metrics.json", metrics.is_some());
    let stats = log_failure(host, port, "/stats.json", timeout, stats);
    ok("/stats.json", stats.is_some());
    let health = log_failure(host, port, "/health", timeout, health);
    ok("/health", health.is_some());
    let debug = log_failure(host, port, "/debug", timeout, debug);
    ok("/debug", debug.is_some());
    let status = log_failure(host, port, "/status", timeout, status);
    ok("/status", status.is_some());
    let collections = log_failure(host, port, "/collections", timeout, collections);
    ok("/collections", collections.is_some());

    tracing::debug!(
        target = %format!("{host}:{port}"),
        failed = endpoints.iter().filter(|(_, ok)| !ok).count(),
        duration_ms = (scrape_duration * 1000.0) as u64,
        "scrape finished"
    );
    let scrape = Scrape {
        metrics,
        stats,
        healthy: health.is_some_and(|h| h.ok),
        raft_state: debug.as_ref().map_or(0, |d| d.state),
        version: debug.and_then(|d| d.version),
        status,
        collections,
        endpoints,
        scrape_duration: Some(scrape_duration),
        metric_names,
    };

    prometheus_exp::generate_metrics(&scrape, host, port)
}

/// Run one Typesense request and record its outcome and duration.
async fn timed<T>(
    exporter: &ExporterMetrics,
    endpoint: &'static str,
    request: impl std::future::Future<Output = reqwest::Result<T>>,
) -> reqwest::Result<T> {
    let started = Instant::now();
    let result = request.await;
    exporter.observe_request(
        endpoint,
        RequestResult::of(&result),
        started.elapsed().as_secs_f64(),
    );
    result
}

fn log_failure<T>(
    host: &str,
    port: u16,
    endpoint: &str,
    timeout: u64,
    result: reqwest::Result<T>,
) -> Option<T> {
    result
        .inspect_err(|err| {
            let is_timeout = err.is_timeout();
            let err = crate::typesense::describe(err);
            if is_timeout {
                tracing::warn!(
                    "typesense {host}:{port} {endpoint} timed out after {timeout}s: {err}; raise TYPESENSE_TIMEOUT (--typesense-timeout) if Typesense is only slow, and set TYPESENSE_EXPORTER_LOG_LEVEL=debug to see each request"
                );
            } else {
                tracing::warn!("typesense {host}:{port} {endpoint} failed: {err}");
            }
        })
        .ok()
}
