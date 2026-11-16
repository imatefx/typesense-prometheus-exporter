//! The exporter's own health, served at `/exporter/metrics`.
//!
//! Unlike the per-scrape registry that renders a Typesense node, this one
//! lives as long as the process so counters accumulate across scrapes. No
//! series carries `host`, `port` or a caller-supplied value: every label is
//! one of a fixed set, a Typesense path, or a cluster name from the config
//! file.

use prometheus::{
    Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts,
    Registry, TextEncoder,
};

/// The Typesense paths a scrape reads; the `endpoint` label values.
pub const ENDPOINTS: [&str; 6] = [
    "/metrics.json",
    "/stats.json",
    "/health",
    "/debug",
    "/status",
    "/collections",
];

const REQUEST_BUCKETS: [f64; 12] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
const SLOT_WAIT_BUCKETS: [f64; 9] = [0.001, 0.01, 0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];

/// Which handler served a scrape.
#[derive(Clone, Copy, Debug)]
pub enum Handler {
    Metrics,
    Probe,
}

/// How a scrape ended, by HTTP status: 200, 403, 400, 404.
#[derive(Clone, Copy, Debug)]
pub enum ScrapeResult {
    Ok,
    Refused,
    Invalid,
    NotFound,
}

/// How one Typesense request ended.
#[derive(Clone, Copy, Debug)]
pub enum RequestResult {
    Ok,
    Error,
    Timeout,
}

impl RequestResult {
    pub fn of<T>(result: &reqwest::Result<T>) -> Self {
        match result {
            Ok(_) => Self::Ok,
            Err(err) if err.is_timeout() => Self::Timeout,
            Err(_) => Self::Error,
        }
    }
}

pub struct ExporterMetrics {
    registry: Registry,
    scrapes: IntCounterVec,
    typesense_requests: IntCounterVec,
    typesense_duration: HistogramVec,
    in_flight: IntGauge,
    slot_wait: Histogram,
    key_reloads: IntCounterVec,
    key_failing: IntGaugeVec,
}

/// Counts the reloads of one key file; handed to its `KeySource`.
#[derive(Clone)]
pub struct ReloadObserver {
    ok: IntCounter,
    failed: IntCounter,
    failing: IntGauge,
}

impl ReloadObserver {
    pub fn reloaded(&self) {
        self.ok.inc();
        self.failing.set(0);
    }

    pub fn failed(&self) {
        self.failed.inc();
        self.failing.set(1);
    }
}

/// Decrements `typesense_exporter_probes_in_flight` when dropped.
pub struct InFlight(IntGauge);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.dec();
    }
}

impl ExporterMetrics {
    pub fn new() -> Self {
        let registry = Registry::new();
        let register = |c: Box<dyn prometheus::core::Collector>| {
            registry
                .register(c)
                .expect("exporter metric registers once");
        };
        let counter = |name: &str, help: &str, labels: &[&str]| {
            let c = IntCounterVec::new(Opts::new(name, help), labels).expect("valid metric");
            register(Box::new(c.clone()));
            c
        };

        let build = IntGaugeVec::new(
            Opts::new(
                "typesense_exporter_build_info",
                "The exporter's version; always 1",
            ),
            &["version"],
        )
        .expect("valid metric");
        register(Box::new(build.clone()));
        build.with_label_values(&[env!("CARGO_PKG_VERSION")]).set(1);

        // Start every outcome a handler can produce at 0, so rate() and
        // increase() see the first one.
        let scrapes = counter(
            "typesense_exporter_scrapes_total",
            "Scrapes of /metrics and /probe, by handler and outcome",
            &["handler", "result"],
        );
        for (handler, result) in [
            ("metrics", "ok"),
            ("metrics", "not_found"),
            ("probe", "ok"),
            ("probe", "refused"),
            ("probe", "invalid"),
        ] {
            scrapes.with_label_values(&[handler, result]);
        }

        let typesense_requests = counter(
            "typesense_exporter_typesense_requests_total",
            "Requests to Typesense, by endpoint and outcome, over all targets",
            &["endpoint", "result"],
        );
        let typesense_duration = HistogramVec::new(
            HistogramOpts::new(
                "typesense_exporter_typesense_request_duration_seconds",
                "Time of requests to Typesense, by endpoint, over all targets",
            )
            .buckets(REQUEST_BUCKETS.to_vec()),
            &["endpoint"],
        )
        .expect("valid metric");
        register(Box::new(typesense_duration.clone()));
        for endpoint in ENDPOINTS {
            for result in ["ok", "error", "timeout"] {
                typesense_requests.with_label_values(&[endpoint, result]);
            }
            typesense_duration.with_label_values(&[endpoint]);
        }

        let in_flight = IntGauge::new(
            "typesense_exporter_probes_in_flight",
            "/probe scrapes running now",
        )
        .expect("valid metric");
        register(Box::new(in_flight.clone()));
        let slot_wait = Histogram::with_opts(
            HistogramOpts::new(
                "typesense_exporter_probe_slot_wait_seconds",
                "Time /probe scrapes waited for a --probe-max-concurrency slot",
            )
            .buckets(SLOT_WAIT_BUCKETS.to_vec()),
        )
        .expect("valid metric");
        register(Box::new(slot_wait.clone()));

        let key_reloads = counter(
            "typesense_exporter_key_reloads_total",
            "Reloads of an API key file, by key (default or cluster name) and outcome",
            &["key", "result"],
        );
        let key_failing = IntGaugeVec::new(
            Opts::new(
                "typesense_exporter_key_reload_failing",
                "1 while the latest reload of the key's file failed, else 0",
            ),
            &["key"],
        )
        .expect("valid metric");
        register(Box::new(key_failing.clone()));

        Self {
            registry,
            scrapes,
            typesense_requests,
            typesense_duration,
            in_flight,
            slot_wait,
            key_reloads,
            key_failing,
        }
    }

