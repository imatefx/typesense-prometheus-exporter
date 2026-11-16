# Configuration

## Config file

Every option can also be set in a TOML file, so the exporter can start with just:

```bash
typesense-prometheus-exporter --config exporter.toml    # or TYPESENSE_EXPORTER_CONFIG=exporter.toml
```

The keys are the option names in snake_case. **Precedence: command-line flag, then environment variable, then config file, then the built-in default.** A flag or variable always wins over the file, so one file can be shared and overridden per deployment.

```toml
# deploy/exporter.example.toml lists every key; all are optional.
typesense_host = "localhost"          # node served at /metrics
typesense_port = 8108                 # default 8108
typesense_protocol = "http"           # default "http"
typesense_timeout = 5                 # default 5 (seconds, 0 disables)
typesense_api_key_file = "/run/secrets/typesense-api-key"   # or typesense_api_key_env
exporter_bind_address = "0.0.0.0"     # default "0.0.0.0"
exporter_bind_port = 8888             # default 8888
metric_names = "v1"                   # default "v1"; "both" or "legacy"
probe_allowed_targets = ["ts-1", "ts-2:8108"]   # default empty
probe_max_concurrency = 16            # default 16

[clusters.prod]                       # see "Several clusters with different keys"
targets = ["ts-prod-1:8108"]
api_key_file = "/run/secrets/typesense-prod"
```

- **No inline secrets.** `typesense_api_key` and `api_key` are not accepted in the file; they are errors that say so. Name a file (`typesense_api_key_file`) or an environment variable (`typesense_api_key_env`) instead. `--typesense-api-key` and `TYPESENSE_API_KEY` keep working, and a key given by flag or environment replaces the file's key settings.
- **Unknown keys are errors**, so a typo is never silently ignored. Invalid values are reported with the file, line and key. Errors never contain key values.
- Relative paths in the file (key files) are relative to the exporter's working directory.
- Requirements are checked after merging, so a file-only setup works: at least one of `typesense_host`, `probe_allowed_targets` or a `[clusters.*]` table, and a default key whenever `typesense_host` or `probe_allowed_targets` is used.
- The file is read once at startup.

## Exporter options

