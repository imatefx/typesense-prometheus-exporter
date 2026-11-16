# Typesense Prometheus Exporter

[![CI](https://github.com/imatefx/typesense-prometheus-exporter/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/imatefx/typesense-prometheus-exporter/actions/workflows/ci.yml)
[![Release build](https://github.com/imatefx/typesense-prometheus-exporter/actions/workflows/rust.yml/badge.svg)](https://github.com/imatefx/typesense-prometheus-exporter/actions/workflows/rust.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/imatefx/typesense-prometheus-exporter/badges/coverage.json)](https://github.com/imatefx/typesense-prometheus-exporter/actions/workflows/ci.yml)
[![Docker version](https://img.shields.io/docker/v/imatefx/typesense-prometheus-exporter?sort=semver&label=docker)](https://hub.docker.com/r/imatefx/typesense-prometheus-exporter)
[![Docker pulls](https://img.shields.io/docker/pulls/imatefx/typesense-prometheus-exporter)](https://hub.docker.com/r/imatefx/typesense-prometheus-exporter)
[![Image size](https://img.shields.io/docker/image-size/imatefx/typesense-prometheus-exporter/latest)](https://hub.docker.com/r/imatefx/typesense-prometheus-exporter/tags)
[![Platforms](https://img.shields.io/badge/platforms-linux%2Famd64%20%7C%20linux%2Farm64-blue)](https://hub.docker.com/r/imatefx/typesense-prometheus-exporter/tags)
[![Typesense](https://img.shields.io/badge/tested%20with-Typesense%2030.1-e5446d)](https://typesense.org)
[![License: MIT](https://img.shields.io/github/license/imatefx/typesense-prometheus-exporter)](LICENSE)

Exposes [Typesense](https://typesense.org) metrics, stats, health and raft cluster state in [Prometheus](https://prometheus.io) format, with ready-made Grafana dashboards and alert rules.

## Features

- **System and memory metrics** from `/metrics.json`: CPU (every core), disk, memory, swap, network, and Typesense allocator memory.
- **Request stats** from `/stats.json`: requests per second and latency by operation, per endpoint and per collection, plus pending write batches and cache hit ratio.
- **Health and cluster state:** `typesense_health` and `typesense_raft_state` (leader or follower), so you can alert on unhealthy nodes or a cluster with no leader.
- **Honest failures:** if Typesense can't be reached, its metrics are left out rather than reported as zeros, and a request timeout keeps a hung node from stalling scrapes.
- **Small static image:** about 13 MB, built `FROM scratch` for linux/amd64 and linux/arm64.
- **Batteries included:** 3 Grafana dashboards, Prometheus alert rules, and a one-command Docker Compose cluster for trying it out.

## Screenshots

Captured from the bundled Docker Compose stack: a 3-node cluster under generated traffic.

### Typesense Metrics
![Typesense Metrics dashboard](screenshots/Typesense-Metrics.png)

### Typesense Stats
![Typesense Stats dashboard](screenshots/Typesense-Stats.png)

### Typesense By Collection
![Typesense By Collection dashboard](screenshots/Typesense-By-Collection.png)

## Quick start

Try the full stack (Typesense cluster, exporters, Prometheus, Grafana) locally:

```
docker compose up -d --build
./scripts/traffic.sh 300        # optional: generate traffic for the dashboards
open http://localhost:3000      # Grafana, admin / admin
```

Or run just the exporter against your own Typesense node:

```
docker run -p 8888:8888 \
  -e TYPESENSE_HOST=typesense.example.com \
  -e TYPESENSE_API_KEY=your-admin-key \
  imatefx/typesense-prometheus-exporter
curl localhost:8888/metrics
```

## Docker Deployment

### Pulling the Docker Image

To pull the Typesense-Prometheus-Exporter image from Docker Hub:

```
docker pull imatefx/typesense-prometheus-exporter
```

### Running the Docker Container

To run the exporter in a Docker container, use the following command:

```
docker run --name="example-ts-prom-exp" \
           -e "TYPESENSE_HOST=tshost.example.com" \
           -e "TYPESENSE_PROTOCOL=http" \
           -e "TYPESENSE_API_KEY=tsAPIKEY" \
           -e "TYPESENSE_PORT=8108" \
           -p 8888:8888 \
           imatefx/typesense-prometheus-exporter
```

This command sets up the Typesense-Prometheus-Exporter with the necessary environment variables and starts the exporter service on exposed port `8888`.


## Usage

### Exposing Typesense Metrics and Stats in Prometheus Format

Run the exporter with the necessary options:

```
typesense-prometheus-exporter [OPTIONS] --typesense-host <TYPESENSE_HOST> --typesense-api-key <TYPESENSE_API_KEY>
```

### Options

- `--typesense-host <TYPESENSE_HOST>`: Typesense Host URL (env: TYPESENSE_HOST).
- `--typesense-protocol <TYPESENSE_PROTOCOL>`: Typesense protocol (env: TYPESENSE_PROTOCOL, default: http).
- `--typesense-api-key <TYPESENSE_API_KEY>`: Typesense API key (env: TYPESENSE_API_KEY).
- `--typesense-port <TYPESENSE_PORT>`: Typesense port number (env: TYPESENSE_PORT, default: 8108).
- `--typesense-timeout <TYPESENSE_TIMEOUT>`: Timeout for Typesense API requests in seconds (env: TYPESENSE_TIMEOUT, default: 5; 0 or less disables it). Keep it below the Prometheus scrape timeout.
- `--exporter-bind-address <EXPORTER_BIND_ADDRESS>`: Internal server bind address (env: EXPORTER_BIND_ADDRESS, default: 0.0.0.0).
- `--exporter-bind-port <EXPORTER_BIND_PORT>`: Internal server bind port (env: EXPORTER_BIND_PORT, default: 8888).

### Help and Version

- `-h, --help`: Display help.
- `-V, --version`: Display version.

## Metrics

All metrics are gauges labelled with `host` and `port` (the Typesense node being scraped).

| Metric | Extra labels | Source | Description |
|---|---|---|---|
| `typesense_health` | | `/health` | 1 if the node reports `{"ok": true}`, otherwise 0 (including unreachable) |
| `typesense_raft_state` | | `/debug` | 1 = leader, 4 = follower, 0 = other or unreachable |
| `typesense_metrics` | `key` | `/metrics.json` | System and Typesense memory metrics, e.g. `key="system_memory_used_bytes"`, `key="system_cpu3_active_percentage"` |
| `typesense_stats` | `key` | `/stats.json` | Totals such as `search_latency_ms`, `write_requests_per_second`, `pending_write_batches`, `cache_hit_ratio` |
| `typesense_stats_latency_ms` | `key` | `/stats.json` | Latency per endpoint, e.g. `key="GET /collections/products/documents/search"` |
| `typesense_stats_requests_per_second` | `key` | `/stats.json` | Requests per second per endpoint |
| `typesense_stats_latency_ms_by_collection` | `key`, `method`, `collection_name`, `action` | `/stats.json` | Per-endpoint latency for collection endpoints |
| `typesense_stats_requests_per_second_by_collection` | `key`, `method`, `collection_name`, `action` | `/stats.json` | Per-endpoint requests per second for collection endpoints |

The API key needs access to `/metrics.json`, `/stats.json` and `/debug`. An admin key works; `/health` needs no key.

### Prometheus

Run one exporter per Typesense node and scrape them all:

```yaml
scrape_configs:
  - job_name: typesense-exporter
    scrape_timeout: 10s   # keep above the exporter's TYPESENSE_TIMEOUT (default 5s)
    static_configs:
      - targets: ["exporter-1:8888", "exporter-2:8888", "exporter-3:8888"]
```

Ready-made alert rules (exporter down, unhealthy node, no raft leader) are in [`deploy/prometheus/alerts.yml`](deploy/prometheus/alerts.yml). Grafana dashboards are in [`grafana/`](grafana): import them and pick your Prometheus data source.

## Local stack (Docker Compose)

`docker compose up -d --build` starts a 3-node Typesense cluster with one exporter per node, plus Prometheus and Grafana. Run one exporter per Typesense node in production too: each node reports its own health, raft role and traffic.

| Service | URL |
|---|---|
| Grafana (dashboards provisioned) | http://localhost:3000 (admin / admin) |
| Prometheus (with alert rules) | http://localhost:9090 |
| Exporters | http://localhost:8888/metrics, :8889, :8890 (nodes 1–3) |
| Typesense nodes | http://localhost:8108, :8109, :8110 (API key `local-dev-key`) |

`scripts/traffic.sh [seconds]` generates search and write traffic across the cluster so the dashboards have something to show.

Set `TYPESENSE_TIMEOUT` below Prometheus' `scrape_timeout`. The stack uses 2s against 4s. Otherwise a hung node makes the whole scrape time out, and you get `TypesenseExporterDown` instead of `TypesenseUnhealthy`.

Override the defaults with `TYPESENSE_API_KEY` and `GRAFANA_ADMIN_PASSWORD`. Prometheus alert rules live in `deploy/prometheus/alerts.yml`:
- `TypesenseExporterDown`: the exporter can't be scraped.
- `TypesenseUnhealthy`: a node is unhealthy or unreachable.
- `TypesenseNoLeader`: no node reports raft leader.

## Testing

| Command | What it runs |
|---|---|
| `make test-unit` | Unit tests (models, metric rendering, CLI) |
| `make test-integration` | The exporter served over HTTP against a mock Typesense (healthy, follower, down, 401, timeout, malformed responses) |
| `make e2e` | Starts the 3-node cluster stack and checks, end to end:<br>• cluster formation, with one leader and two followers<br>• per-node traffic reaching the exporters, Prometheus and Grafana<br>• leader failover<br>• a hung node, and quorum loss firing `TypesenseNoLeader`<br>• recovery |
| `make coverage` | Unit + integration coverage via [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (fails under 90% of lines) |

CI (`.github/workflows/ci.yml`) runs lint, tests with coverage, and the e2e suite on every pull request.

## Building

- Local build: `cargo build --release`
- Static Linux binaries for amd64 and arm64 (needs Docker with buildx): `./build-cross.sh`. Binaries are written to `releases/`.

## Behaviour notes

- `typesense_health` (1 = healthy) and `typesense_raft_state` (1 = leader, 4 = follower, 0 = other/unreachable) are always exported, so you can alert on an unhealthy node or a cluster with no leader.
- If a Typesense endpoint is unreachable, times out, or rejects the API key, its metrics are omitted for that scrape instead of being reported as zeros. Set `--typesense-timeout` so a hung node doesn't stall the scrape.
- Logs go to stderr via `tracing`.

## Contributors

Thanks to everyone who has contributed to this exporter:

| Contributor | Contributions |
|---|---|
| [Stalin Pereira (imatefx)](https://github.com/imatefx) | Creator and maintainer |
| [Leandro (leapalazzolo)](https://github.com/leapalazzolo) | Fixed the hardcoded protocol on the stats endpoint ([#4](https://github.com/imatefx/typesense-prometheus-exporter/pull/4)) |
| [Andrii Maryskevych (andriimredocly)](https://github.com/andriimredocly) | Multi-platform Docker builds with ARM64 ([#10](https://github.com/imatefx/typesense-prometheus-exporter/pull/10)); health, raft state and request timeout ([#13](https://github.com/imatefx/typesense-prometheus-exporter/pull/13)) |
| [weriomat](https://github.com/weriomat) | Optional CPU metrics for hosts that don't report them ([#11](https://github.com/imatefx/typesense-prometheus-exporter/pull/11)) |
| [Sebastian Caldarola (scaldarola)](https://github.com/scaldarola) | Proposed scraping multiple Typesense hosts from one exporter ([#14](https://github.com/imatefx/typesense-prometheus-exporter/pull/14)) |
| [cyppe](https://github.com/cyppe) | Per-core CPU metrics for any core count, swap memory and cache hit ratio ([#15](https://github.com/imatefx/typesense-prometheus-exporter/pull/15)) |

## License

[MIT](LICENSE)