    pub fn record_scrape(&self, handler: Handler, result: ScrapeResult) {
        let handler = match handler {
            Handler::Metrics => "metrics",
            Handler::Probe => "probe",
        };
        let result = match result {
            ScrapeResult::Ok => "ok",
            ScrapeResult::Refused => "refused",
            ScrapeResult::Invalid => "invalid",
            ScrapeResult::NotFound => "not_found",
        };
        self.scrapes.with_label_values(&[handler, result]).inc();
    }

    /// `endpoint` must be one of [`ENDPOINTS`].
    pub fn observe_request(&self, endpoint: &'static str, result: RequestResult, seconds: f64) {
        let result = match result {
            RequestResult::Ok => "ok",
            RequestResult::Error => "error",
            RequestResult::Timeout => "timeout",
        };
        self.typesense_requests
            .with_label_values(&[endpoint, result])
            .inc();
        self.typesense_duration
            .with_label_values(&[endpoint])
            .observe(seconds);
    }

    pub fn probe_started(&self) -> InFlight {
        self.in_flight.inc();
        InFlight(self.in_flight.clone())
    }

    pub fn observe_slot_wait(&self, seconds: f64) {
        self.slot_wait.observe(seconds);
    }

    /// The counters for one file-backed key; its series start at 0.
    pub fn reload_observer(&self, key: &str) -> ReloadObserver {
        let failing = self.key_failing.with_label_values(&[key]);
        failing.set(0);
        ReloadObserver {
            ok: self.key_reloads.with_label_values(&[key, "ok"]),
            failed: self.key_reloads.with_label_values(&[key, "failed"]),
            failing,
        }
    }

    pub fn render(&self) -> String {
        TextEncoder::new()
            .encode_to_string(&self.registry.gather())
            .unwrap_or_default()
    }
}