Every option can be set as a command-line flag, an environment variable or a [config file](#config-file) key (flag first).

| Flag | Environment variable | Default | Description |
|---|---|---|---|
| `--config` | `TYPESENSE_EXPORTER_CONFIG` | | Path to the [config file](#config-file). |

| Flag | Environment variable | Default | Description |
|---|---|---|---|
| `--typesense-host` | `TYPESENSE_HOST` | *(see description)* | Hostname or IP of the Typesense node served at `/metrics`. Also used as the `host` label on every metric. Optional when `--probe-allowed-targets` or config-file clusters are set; one of the three is required. Without it, `/metrics` answers 404 and you scrape nodes through `/probe`. |
| `--typesense-api-key` | `TYPESENSE_API_KEY` | *(see description)* | Default API key, required with `--typesense-host` or `--probe-allowed-targets` (as this or the file option); clusters in the config file bring their own. Sent as `X-TYPESENSE-API-KEY`. Never printed: hidden in `--help` and redacted in debug output. See [API key permissions](#api-key-permissions). |
| `--typesense-api-key-file` | `TYPESENSE_API_KEY_FILE` | | File holding the API key, e.g. a Docker or Kubernetes secret; surrounding whitespace is ignored. Preferred over the flag, which other users can see in the process list. |
| `--typesense-port` | `TYPESENSE_PORT` | `8108` | Typesense API port. Also the `port` label. It is also the default port for `/probe` targets and `--probe-allowed-targets` entries that don't name one. |
| `--typesense-protocol` | `TYPESENSE_PROTOCOL` | `http` | `http` or `https`. HTTPS uses rustls with the bundled CA certificates. Also the default scheme for `/probe` targets that don't name one. |
| `--typesense-timeout` | `TYPESENSE_TIMEOUT` | `5` | Seconds allowed for each Typesense request; `0` disables the timeout. See [Timeouts](#timeouts). |
| `--exporter-bind-address` | `EXPORTER_BIND_ADDRESS` | `0.0.0.0` | Address the exporter listens on. |
| `--exporter-bind-port` | `EXPORTER_BIND_PORT` | `8888` | Port the exporter listens on. |
| `--metric-names` | `TYPESENSE_METRIC_NAMES` | `v1` | Which metric names to export: `v1`, `both` or `legacy`. See [Metric names](#metric-names). |
| `--probe-allowed-targets` | `TYPESENSE_PROBE_ALLOWED_TARGETS` | *(empty)* | Comma-separated `host[:port]` nodes that `/probe` may scrape, or `*` for any. See [Probing several nodes](#probing-several-nodes-from-one-exporter). |
| `--probe-max-concurrency` | `TYPESENSE_PROBE_MAX_CONCURRENCY` | `16` | Most `/probe` scrapes running at once (minimum 1). Further probes wait for a free slot rather than fail. Each scrape makes six requests to its node, so this caps the load anyone who can reach the exporter can put on your nodes. |

`typesense-prometheus-exporter --help` prints the same list. Logs go to stdout at `INFO` level, and failed Typesense calls are logged at `WARN`.

## Metric names

**1.0.0 renamed the metrics from `/metrics.json` and `/stats.json`** (units in the name, base units, one family per value). `--metric-names` / `TYPESENSE_METRIC_NAMES` picks which names are exported:

| Value | Exports |
|---|---|
| `v1` *(default)* | The 1.0 names only, listed in the [metrics reference](metrics.md). Unknown Typesense fields go to `typesense_unmapped{source,key}`. |
| `both` | The 1.0 names and the 0.4 names side by side, for moving dashboards and queries over |
| `legacy` | The 0.4 names only (`typesense_metrics{key}`, `typesense_stats{key}`, `typesense_stats_*`), exactly as 0.4.x |

Any other value is rejected at startup. Health, raft, version, collection and scrape-health metrics are the same in every mode, so the bundled alert rules work with any value.

**Upgrading from 0.4.x with the default breaks queries that use the old names.** Set `TYPESENSE_METRIC_NAMES=both` while you migrate. `legacy` and `both` are deprecated (the exporter logs a warning) and will be removed in 2.0.0. See [migrating to 1.0](migrating-to-1.0.md).

## Endpoints

| Path | Response |
|---|---|
| `/metrics` | The node set with `--typesense-host`, in Prometheus text format. Always HTTP 200; see [Failure behaviour](#failure-behaviour). 404 with an explanation when no host is configured. |
| `/probe?target=<node>` | The same output for the named node, which must be allowed. 200 even when the node is down; 400 for a missing or malformed target; 403 for a target that isn't allowed. |
| `/` | A short text pointer to `/metrics` and `/probe`. |

## Probing several nodes from one exporter

`/probe?target=<node>` follows the Prometheus multi-target pattern used by `blackbox_exporter`: one exporter, with Prometheus listing the nodes and passing each one as `target`. Every node is its own scrape, so a hung node only times out its own scrape, and `up`, `typesense_scrape_success` and the alerts stay per node. The output is exactly what `/metrics` would return for that node, and the `host` and `port` labels name the Typesense node, not the exporter. See [getting started](getting-started.md#one-exporter-for-the-whole-cluster) for a Prometheus config.

A target is one of `host`, `host:port`, `http://host[:port]`, `https://host[:port]` or an IPv6 address in brackets such as `[::1]:8108`. The port defaults to `--typesense-port` and the scheme to `--typesense-protocol`, so one setting covers `/metrics` and `/probe`. Hosts are lowercased. Anything with a path, query, fragment, credentials or another scheme is rejected with 400.

The same `--typesense-api-key` is used for every node, unless the node belongs to a [cluster with its own key](#several-clusters-with-different-keys).

### Security: the allow list

The exporter sends its API key to whatever node it scrapes. An open `/probe` would let anyone who can reach the exporter make it send the key to a host they control and probe internal addresses (SSRF). So a target is only scraped if it is allowed, and a target that isn't gets a 403 before any request is made (and a `WARN` in the log).

- `--typesense-host:--typesense-port` is always allowed.
- `--probe-allowed-targets` adds more, as comma-separated `host[:port]` entries, for example `typesense-1,typesense-2:9000,[::1]`. Matching is on the lowercase host and the port. The scheme is not part of the match.
- A single `*` allows any target. The exporter logs a warning at startup. Use it only when the exporter is reachable by Prometheus alone.
- With none of `--typesense-host`, `--probe-allowed-targets` and config-file clusters, the exporter refuses to start.

Bind the exporter to a private interface or network as well, and use `https`.

**Plaintext warning.** The exporter warns at startup when the default protocol is `http` and the allow list is `*` or names a non-loopback host, because `/probe` could then send the key unencrypted. It also warns the first time it probes each non-loopback node over `http`, including a target that writes `http://` itself while the default is `https`.

**No downgrade to http.** With `--typesense-protocol https`, a target that writes `http://` is refused with a 403 before any request, so a caller can't make the exporter send the key unencrypted to an allowed node.

**Proxies.** Like most HTTP clients, the exporter honours `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY`. Over `http` a proxy sees the API key; over `https` it only sees the host name. If your platform injects a proxy, add the Typesense nodes to `NO_PROXY` or use https.

**Redirects are never followed.** A redirect from a node (or from something answering in its place) counts as a failed scrape, so the API key is never sent on to another host.

## Several clusters with different keys

One exporter can watch several Typesense clusters that have different API keys. Define each cluster in the [config file](#config-file):

```toml
[clusters.prod]
targets = ["ts-prod-1:8108", "ts-prod-2:8108", "ts-prod-3:8108"]
api_key_file = "/run/secrets/typesense-prod"
protocol = "https"            # optional; defaults to typesense_protocol

[clusters.staging]
targets = ["ts-staging-1:8108"]
api_key_env = "TYPESENSE_STAGING_KEY"
```

`/probe?target=ts-prod-1:8108` finds the cluster that lists the node and scrapes it with that cluster's key and protocol. There is no extra query parameter: the cluster follows from the target, so **a key can only ever be sent to its own cluster's nodes**.

- **Key source.** Exactly one of `api_key_file` (trimmed, must not be empty) or `api_key_env` (the variable must be set and not empty) per cluster. An inline `api_key` is rejected.
- **Names and targets.** Cluster names match `[A-Za-z0-9_-]+`. Targets are `host[:port]`, parsed like `--probe-allowed-targets` (the port defaults to `typesense_port`). Cluster targets are allowed implicitly; they need no entry in `probe_allowed_targets`.
- **One home per node.** A `host:port` listed in two clusters, or in a cluster and in `probe_allowed_targets`, or equal to `typesense_host:typesense_port`, is a startup error naming both places.
- **Lookup order.** Cluster targets first, always with their cluster's key. Then `probe_allowed_targets` and `typesense_host` with the default key. A `*` in `probe_allowed_targets` applies the default key only to nodes that are in no cluster. The default key is optional when only clusters are configured.
- **https per cluster.** A cluster with `protocol = "https"` refuses `http://` targets with a 403. At startup the exporter warns about clusters with `protocol = "http"` and non-local targets, and the first probe of each such node warns again.
- **Startup.** All keys are read once at startup. A key file or variable that can't be read, or an invalid file, stops the exporter with an error naming the file and cluster, never the key.

The metrics are unchanged: they carry `host` and `port` labels only. To tell clusters apart in Prometheus, add a `cluster` label to the scrape targets:

```yaml
scrape_configs:
  - job_name: typesense-prod
    metrics_path: /probe
    static_configs:
      - targets: [ts-prod-1:8108, ts-prod-2:8108, ts-prod-3:8108]
        labels: {cluster: prod}
  - job_name: typesense-staging
    metrics_path: /probe
    static_configs:
      - targets: [ts-staging-1:8108]
        labels: {cluster: staging}
    # Both jobs use the same relabel_configs as in "Probing several nodes".
```

## API key permissions

On every scrape the exporter calls six Typesense endpoints:

| Endpoint | Used for | Scoped-key action |
|---|---|---|
| `/metrics.json` | `typesense_system_*`, `typesense_process_*` (0.4: `typesense_metrics`) | `metrics.json:list` |
| `/stats.json` | request rates, latency, cache, `typesense_route_*`, `typesense_collection_*_seconds`/`_per_second` (0.4: `typesense_stats*`) | `stats.json:list` |
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

A missing action only drops the metrics from that endpoint. The exporter logs the 401, reports `typesense_scrape_success{endpoint=...} 0` for it, and exports the rest.

**Use HTTPS when Typesense is on another machine.** With `--typesense-protocol http` and a non-local host (`--typesense-host`, or any non-local host in `--probe-allowed-targets`), the exporter logs a warning at startup, because the API key would travel unencrypted.

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

Every failure is logged at `WARN` with the endpoint and the reason, and shows up as `typesense_scrape_success{endpoint=...} 0`. A value Typesense reports that isn't a number is left out, not exported as 0.

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

That is one exporter per node. For one exporter in front of every node, use `/probe` instead; see [getting started](getting-started.md#one-exporter-for-the-whole-cluster).

### Alert rules

[`deploy/prometheus/alerts.yml`](../deploy/prometheus/alerts.yml):

| Alert | Expression | Fires after |
|---|---|---|
| `TypesenseExporterDown` | `up{job="typesense-exporter"} == 0` | 1m |
| `TypesenseUnhealthy` | `typesense_health == 0` | 1m |
| `TypesenseNoLeader` | `max(typesense_raft_state == bool 1) == 0` | 1m |
| `TypesenseScrapeFailing` | `typesense_scrape_success{endpoint!="/health"} == 0 unless on (host, port) typesense_health == 0` | 5m |

With `/probe`, `up` is per probe: the exporter answers 200 even when Typesense is down (that is `TypesenseUnhealthy`), so `TypesenseExporterDown` means the exporter is unreachable or refused the target, and it fires for every node at once when the exporter is down.

`TypesenseScrapeFailing` catches a healthy node whose endpoint the exporter can't read: usually the API key lacks an action, or the Typesense version lacks the endpoint. It stays quiet while the node is down, which `TypesenseUnhealthy` already covers.

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
| `TYPESENSE_API_KEY` | `local-dev-key` | Typesense nodes and the exporter |
| `GRAFANA_ADMIN_PASSWORD` | `admin` | Grafana admin login |

| Path | Contents |
|---|---|
| `deploy/typesense/nodes` | Cluster membership, as fixed IPs (see [architecture](architecture.md#docker-compose-stack)) |
| `deploy/prometheus/prometheus.yml` | Scrape config: one `/probe` target per node, with the 4s scrape timeout |
| `deploy/prometheus/alerts.yml` | Alert rules |
| `deploy/grafana/provisioning/` | Grafana data source and dashboard provisioning |
