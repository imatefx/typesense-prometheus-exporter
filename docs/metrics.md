# Metrics reference

All metrics are gauges. Every series carries:
- `host`: the Typesense node the exporter scrapes (`TYPESENSE_HOST`)
- `port`: its API port (`TYPESENSE_PORT`)

## Health and cluster state

| Metric | Values | Source |
|---|---|---|
| `typesense_health` | `1` if `/health` returned `{"ok": true}`, otherwise `0` (including unreachable, timeout and `503`) | `/health` |
| `typesense_raft_state` | `1` leader, `4` follower, `0` anything else (candidate, unreachable, ...) | `/debug` |

Both are exported on every scrape, even when Typesense is down.

```promql
# Unhealthy nodes
typesense_health == 0

# The cluster has no leader (alert after 1m: elections take a few seconds)
max(typesense_raft_state == bool 1) == 0

# Which node is leader?
typesense_raft_state == 1
```

## System and memory: `typesense_metrics{key=...}`

From `/metrics.json`. Typesense reports each value as a string, and the exporter converts it to a number.

| `key` | Unit | Meaning |
|---|---|---|
| `system_cpu_active_percentage` | % | CPU use across all cores |
| `system_cpu<N>_active_percentage` | % | CPU use of core N, one series per core the host has |
| `system_disk_total_bytes`, `system_disk_used_bytes` | bytes | Disk of the Typesense data directory |
| `system_memory_total_bytes`, `system_memory_used_bytes` | bytes | Host memory |
| `system_memory_total_swap_bytes`, `system_memory_used_swap_bytes` | bytes | Host swap |
| `system_network_received_bytes`, `system_network_sent_bytes` | bytes | Network counters (cumulative; use `rate()`) |
| `typesense_memory_active_bytes` | bytes | Allocator: active memory |
| `typesense_memory_allocated_bytes` | bytes | Allocator: allocated memory |
| `typesense_memory_fragmentation_ratio` | ratio | Allocator fragmentation |
| `typesense_memory_mapped_bytes` | bytes | Allocator: mapped memory |
| `typesense_memory_metadata_bytes` | bytes | Allocator: metadata |
| `typesense_memory_resident_bytes` | bytes | Allocator: resident memory |
| `typesense_memory_retained_bytes` | bytes | Allocator: retained memory |

A key that Typesense doesn't report is not exported.

```promql
# Memory use per node, as a fraction
typesense_metrics{key="system_memory_used_bytes"} / ignoring(key) typesense_metrics{key="system_memory_total_bytes"}

# Busiest core on each node
max by (host) (typesense_metrics{key=~"system_cpu[0-9]+_active_percentage"})
```

## Request stats: `typesense_stats{key=...}`

From `/stats.json`. Typesense computes these over a short rolling window, and they only count requests **this node** served.

| `key` | Unit |
|---|---|
| `total_requests_per_second` | req/s |
| `search_requests_per_second`, `search_latency_ms` | req/s, ms |
| `write_requests_per_second`, `write_latency_ms` | req/s, ms |
| `import_requests_per_second`, `import_latency_ms` | req/s, ms |
| `delete_requests_per_second`, `delete_latency_ms` | req/s, ms |
| `overloaded_requests_per_second` | req/s |
| `pending_write_batches` | count |
| `cache_hit_ratio` | ratio, 0–1. Typesense v28+ only; absent on older versions. Searches only use the cache with `use_cache=true`. |

## Per endpoint and per collection

| Metric | Extra labels |
|---|---|
| `typesense_stats_latency_ms` | `key`, e.g. `"GET /collections/products/documents/search"` |
| `typesense_stats_requests_per_second` | `key` |
| `typesense_stats_latency_ms_by_collection` | `key`, `method`, `collection_name`, `action` |
| `typesense_stats_requests_per_second_by_collection` | `key`, `method`, `collection_name`, `action` |

The `_by_collection` variants only include endpoints of the form `METHOD /collections/<name>/<a>/<b>`. For example, `GET /collections/products/documents/search` gives `method="GET"`, `collection_name="products"`, `action="documents/search"`.

```promql
# Search rate per collection across the cluster
sum by (collection_name) (typesense_stats_requests_per_second_by_collection{action="documents/search"})
```
