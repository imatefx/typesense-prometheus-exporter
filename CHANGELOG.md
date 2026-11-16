# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/imatefx/typesense-prometheus-exporter/compare/v0.4.0...HEAD
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
