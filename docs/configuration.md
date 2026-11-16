# Configuration

## Exporter options

Every option can be set as a command-line flag or as an environment variable. The flag wins if both are set.

| Flag | Environment variable | Default | Description |
|---|---|---|---|
| `--typesense-host` | `TYPESENSE_HOST` | *(required)* | Hostname or IP of the Typesense node. Also used as the `host` label on every metric. |
| `--typesense-api-key` | `TYPESENSE_API_KEY` | *(required)* | API key sent as `X-TYPESENSE-API-KEY`. See [API key permissions](#api-key-permissions). |
| `--typesense-port` | `TYPESENSE_PORT` | `8108` | Typesense API port. Also the `port` label. |
| `--typesense-protocol` | `TYPESENSE_PROTOCOL` | `http` | `http` or `https`. HTTPS uses rustls with the bundled CA certificates. |
| `--typesense-timeout` | `TYPESENSE_TIMEOUT` | `5` | Seconds allowed for each Typesense request. `0` or less disables the timeout. See [Timeouts](#timeouts). |
| `--exporter-bind-address` | `EXPORTER_BIND_ADDRESS` | `0.0.0.0` | Address the exporter listens on. |
| `--exporter-bind-port` | `EXPORTER_BIND_PORT` | `8888` | Port the exporter listens on. |

`typesense-prometheus-exporter --help` prints the same list. Logs go to stdout at `INFO` level, and failed Typesense calls are logged at `WARN`.

## Endpoints

| Path | Response |
|---|---|
| `/metrics` | Prometheus text format. Always HTTP 200; see [Failure behaviour](#failure-behaviour). |
| `/` | A short text pointer to `/metrics`. |

## API key permissions

On every scrape the exporter calls six Typesense endpoints:

| Endpoint | Used for | Scoped-key action |
|---|---|---|
| `/metrics.json` | `typesense_metrics` | `metrics.json:list` |
| `/stats.json` | all `typesense_stats*` metrics | `stats.json:list` |
| `/debug` | `typesense_raft_state`, `typesense_info` | `debug:list` |
| `/status` | `typesense_raft_committed_index`, `typesense_raft_queued_writes` | `status:list` |
| `/collections` | `typesense_collection_documents` | `collections:list` |
| `/health` | `typesense_health` | none |

An admin key works. A least-privilege [scoped key](https://typesense.org/docs/latest/api/api-keys.html) needs exactly the actions above:

```bash
curl -X POST "$TYPESENSE/keys" -H "X-TYPESENSE-API-KEY: $ADMIN_KEY" -d '{
  "description": "prometheus exporter",
  "actions": ["metrics.json:list", "stats.json:list", "debug:list", "status:list", "collections:list"],
  "collections": ["*"]
}'
```

A missing action only drops the metrics from that endpoint; the exporter logs the 401 and exports the rest.

## Timeouts

Keep `TYPESENSE_TIMEOUT` **below** Prometheus' `scrape_timeout`.

The exporter waits for Typesense before answering Prometheus. If a node hangs and the exporter waits longer than Prometheus will, Prometheus abandons the whole scrape. You then see `up == 0` and `TypesenseExporterDown` instead of `typesense_health == 0` and `TypesenseUnhealthy`. With the timeout below the scrape timeout, the exporter answers in time and reports the node as unhealthy.

| Setup | `TYPESENSE_TIMEOUT` | Prometheus `scrape_timeout` |
|---|---|---|
| Defaults | 5s | 10s (Prometheus default) |
| Bundled Docker Compose stack | 2s | 4s (5s scrape interval) |

## Failure behaviour

Each scrape calls the six Typesense endpoints concurrently, and each one succeeds or fails on its own.

| Situation | What `/metrics` returns |
|---|---|
| Everything works | All metrics |
| `/metrics.json` or `/stats.json` fails (unreachable, timeout, 401, bad JSON) | That endpoint's metrics are left out. They are **not** reported as 0. |
| `/health` fails, or reports `ok: false` | `typesense_health 0` |
| `/debug` fails | `typesense_raft_state 0`, no `typesense_info` |
| `/status` or `/collections` fails (e.g. older Typesense, missing key action) | Those metrics are left out |
| Typesense leaves a field out (e.g. latency percentiles on older versions) | That metric is left out |

Every failure is logged at `WARN` with the endpoint and the reason.

## Prometheus

```yaml
scrape_configs:
  - job_name: typesense-exporter
    scrape_interval: 15s
    scrape_timeout: 10s
    static_configs:
      - targets: ["exporter-1:8888", "exporter-2:8888", "exporter-3:8888"]

rule_files:
  - alerts.yml
```

### Alert rules

[`deploy/prometheus/alerts.yml`](../deploy/prometheus/alerts.yml):

| Alert | Expression | Fires after |
|---|---|---|
| `TypesenseExporterDown` | `up{job="typesense-exporter"} == 0` | 1m |
| `TypesenseUnhealthy` | `typesense_health == 0` | 1m |
| `TypesenseNoLeader` | `max(typesense_raft_state == bool 1) == 0` | 1m |

The `for: 1m` matters for `TypesenseNoLeader`. A normal leader election leaves the cluster without a leader for a few seconds, and the alert should not fire for that. The e2e suite checks that it doesn't.

If one Prometheus monitors several Typesense clusters, add a cluster label to the scrape targets and aggregate per cluster. Otherwise a leader in one cluster hides a leaderless one:

```yaml
expr: max by (cluster) (typesense_raft_state == bool 1) == 0
```

## Grafana

Import the dashboards from [`grafana/`](../grafana) (**Dashboards → New → Import**) and pick your Prometheus data source when asked.

| Dashboard | Shows |
|---|---|
| Typesense Cluster Overview | Unhealthy nodes, leader, documents, search rate, p95, cache ratio; per-node health, raft state and version; replication lag, commit rate, queued writes; traffic and tail latency |
| Typesense Metrics | CPU (overall and per core), disk, memory, swap, network, Typesense allocator memory |
| Typesense Stats | Requests per second, and latency (avg, p95, p99, max) by operation; cache hit ratio, hits and misses |
| Typesense By Collection | Requests per second and latency per collection; documents per collection |

Every dashboard has a **Node** filter (`host` variable) to focus on one or more nodes.

## Docker Compose stack

[`docker-compose.yml`](../docker-compose.yml) reads these variables:

| Variable | Default | Used for |
|---|---|---|
| `TYPESENSE_API_KEY` | `local-dev-key` | Typesense nodes and exporters |
| `GRAFANA_ADMIN_PASSWORD` | `admin` | Grafana admin login |

| Path | Contents |
|---|---|
| `deploy/typesense/nodes` | Cluster membership, as fixed IPs (see [architecture](architecture.md#docker-compose-stack)) |
| `deploy/prometheus/prometheus.yml` | Scrape config, with the 4s scrape timeout |
| `deploy/prometheus/alerts.yml` | Alert rules |
| `deploy/grafana/provisioning/` | Grafana data source and dashboard provisioning |
