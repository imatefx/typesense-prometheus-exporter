# Metrics reference

This page describes the **1.0 metric names**, which the exporter emits by default since 1.0.0. The 0.4 names (`typesense_metrics{key=…}`, `typesense_stats{key=…}`, …) are still available behind [`--metric-names`](#choosing-the-names---metric-names) until 2.0.0; the [migration guide](migrating-to-1.0.md) maps every old name to its new one.

Conventions:

- **Base units.** Latency is in seconds, sizes in bytes, shares are ratios from 0 to 1. The unit is the last part of the name (`_seconds`, `_bytes`, `_ratio`), and counters end in `_total`. Typesense reports milliseconds and percentages; the exporter converts them.
- **Every series carries `host` and `port`**: the Typesense node the exporter scrapes (`TYPESENSE_HOST`) and its API port (`TYPESENSE_PORT`). The tables below list only the extra labels.
- **Omit, don't zero.** A value Typesense doesn't report (endpoint failed, field missing, older version, not a number) is left out, never exported as 0. Only `typesense_health` and `typesense_raft_state` are always present.
- All metrics are gauges, except the two network byte counters.
- The sections up to [Scrape health](#scrape-health) describe a Typesense node. The exporter's own metrics, from a separate endpoint, are in [Exporter metrics](#exporter-metrics).

## Health and cluster state

| Metric | Type | Extra labels | Values | Source |
|---|---|---|---|---|
| `typesense_health` | gauge | | `1` if `/health` returned `{"ok": true}`, otherwise `0` (including unreachable, timeout and `503`) | `/health` |
| `typesense_raft_state` | gauge | | `1` leader, `4` follower, `0` anything else (candidate, unreachable, ...) | `/debug` |
| `typesense_info` | gauge | `version` | Always `1`; the `version` label carries the Typesense version | `/debug` |
| `typesense_raft_committed_index` | gauge | | Last raft log index committed on this node | `/status` |
| `typesense_raft_queued_writes` | gauge | | Writes queued on this node, waiting to be applied | `/status` |

`typesense_health` and `typesense_raft_state` are exported on every scrape, even when Typesense is down. The other three are left out if the endpoint fails or Typesense doesn't report the field (older versions).

```promql
# Unhealthy nodes
typesense_health == 0

# The cluster has no leader (alert after 1m: elections take a few seconds)
max(typesense_raft_state == bool 1) == 0

# Which node is leader?
typesense_raft_state == 1

# Replication lag: log entries each node is behind the most advanced node.
# Exporters are scraped a few seconds apart, so under steady writes this
# includes up to (commit rate × scrape interval) of skew; it is exact when
# writes pause. Alert on lag that keeps growing, not on a single value.
scalar(max(typesense_raft_committed_index)) - typesense_raft_committed_index

# Cluster write throughput (raft entries committed per second)
rate(typesense_raft_committed_index{host="typesense-1"}[5m])

# Nodes running a different version from the rest
count by (version) (typesense_info)
```

## System resources

From `/metrics.json`. Typesense reports each value as a string; the exporter converts it to a number. These describe the **machine** Typesense runs on.

| Metric | Type | Unit | Extra labels | Meaning |
|---|---|---|---|---|
| `typesense_system_cpu_active_ratio` | gauge | ratio 0–1 | | CPU use across all cores |
| `typesense_system_cpu_core_active_ratio` | gauge | ratio 0–1 | `core` (`"1"`, `"2"`, …) | CPU use of one core, one series per core the host has |
| `typesense_system_memory_total_bytes` | gauge | bytes | | Host memory |
| `typesense_system_memory_used_bytes` | gauge | bytes | | Host memory in use |
| `typesense_system_swap_total_bytes` | gauge | bytes | | Host swap |
| `typesense_system_swap_used_bytes` | gauge | bytes | | Host swap in use |
| `typesense_system_disk_total_bytes` | gauge | bytes | | Disk holding the Typesense data directory |
| `typesense_system_disk_used_bytes` | gauge | bytes | | Used space on that disk |
| `typesense_system_network_received_bytes_total` | **counter** | bytes | | Bytes received by the machine. Use `rate()`. Resets when the node restarts. |
| `typesense_system_network_sent_bytes_total` | **counter** | bytes | | Bytes sent by the machine. Use `rate()`. Resets when the node restarts. |

Overall CPU and per-core CPU are separate families, so `sum()` or `avg()` over cores never includes the overall figure. A negative network value (which a counter can't hold) is left out.

```promql
# Memory use per node, as a fraction
typesense_system_memory_used_bytes / typesense_system_memory_total_bytes

# Busiest core on each node
max by (host) (typesense_system_cpu_core_active_ratio)

# Network throughput in bytes per second
rate(typesense_system_network_received_bytes_total[5m])
```

## Typesense process memory

Also from `/metrics.json`: the jemalloc allocator statistics of the **Typesense process**.

| Metric | Type | Unit | Meaning |
|---|---|---|---|
| `typesense_process_memory_active_bytes` | gauge | bytes | Active pages allocated by Typesense |
| `typesense_process_memory_allocated_bytes` | gauge | bytes | Allocated by Typesense |
| `typesense_process_memory_mapped_bytes` | gauge | bytes | Mapped by the allocator |
| `typesense_process_memory_metadata_bytes` | gauge | bytes | Allocator metadata |
| `typesense_process_memory_resident_bytes` | gauge | bytes | Physically resident pages mapped by the allocator |
| `typesense_process_memory_retained_bytes` | gauge | bytes | Retained by the allocator instead of returned to the OS |
| `typesense_process_memory_fragmentation_ratio` | gauge | ratio | Allocator fragmentation, as Typesense reports it |

## Requests, latency and cache

From `/stats.json`. Typesense computes these over a short rolling window (about 10 s), and they only count requests **this node** served. When the window is idle, fields are 0 or missing.

The `operation` label is one of `search`, `import`, `write` or `delete`.

| Metric | Type | Unit | Extra labels | Meaning |
|---|---|---|---|---|
| `typesense_requests_per_second` | gauge | req/s | `operation` | Request rate per operation |
| `typesense_all_requests_per_second` | gauge | req/s | | Request rate over all routes, including ones outside the four operations (health checks, …) |
| `typesense_overloaded_requests_per_second` | gauge | req/s | | Requests rejected because the node was overloaded |
| `typesense_request_latency_avg_seconds` | gauge | seconds | `operation` | Average latency |
| `typesense_request_latency_seconds` | gauge | seconds | `operation`, `quantile` | Latency quantiles: `0` (minimum), `0.7`, `0.95`, `0.99`, `1` (maximum) |
| `typesense_pending_write_batches` | gauge | count | | Write batches waiting to be applied |
| `typesense_cache_hits` | gauge | count | | Search cache hits in the rolling window |
| `typesense_cache_misses` | gauge | count | | Search cache misses in the rolling window |
| `typesense_cache_hit_ratio` | gauge | ratio 0–1 | | Share of searches answered from the cache. Searches only use the cache with `use_cache=true`. |

`typesense_all_requests_per_second` is its own family, not `operation="total"`, so `sum by (host) (typesense_requests_per_second)` never counts it twice.

`typesense_request_latency_seconds` is a gauge, not a Prometheus summary, because Typesense provides no `_sum` or `_count`. Min and max are `quantile="0"` and `quantile="1"`, so one query draws the whole spread.

`typesense_cache_hits` and `typesense_cache_misses` are gauges, not counters: they count requests in Typesense's rolling window and disappear from the response when it is idle. Don't use `rate()` on them.

Typesense 30.x reports percentiles, min/max and cache figures; older versions report fewer fields. An operation's percentiles only appear once that operation has had traffic.

```promql
# Worst p99 search latency in the cluster, in seconds
max(typesense_request_latency_seconds{operation="search", quantile="0.99"})

# Full latency spread of searches on one node (min, p70, p95, p99, max)
typesense_request_latency_seconds{operation="search", host="typesense-1"}

# Requests per second per node, by operation
sum by (host, operation) (typesense_requests_per_second)
```

## Per route and per collection

Also from `/stats.json`.

| Metric | Type | Unit | Extra labels |
|---|---|---|---|
| `typesense_route_latency_seconds` | gauge | seconds | `route`, e.g. `"GET /collections/products/documents/search"` |
| `typesense_route_requests_per_second` | gauge | req/s | `route` |
| `typesense_collection_latency_seconds` | gauge | seconds | `method`, `collection`, `action` |
| `typesense_collection_requests_per_second` | gauge | req/s | `method`, `collection`, `action` |

The label is `route`, not `endpoint`, because `typesense_scrape_success{endpoint}` already uses `endpoint` for the Typesense API the exporter called.

Typesense names these entries after the request path, **including document and other resource IDs** (`GET /collections/products/documents/41`). The exporter replaces IDs with `:id`, so the number of series stays bounded: `GET /collections/products/documents/:id`. Entries that collapse to the same route are merged. Request rates are summed, and latency is averaged, weighted by each entry's request rate. Collection names, and the actions `search`, `import` and `export`, are kept.

The `typesense_collection_*` families cover routes under a collection, `METHOD /collections/<name>/<action>`. For example, `GET /collections/products/documents/search` gives `method="GET"`, `collection="products"`, `action="documents/search"`; a read by ID gives `action="documents/:id"`.

```promql
# Search rate per collection across the cluster
sum by (collection) (typesense_collection_requests_per_second{action="documents/search"})

# Slowest routes on a node
topk(5, typesense_route_latency_seconds{host="typesense-1"})
```

## Collections

| Metric | Type | Extra labels | Source |
|---|---|---|---|
| `typesense_collection_documents` | gauge | `collection` | `/collections` |

The number of documents in each collection, as seen by this node. In a healthy cluster every node reports the same counts.

```promql
# Documents per collection
max by (collection) (typesense_collection_documents)

# Collections that grew by more than 10k documents in the last hour
delta(typesense_collection_documents{host="typesense-1"}[1h]) > 10000
```

## Scrape health

| Metric | Type | Extra labels | Values |
|---|---|---|---|
| `typesense_scrape_success` | gauge | `endpoint` | `1` if the exporter fetched and parsed that Typesense endpoint (`/metrics.json`, `/stats.json`, `/health`, `/debug`, `/status`, `/collections`) on this scrape, otherwise `0` |
| `typesense_scrape_duration_seconds` | gauge | | Time taken to query all six endpoints |

```promql
# Endpoints the exporter can't read on a healthy node (permissions, version)
typesense_scrape_success{endpoint!="/health"} == 0 unless on (host, port) typesense_health == 0
```

## Fields without a name: `typesense_unmapped`

| Metric | Type | Extra labels | Source |
|---|---|---|---|
| `typesense_unmapped` | gauge | `source` (`metrics.json` or `stats.json`), `key` (the Typesense field) | `/metrics.json`, `/stats.json` |

A numeric field Typesense reports that the exporter has no 1.0 name for (for example, one added in a newer Typesense release) is exported here with its **raw value and no unit conversion**. The family only appears when there is such a field. Don't build dashboards or alerts on it: when a field shows up here, it gets a proper name in the next exporter release.

```promql
# Fields this exporter version doesn't know yet
count by (source, key) (typesense_unmapped)
```

## Exporter metrics

The exporter's own health, served at **`GET /exporter/metrics`** (Prometheus text format), apart from every Typesense node's output. It is the same with `/metrics` and `/probe`, like the `/metrics` of `blackbox_exporter` itself, so scrape it as a separate job; see [Prometheus](configuration.md#prometheus).

These series are not about a Typesense node, so none carries `host` or `port`, and none is affected by `--metric-names`. Counters and histograms live as long as the exporter process and add up over all scrapes and all targets; they restart from 0 when the exporter restarts (use `rate()` or `increase()`). Every label value comes from a fixed set, a Typesense path, or a cluster name in the config file: a refused or malformed `/probe` target is counted but never becomes a label value, so a caller can't grow the number of series.

| Metric | Type | Extra labels | Meaning |
|---|---|---|---|
| `typesense_exporter_build_info` | gauge | `version` | Always `1`; `version` is the exporter's version |
| `typesense_exporter_scrapes_total` | counter | `handler` (`metrics`, `probe`), `result` | Scrapes of `/metrics` and `/probe` by outcome. `result` is `ok` (HTTP 200, even when Typesense is down), `refused` (`/probe` 403: target not allowed), `invalid` (`/probe` 400: missing or malformed target) or `not_found` (`/metrics` 404: no `--typesense-host`) |
| `typesense_exporter_typesense_requests_total` | counter | `endpoint`, `result` | Requests to Typesense, summed over all targets. `endpoint` is one of `/metrics.json`, `/stats.json`, `/health`, `/debug`, `/status`, `/collections`; `result` is `ok`, `timeout` (hit `--typesense-timeout`) or `error` (connection error, non-2xx status, unparsable body) |
| `typesense_exporter_typesense_request_duration_seconds` | histogram | `endpoint` | How long those requests took, timeouts included |
| `typesense_exporter_probes_in_flight` | gauge | | `/probe` scrapes running now (at most `--probe-max-concurrency`) |
| `typesense_exporter_probe_slot_wait_seconds` | histogram | | Time a `/probe` waited for a `--probe-max-concurrency` slot before it could start; near 0 unless probes queue |
| `typesense_exporter_key_reloads_total` | counter | `key`, `result` (`ok`, `failed`) | Attempts to re-read an API key file after it changed |
| `typesense_exporter_key_reload_failing` | gauge | `key` | `1` while the latest reload attempt of that key file failed (the previous key is still in use), else `0` |

`key` is `default` for `--typesense-api-key-file` and the cluster's name for a cluster's `api_key_file`. Both key metrics exist only for keys read from a file: a key from a flag or environment variable never changes, so it has no series. A failing file is retried at most once per second while the exporter is scraped, so `failed` counts attempts, not rotations.

`typesense_exporter_scrapes_total` and the `*_requests_total` series start at 0 when the exporter starts, so `increase()` sees the first event. The histograms use the buckets 5ms to 30s for requests and 1ms to 30s for slot waits.

```promql
# Probes the exporter refused or rejected, per minute
sum by (instance) (rate(typesense_exporter_scrapes_total{handler="probe",result=~"refused|invalid"}[5m])) * 60

# Typesense requests that failed, by endpoint
sum by (endpoint) (rate(typesense_exporter_typesense_requests_total{result!="ok"}[5m]))

# p95 duration of /stats.json across all targets
histogram_quantile(0.95, sum by (le) (rate(typesense_exporter_typesense_request_duration_seconds_bucket{endpoint="/stats.json"}[5m])))

# p95 wait for a probe slot: raise --probe-max-concurrency if this stays above 1s
histogram_quantile(0.95, sum by (le) (rate(typesense_exporter_probe_slot_wait_seconds_bucket[10m])))

# Key files the exporter can't read after a rotation
typesense_exporter_key_reload_failing == 1
```

## Choosing the names: `--metric-names`

`--metric-names` / `TYPESENSE_METRIC_NAMES` selects which names the `/metrics.json` and `/stats.json` values are exported under. Every other family on this page (health, raft, info, collections, scrape health) is the same in every mode and appears exactly once.

| Value | Exports |
|---|---|
| `v1` *(default)* | The 1.0 names on this page, including `typesense_unmapped` |
| `both` | The 1.0 names and the 0.4 names side by side, while you move queries over |
| `legacy` | Only the 0.4 names, as 0.4.x exported them. No `typesense_unmapped`: unknown `/stats.json` fields appear as `typesense_stats{key=…}` as before. |

`legacy` and `both` are deprecated and will be removed in 2.0.0. The 0.4 families are `typesense_metrics{key}`, `typesense_stats{key}`, `typesense_stats_latency_ms{key}`, `typesense_stats_requests_per_second{key}`, `typesense_stats_latency_ms_by_collection{method,collection_name,action}` and `typesense_stats_requests_per_second_by_collection{method,collection_name,action}`. For the old name of every 1.0 metric, see the [migration guide](migrating-to-1.0.md#name-mapping). The reasoning behind the names is in the [design note](metrics-v1.md).
