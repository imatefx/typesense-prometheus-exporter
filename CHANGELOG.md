# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.4.0] - 2026-11-16

### Added
- **`GET /exporter/metrics`: the exporter's own health**, in Prometheus text format, on its own endpoint so it never mixes into a node's output and is the same with `/metrics` and `/probe` (like the `/metrics` of `blackbox_exporter` itself). Scrape it as a separate job (`metrics_path: /exporter/metrics`); the Docker Compose stack's `prometheus.yml` has one. The output of `/metrics` and `/probe` is unchanged. The series live as long as the process, carry no `host` or `port`, and never take a label value from a request: a refused or malformed `/probe` target is only counted. See [Exporter metrics](docs/metrics.md#exporter-metrics).
  - `typesense_exporter_build_info{version}`
  - `typesense_exporter_scrapes_total{handler, result}`: `/metrics` and `/probe` scrapes by `ok`, `refused` (403), `invalid` (400), `not_found` (`/metrics` 404), `unavailable` (`/probe` 503 during shutdown) and `cancelled` (the caller hung up first, e.g. Prometheus at `scrape_timeout`).
  - `typesense_exporter_typesense_requests_total{endpoint, result}` and `typesense_exporter_typesense_request_duration_seconds{endpoint}`: the six Typesense requests by `ok`, `error`, `timeout` and `cancelled`, over all targets.
  - `typesense_exporter_probes_in_flight`, `typesense_exporter_probes_waiting` and `typesense_exporter_probe_slot_wait_seconds`: running and queued `/probe` scrapes, and the time waiting for a `--probe-max-concurrency` slot. A probe whose caller hangs up while queued is still observed, so saturation shows even when Prometheus gives up first.
  - `typesense_exporter_key_reloads_total{key, result}` and `typesense_exporter_key_reload_failing{key}`: reloads of an API key file (`default` or the cluster's name), and whether the latest one failed. Only for file-backed keys. Key files are checked in the background every 5s as well as on use, so a broken rotated key is flagged even for a cluster nobody probes; `ok` counts any successful re-read after the file changed (a `touch` too), not only a new key.
- **Four alert rules** in a new `typesense-exporter` group of `deploy/prometheus/alerts.yml`, with `promtool` unit tests in `deploy/prometheus/alerts.test.yml`: `TypesenseExporterKeyReloadFailing` (a rotated key file can't be read, 5m), `TypesenseExporterProbeRefused` (targets refused (403) in the last 10m, fires at once; malformed 400s are counted but don't alert), `TypesenseExporterProbesSaturated` (p95 probe slot wait above 1s for 10m) and `TypesenseExporterSelfMetricsDown` (the `typesense-exporter-self` job is down for 5m, which would silence the other three). The existing alerts are unchanged.
- The Kubernetes examples scrape `/exporter/metrics` too: a second endpoint in the single-node `ServiceMonitor` and a new `servicemonitor.yaml` in cluster-probe, both relabelled to the job `typesense-exporter-self` so they don't collide with the node's `up` and `scrape_*` series or match `TypesenseExporterDown`; the plain Prometheus job is in the cluster-probe README.

### CI
- **End-to-end tests run against two Typesense versions**, 30.1 and 27.1 (the oldest the compose cluster was found to work with), as a matrix with `fail-fast: false`. `docker-compose.yml` takes the image tag from `TYPESENSE_VERSION` (default 30.1), and the e2e tests read the same variable.
- The `rules` job also runs `promtool test rules alerts.test.yml`.

## [1.3.0] - 2026-11-16

### Added
- **Log level and format** (#8). `--log-level` / `TYPESENSE_EXPORTER_LOG_LEVEL` / `log_level` takes `error`, `warn`, `info` (default), `debug` or `trace`; `--log-format` / `TYPESENSE_EXPORTER_LOG_FORMAT` / `log_format` takes `text` (default) or `json`. `RUST_LOG`, when set, takes precedence over the level and accepts full filter directives such as `typesense_prometheus_exporter=debug,reqwest=info`. Logging now starts after the options are loaded, so the level can come from the config file. See [Logging](docs/configuration.md#logging).
- Text logs are coloured only on a terminal and never with `NO_COLOR` set, so `docker logs` and log collectors no longer get ANSI escape codes.
- **Request logs.** At `debug`, one line per Typesense request with the endpoint, the target `host:port`, the HTTP status and the time in milliseconds (the error when there was no response), plus a summary per scrape. At `info`, startup now logs the version, bind address, metric names, timeout and what is served (the `/metrics` node, the `/probe` allow list, each cluster with its node count). The API key is never logged at any level; a test captures the logs at `trace` to check it.
- A request that times out is logged at `warn` as `timed out after Ns`, naming `TYPESENSE_TIMEOUT`. Failed requests now include the underlying cause (for example `connection refused`).

### CI
- Releases check that the tag matches the `Cargo.toml` version (#5). In both `rust.yml` and `docker.yml`, the test job that runs before publishing fails with a clear message when `GITHUB_REF_NAME` is not `v` plus the package version, so a binary or image can no longer be released with a stale version. `cargo test --locked` already fails if `Cargo.lock` is out of date.

## [1.2.0] - 2026-11-16

### Added
- **API keys read from a file are reloaded without a restart.** This covers `--typesense-api-key-file` / `typesense_api_key_file` and a cluster's `api_key_file`, so rotating a key (for example a Kubernetes Secret update) no longer needs a restart. Before a scrape the exporter looks at the file's modified time and length (following symlinks, which is how Kubernetes swaps a Secret volume's `..data` link), at most once per second per file, and re-reads it when they changed (on unix its inode, device and change time are compared too, so a same-length rewrite that restores the mtime is seen). One scrape uses one key for all its requests. A reload logs `reloaded the Typesense API key from <path>` at info, never the key. If the new file is unreadable, empty or not a valid HTTP header value (a byte order mark, control characters) the previous key stays in use, the problem is logged once at warn, and the read is retried until it works. Startup still fails on a bad key file. Keys from `--typesense-api-key`, `TYPESENSE_API_KEY`, `typesense_api_key_env` and a cluster's `api_key_env` never change at runtime.
- **Kubernetes examples** in [`deploy/kubernetes/`](deploy/kubernetes): plain manifests (no Helm chart) that run as a non-root user with a read-only root file system and no capabilities. `single-node/` is one exporter next to one Typesense node with a Prometheus Operator `ServiceMonitor`; `cluster-probe/` is one exporter for a whole cluster with a config file, a Secret and a Prometheus Operator `Probe` (plus the plain Prometheus scrape config). Each folder has a `kustomization.yaml` for `kubectl apply -k`.
- CI job `kubernetes`: renders both kustomizations and validates them, including the `ServiceMonitor` and `Probe` resources, with kubeconform.

## [1.1.0] - 2026-11-16

### Added
- **`GET /probe?target=<node>`: one exporter for a whole cluster**, using the Prometheus multi-target pattern of `blackbox_exporter`. Prometheus lists the nodes and passes each one as `target`; every node is its own scrape, so a hung node only times out its own scrape and `up` stays per node. The output is exactly what `/metrics` returns for that node, and the `host` and `port` labels name the Typesense node, so the dashboards and alerts work unchanged. A target is `host`, `host:port`, `http(s)://host[:port]` or an IPv6 address such as `[::1]:8108`; the port defaults to `--typesense-port` and the scheme to `--typesense-protocol`. Paths, queries, credentials and other schemes are rejected with 400.
- `--probe-allowed-targets` / `TYPESENSE_PROBE_ALLOWED_TARGETS`: comma-separated `host[:port]` nodes that `/probe` may scrape. The exporter sends its API key to every node it scrapes, so a target that isn't allowed gets a 403 before any request is made. The configured `--typesense-host` is always allowed. A single `*` allows any target and logs a startup warning. The exporter also warns at startup when the default protocol is `http` and the allow list names a non-local host.
- **`--config <file>` / `TYPESENSE_EXPORTER_CONFIG`: a TOML config file** that can set every option under its snake_case name (`typesense_host`, `probe_allowed_targets`, ...). Precedence is flag, then environment variable, then file, then default. Unknown keys are errors, and API keys are never accepted inline: use `typesense_api_key_file` or `typesense_api_key_env`. `--typesense-host`, the API key and the allow list are now required after merging, so a file-only setup works. See `deploy/exporter.example.toml`.
- **Several clusters with different API keys.** `[clusters.<name>]` tables in the config file list `targets` and one of `api_key_file` / `api_key_env`, with an optional `protocol`. `/probe?target=<node>` scrapes a node with its own cluster's key and protocol, so a key only reaches its own nodes; cluster nodes are allowed implicitly, a node in two places is a startup error, and a target that matches a cluster entry always uses that cluster's key, even with `*` in the allow list (the match is on the exact host and port, so list nodes the way Prometheus targets them). Metrics are unchanged; add a `cluster` label with `static_configs` labels in Prometheus.
- The Docker Compose stack runs one exporter that probes the three nodes, and the e2e suite checks that a target outside the allow list is refused.
- [`deploy/prometheus/legacy-names.rules.yml`](deploy/prometheus/legacy-names.rules.yml): Prometheus recording rules that rebuild the 0.4 metric names from the 1.0 names, as a ready-to-load file (previously only inside the migration guide), with `promtool` rule tests.
- CI job `rules`: `promtool check rules` on the alert and recording rules, and `promtool test rules` on the recording-rule tests.

### Security
- The exporter no longer follows HTTP redirects from Typesense, for `/metrics` and `/probe`. A redirect is a failed scrape, so the API key header is never resent to another host.
- `/probe` warns the first time it sends the API key over plain `http` to each non-loopback node.
- With `--typesense-protocol https`, `/probe` refuses `http://` targets, so a caller can't downgrade an allowed node to plaintext.
- `--probe-max-concurrency` / `TYPESENSE_PROBE_MAX_CONCURRENCY` (default 16) caps concurrent probe scrapes; further probes wait for a slot.
- Refused probes log at debug, not warn, so unauthenticated callers can't flood the logs.
- The API key header is marked sensitive so it's never printed if requests are logged.

### Changed
- `--typesense-host` is optional when `--probe-allowed-targets` is set. Without it `/metrics` answers 404 and points to `/probe`; at least one of the two options is required. With `--typesense-host` set, `/metrics` is unchanged.
- The bundled `TypesenseExporterDown` alert description now says that, with `/probe`, `up` is per probe and the alert means the exporter is unreachable or refused the target.
- `/` mentions `/probe`. Failed Typesense calls are logged with the node (`host:port`).
- Dashboard screenshots in the README show the 1.0 dashboards.
- `docs/testing.md`: the bug-injection figures were re-measured on the 1.0.0 suite.

## [1.0.0] - 2026-11-16

> **BREAKING: the metric names changed.** 1.0.0 exports the new metric names by default. Dashboards, alerts and queries that use `typesense_metrics{key=…}`, `typesense_stats{key=…}` or `typesense_stats_*` stop returning data after the upgrade. To keep them working, set `TYPESENSE_METRIC_NAMES=both` (old and new names together while you migrate) or `legacy` (old names only). See the [migration guide](https://github.com/imatefx/typesense-prometheus-exporter/blob/main/docs/migrating-to-1.0.md) for a before/after table of every query and ready-made recording rules.

### Added
- **1.0 metric names** for everything from `/metrics.json` and `/stats.json`: one family per value, in base units, with the unit in the name. Latency is in seconds, CPU and cache shares are ratios from 0 to 1. See the [metrics reference](https://github.com/imatefx/typesense-prometheus-exporter/blob/main/docs/metrics.md) and the [design note](https://github.com/imatefx/typesense-prometheus-exporter/blob/main/docs/metrics-v1.md).
  - System: `typesense_system_cpu_active_ratio`, `typesense_system_cpu_core_active_ratio{core}`, `typesense_system_{memory,swap,disk}_{total,used}_bytes`.
  - Network bytes are now **counters**: `typesense_system_network_received_bytes_total` and `typesense_system_network_sent_bytes_total`.
  - Typesense allocator memory: `typesense_process_memory_{active,allocated,mapped,metadata,resident,retained}_bytes` and `typesense_process_memory_fragmentation_ratio`.
  - Requests: `typesense_requests_per_second{operation}`, `typesense_all_requests_per_second`, `typesense_overloaded_requests_per_second`, `typesense_pending_write_batches`.
  - Latency: `typesense_request_latency_avg_seconds{operation}` and `typesense_request_latency_seconds{operation,quantile}`, with quantiles `0` (min), `0.7`, `0.95`, `0.99` and `1` (max).
  - Cache: `typesense_cache_hits`, `typesense_cache_misses`, `typesense_cache_hit_ratio`.
  - Per route and per collection: `typesense_route_latency_seconds{route}`, `typesense_route_requests_per_second{route}`, `typesense_collection_latency_seconds{method,collection,action}`, `typesense_collection_requests_per_second{method,collection,action}`.
- `typesense_unmapped{source,key}`: numeric Typesense fields the exporter has no name for yet, with their raw value. New Typesense fields still show up without an exporter release.
- `--metric-names` / `TYPESENSE_METRIC_NAMES`: `v1` (default), `both` or `legacy`. `legacy` output is identical to 0.4.1. `legacy` and `both` are deprecated: the exporter logs a warning when they are used, and they will be removed in 2.0.0.
- [Migration guide](https://github.com/imatefx/typesense-prometheus-exporter/blob/main/docs/migrating-to-1.0.md) with PromQL for every renamed family, Grafana unit tips, and recording rules that rebuild the 0.4 names in Prometheus.

### Changed
- **The default metric names are the 1.0 names** (see the note above). Health, raft, version, collection document counts and scrape health keep their names in every mode, so the bundled alert rules need no change.
- The collection label is `collection` everywhere (`collection_name` in the 0.4 names), and the per-route label is `route` (`key` in the 0.4 names).
- The bundled Grafana dashboards use the 1.0 names: latency panels are in seconds, CPU in ratio units, and network panels plot `rate()` of the new byte counters. The Docker Compose stack runs the exporters with `TYPESENSE_METRIC_NAMES=v1`, and the e2e tests check that no 0.4 family is emitted.
- The Docker image builds with Rust 1.100 (`rust:1.100-alpine`, from Dependabot).
- GitHub Actions updated by Dependabot: `actions/checkout` 7.0.1, `actions/upload-artifact` 7.0.2, `actions/download-artifact` 8.0.2, `actions/attest-build-provenance` 4.2.2, `docker/setup-buildx-action` 4.4.1, `docker/login-action` 4.8.0, `docker/build-push-action` 7.4.0 and `docker/metadata-action`.

## [0.4.1] - 2026-11-16

### Added
- Dependabot: weekly grouped updates for Rust crates, GitHub Actions and the Docker base image. Security alerts and automatic security fixes are enabled.

### Changed
- Every GitHub Action is pinned to a commit SHA, with its version as a comment for Dependabot.
- CI builds arm64 natively on GitHub's ARM runners (`ubuntu-24.04-arm`), for both the Docker image and the Linux arm64 release binary. QEMU emulation and the cross toolchain are no longer used. Each architecture's image builds in parallel and is merged into one multi-arch manifest.

## [0.4.0] - 2026-11-16

### Added
- `typesense_scrape_success{endpoint}` and `typesense_scrape_duration_seconds`, so partial scrape failures (missing key permissions, older Typesense) can be alerted on. Comes with a `TypesenseScrapeFailing` alert rule.
- `--typesense-api-key-file` / `TYPESENSE_API_KEY_FILE`: read the API key from a file, e.g. a Docker or Kubernetes secret.
- CI runs `cargo audit`. Releases and Docker images are only published after the tests pass.

### Changed
- **Bounded labels:** document and other resource IDs in Typesense's per-endpoint stats are replaced with `:id` (`GET /collections/products/documents/:id`). Before, each document read by ID created new series. Merged entries sum their request rates and average latency weighted by rate.
- **The `_by_collection` metrics no longer carry the `key` label.** It duplicated the per-endpoint metrics. Update any queries that filter on it.
- **`--typesense-timeout` takes 0 or a positive number of seconds;** 0 disables it. Negative values are rejected.
- Values Typesense reports that aren't numbers are left out instead of exported as 0.
- `/collections` is requested without schemas (`exclude_fields=fields`), a much smaller response.
- The container runs as an unprivileged user (65534) and caches dependencies between builds.
- Dependencies upgraded: axum 0.8, reqwest 0.12, prometheus 0.14 (without protobuf), tokio, clap and others.

### Security
- Fixed 19 known vulnerabilities in pinned dependencies (h2, rustls, rustls-webpki, ring, tokio, protobuf and others). `cargo audit` is clean.
- The API key is hidden in `--help` and redacted from debug output. A warning is logged when it would be sent over plain HTTP to another machine.

## [0.3.1] - 2026-11-16

### Fixed
- Docker images are published automatically again. The Docker workflow now runs on version tags, instead of on GitHub releases (which the release workflow creates in a way that doesn't trigger other workflows). Its build attestation step also failed for lack of the `id-token: write` permission.

## [0.3.0] - 2026-11-16

### Added
- **Every numeric `/stats.json` field is now exported** as `typesense_stats{key=...}`:
  - latency p70, p95 and p99, min and max for search, write, import and delete
  - `cache_hit_count` and `cache_miss_count`
  - any field later Typesense versions add
- `typesense_info{version}`: the Typesense version each node runs, from `/debug`.
- `typesense_raft_committed_index` and `typesense_raft_queued_writes` from `/status`. Comparing the committed index across nodes shows replication lag.
- `typesense_collection_documents{collection}`: document count per collection, from `/collections`.
- Grafana: a new **Typesense Cluster Overview** dashboard, with:
  - unhealthy nodes, leader, documents, search rate, p95 and cache ratio
  - per-node health, raft state and version
  - replication lag, commit rate and queued writes
  - traffic
- Grafana: a **Node** filter on every dashboard.
- Grafana: latency panels show avg, p95, p99 and max.
- Grafana: cache hits and misses, and documents per collection.
- Docs: the exact scoped API key actions the exporter needs.

### Changed
- **The scoped API key needs two more actions:** `status:list` and `collections:list`. Without them, only the new metrics are missing; the exporter logs the 401 and exports the rest.
- `/stats.json` fields that are missing or non-numeric are now left out instead of exported as 0.
- Grafana: health and raft state tiles moved from Typesense Stats to the Cluster Overview.

### Fixed
- Grafana By Collection: selecting several collections, or All, now works (`=~` instead of `=`). The collection list now includes collections with no traffic yet.

## [0.2.0] - 2026-11-16

A rewrite of the exporter's internals, plus cluster monitoring, new metrics,
dashboards, a local stack and a full test suite. Metric names and labels from
0.1.x are unchanged; see **Changed** for behaviour differences.

### Added
- `typesense_health`: 1 when the node's `/health` reports `{"ok": true}`, otherwise 0. ([#13], Andrii Maryskevych)
- `typesense_raft_state`: 1 = leader, 4 = follower, 0 = other or unreachable. ([#13], Andrii Maryskevych)
- `--typesense-timeout` / `TYPESENSE_TIMEOUT`: per-request timeout for Typesense calls. ([#13], Andrii Maryskevych)
- Per-core CPU metrics for every core Typesense reports. 0.1.x stopped at core 4 and ignored cores 5–8. ([#15], cyppe)
- Swap metrics: `system_memory_total_swap_bytes` and `system_memory_used_swap_bytes`. ([#15], cyppe)
- `cache_hit_ratio` in `typesense_stats`, exported when Typesense reports it (v28+). ([#15], cyppe)
- Grafana panels for:
  - health and raft state per node
  - cache hit ratio
  - swap usage
  - per-core CPU
- Prometheus alert rules: `TypesenseExporterDown`, `TypesenseUnhealthy` and `TypesenseNoLeader`.
- A Docker Compose stack: a 3-node Typesense cluster, one exporter per node, and Prometheus and Grafana, both provisioned. Plus `scripts/traffic.sh` to generate load.
- Tests:
  - unit tests
  - integration tests against a mock Typesense
  - Rust end-to-end tests against the 3-node cluster, covering failover, a hung node and quorum loss
  - coverage reporting
- CI: lint, tests with coverage (90% minimum), e2e with a JUnit report, and a coverage badge.
- `build-cross.sh`: builds static linux/amd64 and linux/arm64 binaries.
- Docs: getting started, configuration, architecture, metrics and testing.

### Changed
- **Failed endpoints are omitted, not zeroed.** If Typesense is unreachable, times out or rejects the API key, that endpoint's metrics are left out of the scrape. Before, they were reported as 0, or the request panicked. `/metrics` always returns 200, and health and raft state drop to 0.
- **Unreported values are omitted.** Metrics that Typesense doesn't include in its response (e.g. swap on older versions) are left out instead of exported as 0.
- `TYPESENSE_TIMEOUT` defaults to 5s, and 0 or less disables it. Keep it below Prometheus' `scrape_timeout`.
- One shared HTTP client, and the regex is compiled once, instead of both being rebuilt on every scrape.
- Logging goes through `tracing`; the per-scrape debug output is gone.
- reqwest uses rustls, so there's no OpenSSL dependency.
- Docker image built with Rust 1.88 for linux/amd64 and linux/arm64.
- Grafana legends name the node, so clusters are readable, and sub-millisecond latencies no longer show as 0.

### Fixed
- Scrapes no longer fail on hosts that don't report `system_cpu1_active_percentage` or `system_cpu_active_percentage`. ([#11], weriomat)
- A request error, an unexpected status or a malformed response no longer crashes the scrape.

## [0.1.5] - 2025-07-03
### Added
- Multi-platform Docker images (linux/amd64 and linux/arm64). ([#10], Andrii Maryskevych)
- MIT license.

## [0.1.4] - 2024-05-06
### Changed
- Docker publishing workflow adjustments.

## [0.1.3] - 2024-05-06
### Added
- GitHub workflow publishing the Docker image to Docker Hub on release.

## [0.1.2] - 2024-05-06
### Added
- Grafana dashboards and screenshots.
### Fixed
- The stats endpoint used a hardcoded protocol instead of `--typesense-protocol`. ([#4], Leandro)

## [0.1.1] - 2024-01-18
### Added
- Per-collection latency and request-rate metrics.
- Graceful shutdown.
- Dockerfile.

## [0.1.0] - 2024-01-17
### Added
- First release: exports Typesense `/metrics.json` and `/stats.json` in Prometheus format.

[Unreleased]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.4.1...v1.0.0
[0.4.1]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.5...v0.2.0
[0.1.5]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/imatefx/typesense-prometheus-exporter/releases/tag/v0.1.0
[#4]: https://github.com/imatefx/typesense-prometheus-exporter/pull/4
[#10]: https://github.com/imatefx/typesense-prometheus-exporter/pull/10
[#11]: https://github.com/imatefx/typesense-prometheus-exporter/pull/11
[#13]: https://github.com/imatefx/typesense-prometheus-exporter/pull/13
[#15]: https://github.com/imatefx/typesense-prometheus-exporter/pull/15
