# Typesense Prometheus Exporter
## Screenshots
### Typesense Metrics Dashboard
![Typesense-Metrics-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/36daa26f-b217-4c1c-9404-2a480a19efe2)

### Typesense Metrics By Collection Dashboard
![Typesense-By-Collection-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/026e6ed9-2a9b-4800-8ae6-2f90aca9fcb7)

### Typesense Stats Dashboard
![Typesense-Stats-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/afc7948a-de7a-4b8f-a1e6-a8394c61859f)

## Introduction

This repository contains the Typesense-Prometheus-Exporter, a tool designed to expose Typesense metrics and stats in Prometheus format.

### About Typesense

[Typesense](https://typesense.org) is an open-source, typo-tolerant search engine that delivers fast and relevant search results. It's designed for simplicity and ease of use, providing a scalable, highly available, and easy-to-scale search solution.

### About Prometheus

[Prometheus](https://prometheus.io/) is an open-source systems monitoring and alerting toolkit. Known for its multi-dimensional data model and flexible query language, Prometheus integrates with various external systems for comprehensive monitoring.

### About Prometheus Exporters

Prometheus exporters are tools for translating metrics from third-party systems into Prometheus-readable format, crucial for monitoring non-Prometheus systems with Prometheus.

## What It Does

This exporter fetches metrics and stats from a configured Typesense instance and exports them in Prometheus format, enabling their consumption by a Prometheus time series database server.

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

## Local stack (Docker Compose)

`docker compose up -d --build` starts Typesense, the exporter, Prometheus and Grafana:

| Service | URL |
|---|---|
| Grafana (dashboards provisioned) | http://localhost:3000 (admin / admin) |
| Prometheus (with alert rules) | http://localhost:9090 |
| Exporter | http://localhost:8888/metrics |
| Typesense | http://localhost:8108 (API key `local-dev-key`) |

Override the defaults with `TYPESENSE_API_KEY` and `GRAFANA_ADMIN_PASSWORD`. Prometheus alert rules live in `deploy/prometheus/alerts.yml`:
- `TypesenseExporterDown`: the exporter can't be scraped.
- `TypesenseUnhealthy`: a node is unhealthy or unreachable.
- `TypesenseNoLeader`: no node reports raft leader.

## Testing

| Command | What it runs |
|---|---|
| `make test-unit` | Unit tests (models, metric rendering, CLI) |
| `make test-integration` | The exporter served over HTTP against a mock Typesense (healthy, follower, down, 401, timeout, malformed responses) |
| `make e2e` | Starts the compose stack, sends real Typesense traffic, and checks the exporter, Prometheus, alerts and Grafana, including Typesense going down and recovering |
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