impl Default for ExporterMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value of the line for exactly `series`.
    fn value(text: &str, series: &str) -> Option<f64> {
        text.lines()
            .find_map(|l| l.strip_prefix(series)?.strip_prefix(' ')?.parse().ok())
    }

    #[test]
    fn build_info_carries_the_version() {
        let text = ExporterMetrics::new().render();
        let series = format!(
            "typesense_exporter_build_info{{version=\"{}\"}}",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(value(&text, &series), Some(1.0));
    }

    #[test]
    fn scrapes_count_by_handler_and_result() {
        let m = ExporterMetrics::new();
        m.record_scrape(Handler::Probe, ScrapeResult::Refused);
        m.record_scrape(Handler::Probe, ScrapeResult::Refused);
        m.record_scrape(Handler::Probe, ScrapeResult::Invalid);
        m.record_scrape(Handler::Metrics, ScrapeResult::Ok);
        m.record_scrape(Handler::Metrics, ScrapeResult::NotFound);
        let t = m.render();
        let s =
            |h, r| format!("typesense_exporter_scrapes_total{{handler=\"{h}\",result=\"{r}\"}}");
        assert_eq!(value(&t, &s("probe", "refused")), Some(2.0));
        assert_eq!(value(&t, &s("probe", "invalid")), Some(1.0));
        assert_eq!(value(&t, &s("probe", "ok")), Some(0.0));
        assert_eq!(value(&t, &s("metrics", "ok")), Some(1.0));
        assert_eq!(value(&t, &s("metrics", "not_found")), Some(1.0));
    }

    #[test]
    fn no_series_carries_a_target_or_node_label() {
        let m = ExporterMetrics::new();
        m.record_scrape(Handler::Probe, ScrapeResult::Refused);
        let t = m.render();
        for label in ["target=", "host=", "port=", "instance="] {
            assert!(!t.contains(label), "unexpected {label} in:\n{t}");
        }
    }

    #[test]
    fn requests_count_and_observe_a_duration_per_endpoint() {
        let m = ExporterMetrics::new();
        m.observe_request("/health", RequestResult::Ok, 0.02);
        m.observe_request("/health", RequestResult::Timeout, 2.0);
        m.observe_request("/stats.json", RequestResult::Error, 0.001);
        let t = m.render();
        let r = |e, r| {
            format!(
                "typesense_exporter_typesense_requests_total{{endpoint=\"{e}\",result=\"{r}\"}}"
            )
        };
        assert_eq!(value(&t, &r("/health", "ok")), Some(1.0));
        assert_eq!(value(&t, &r("/health", "timeout")), Some(1.0));
        assert_eq!(value(&t, &r("/stats.json", "error")), Some(1.0));
        assert_eq!(value(&t, &r("/debug", "ok")), Some(0.0));
        let d = "typesense_exporter_typesense_request_duration_seconds";
        assert_eq!(
            value(&t, &format!("{d}_count{{endpoint=\"/health\"}}")),
            Some(2.0)
        );
        assert_eq!(
            value(
                &t,
                &format!("{d}_bucket{{endpoint=\"/health\",le=\"0.025\"}}")
            ),
            Some(1.0)
        );
        assert_eq!(
            value(&t, &format!("{d}_sum{{endpoint=\"/health\"}}")),
            Some(2.02)
        );
    }

    #[test]
    fn probes_in_flight_goes_up_and_back_and_waits_are_observed() {
        let m = ExporterMetrics::new();
        let in_flight = "typesense_exporter_probes_in_flight";
        let a = m.probe_started();
        let b = m.probe_started();
        assert_eq!(value(&m.render(), in_flight), Some(2.0));
        drop(a);
        drop(b);
        assert_eq!(value(&m.render(), in_flight), Some(0.0));
        m.observe_slot_wait(0.3);
        let t = m.render();
        let w = "typesense_exporter_probe_slot_wait_seconds";
        assert_eq!(value(&t, &format!("{w}_count")), Some(1.0));
        assert_eq!(value(&t, &format!("{w}_bucket{{le=\"0.1\"}}")), Some(0.0));
        assert_eq!(value(&t, &format!("{w}_bucket{{le=\"0.5\"}}")), Some(1.0));
    }

    #[test]
    fn key_reload_gauge_flips_on_failure_and_back() {
        let m = ExporterMetrics::new();
        let o = m.reload_observer("default");
        let failing = "typesense_exporter_key_reload_failing{key=\"default\"}";
        let n =
            |r| format!("typesense_exporter_key_reloads_total{{key=\"default\",result=\"{r}\"}}");
        let t = m.render();
        assert_eq!(value(&t, failing), Some(0.0));
        assert_eq!(value(&t, &n("ok")), Some(0.0));
        o.failed();
        o.failed();
        let t = m.render();
        assert_eq!(value(&t, failing), Some(1.0));
        assert_eq!(value(&t, &n("failed")), Some(2.0));
        o.reloaded();
        let t = m.render();
        assert_eq!(value(&t, failing), Some(0.0));
        assert_eq!(value(&t, &n("ok")), Some(1.0));
        assert_eq!(value(&t, &n("failed")), Some(2.0));
    }

    #[test]
    fn a_key_that_is_not_observed_has_no_series() {
        let t = ExporterMetrics::new().render();
        assert!(!t.contains("key_reload"), "{t}");
    }
}
