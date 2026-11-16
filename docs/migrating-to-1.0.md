# Migrating to the 1.0 metric names

> [!WARNING]
> **1.0.0 changes the default metric names.** Out of the box, 1.0.0 exports only the new 1.0 names. Dashboards, alerts and queries that use `typesense_metrics{key=…}`, `typesense_stats{key=…}` or the `typesense_stats_*` families **stop returning data as soon as you upgrade**, unless you keep the old names as shown below.

## Keep the old names while you migrate

Set `--metric-names` (or `TYPESENSE_METRIC_NAMES`) on every exporter:

| Value | Exports | Use it to |
|---|---|---|
| `v1` *(default)* | 1.0 names only | Run on the new names |
| `both` | 1.0 names **and** 0.4 names | Upgrade without breaking anything, then move queries over one by one |
| `legacy` | 0.4 names only, exactly as 0.4.x | Put the migration off |

```bash
docker run -d -p 8888:8888 \
  -e TYPESENSE_HOST=typesense-1.internal \
  -e TYPESENSE_API_KEY=<admin-api-key> \
  -e TYPESENSE_METRIC_NAMES=both \
  imatefx/typesense-prometheus-exporter:v1.0.0
```

Families that don't come from `/metrics.json` or `/stats.json` keep their names and appear once in every mode: `typesense_health`, `typesense_raft_state`, `typesense_info`, `typesense_raft_committed_index`, `typesense_raft_queued_writes`, `typesense_collection_documents`, `typesense_scrape_success` and `typesense_scrape_duration_seconds`. **The bundled alert rules use only these, so they keep working in every mode.**

`both` roughly doubles the series from those two endpoints for the migration window. `legacy` and `both` are deprecated: the exporter logs a warning while they are set, and they will be removed in 2.0.0, where `--metric-names=v1` is still accepted and does nothing. After that, the [recording rules](#recording-rules-for-the-old-names) below rebuild the old names inside Prometheus.

A suggested path:

1. Upgrade to 1.0.0 with `TYPESENSE_METRIC_NAMES=both`.
2. Rewrite dashboards and alerts with the table below. While both name sets are exported, you can compare old and new panels side by side.
3. Switch to `v1` (or remove the setting). Load the recording rules if something you can't change still needs the old names.

## What changed

- **Units are in the name and in base units.** Latency is in seconds (Typesense reports ms; the exporter divides by 1000). CPU and cache shares are ratios from 0 to 1 (CPU percentages are divided by 100). Bytes are unchanged.
- **One family per value**, instead of catch-all families with a `key` label.
- **Operations are a label:** `operation="search|import|write|delete"`. Latency percentiles, min and max are one family with a `quantile` label: `0` (min), `0.7`, `0.95`, `0.99`, `1` (max).
- **Network bytes are counters** (`…_bytes_total`). `rate()` now handles node restarts correctly.
- **Labels renamed:** `collection_name` is now `collection`; the per-route `key` label is now `route`.
- **New Typesense fields** that the exporter has no name for appear as `typesense_unmapped{source,key}` with their raw value, instead of in `typesense_stats{key=…}`. Don't build on it; they get a proper name in a later release.
- "Omit, don't zero" is unchanged: a value Typesense doesn't send is not exported.

## Name mapping

`<op>` is `search`, `import`, `write` or `delete`. Every series still has `host` and `port`.

### Request rates, latency and cache (`/stats.json`)

| 0.4 query | 1.0 query | Unit change |
|---|---|---|
| `typesense_stats{key="<op>_requests_per_second"}` | `typesense_requests_per_second{operation="<op>"}` | none |
| `typesense_stats{key="total_requests_per_second"}` | `typesense_all_requests_per_second` | none |
| `typesense_stats{key="overloaded_requests_per_second"}` | `typesense_overloaded_requests_per_second` | none |
| `typesense_stats{key="<op>_latency_ms"}` | `typesense_request_latency_avg_seconds{operation="<op>"}` | ms → s |
| `typesense_stats{key="<op>_min_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0"}` | ms → s |
| `typesense_stats{key="<op>_70Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.7"}` | ms → s |
| `typesense_stats{key="<op>_95Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.95"}` | ms → s |
| `typesense_stats{key="<op>_99Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.99"}` | ms → s |
| `typesense_stats{key="<op>_max_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="1"}` | ms → s |
| `typesense_stats{key="pending_write_batches"}` | `typesense_pending_write_batches` | none |
| `typesense_stats{key="cache_hit_count"}` | `typesense_cache_hits` | none |
| `typesense_stats{key="cache_miss_count"}` | `typesense_cache_misses` | none |
| `typesense_stats{key="cache_hit_ratio"}` | `typesense_cache_hit_ratio` | none (already 0–1) |
| `typesense_stats{key="<any other field>"}` | `typesense_unmapped{source="stats.json",key="<field>"}` | none (raw) |

### Per route and per collection (`/stats.json`)

| 0.4 query | 1.0 query | Unit change |
|---|---|---|
| `typesense_stats_latency_ms{key="<route>"}` | `typesense_route_latency_seconds{route="<route>"}` | ms → s |
| `typesense_stats_requests_per_second{key="<route>"}` | `typesense_route_requests_per_second{route="<route>"}` | none |
| `typesense_stats_latency_ms_by_collection{method,collection_name,action}` | `typesense_collection_latency_seconds{method,collection,action}` | ms → s |
| `typesense_stats_requests_per_second_by_collection{method,collection_name,action}` | `typesense_collection_requests_per_second{method,collection,action}` | none |

Route normalization (`…/documents/:id`, merged routes) is unchanged.

### System and process resources (`/metrics.json`)

| 0.4 query | 1.0 query | Unit change |
|---|---|---|
| `typesense_metrics{key="system_cpu_active_percentage"}` | `typesense_system_cpu_active_ratio` | % → 0–1 |
| `typesense_metrics{key="system_cpu<N>_active_percentage"}` | `typesense_system_cpu_core_active_ratio{core="<N>"}` | % → 0–1 |
| `typesense_metrics{key="system_memory_total_bytes"}` | `typesense_system_memory_total_bytes` | none |
| `typesense_metrics{key="system_memory_used_bytes"}` | `typesense_system_memory_used_bytes` | none |
| `typesense_metrics{key="system_memory_total_swap_bytes"}` | `typesense_system_swap_total_bytes` | none |
| `typesense_metrics{key="system_memory_used_swap_bytes"}` | `typesense_system_swap_used_bytes` | none |
| `typesense_metrics{key="system_disk_total_bytes"}` | `typesense_system_disk_total_bytes` | none |
| `typesense_metrics{key="system_disk_used_bytes"}` | `typesense_system_disk_used_bytes` | none |
| `typesense_metrics{key="system_network_received_bytes"}` | `typesense_system_network_received_bytes_total` | none; now a counter |
| `typesense_metrics{key="system_network_sent_bytes"}` | `typesense_system_network_sent_bytes_total` | none; now a counter |
| `typesense_metrics{key="typesense_memory_active_bytes"}` | `typesense_process_memory_active_bytes` | none |
| `typesense_metrics{key="typesense_memory_allocated_bytes"}` | `typesense_process_memory_allocated_bytes` | none |
| `typesense_metrics{key="typesense_memory_mapped_bytes"}` | `typesense_process_memory_mapped_bytes` | none |
| `typesense_metrics{key="typesense_memory_metadata_bytes"}` | `typesense_process_memory_metadata_bytes` | none |
| `typesense_metrics{key="typesense_memory_resident_bytes"}` | `typesense_process_memory_resident_bytes` | none |
| `typesense_metrics{key="typesense_memory_retained_bytes"}` | `typesense_process_memory_retained_bytes` | none |
| `typesense_metrics{key="typesense_memory_fragmentation_ratio"}` | `typesense_process_memory_fragmentation_ratio` | none |

### Unchanged

`typesense_health`, `typesense_raft_state`, `typesense_info{version}`, `typesense_raft_committed_index`, `typesense_raft_queued_writes`, `typesense_collection_documents{collection}`, `typesense_scrape_success{endpoint}`, `typesense_scrape_duration_seconds`.

## Rewriting common queries

| 0.4 query | 1.0 query |
|---|---|
| `typesense_stats{key="search_95Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="search",quantile="0.95"}` (set the panel unit to seconds), or `… * 1000` to keep milliseconds |
| `typesense_stats{key=~"search_(95\|99)Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="search",quantile=~"0.95\|0.99"}` |
| `typesense_stats{key=~".*_requests_per_second", key!="total_requests_per_second"}` | `typesense_requests_per_second` |
| `max(typesense_stats{key="search_99Percentile_latency_ms"})` | `max(typesense_request_latency_seconds{operation="search",quantile="0.99"})` |
| `rate(typesense_metrics{key="system_network_received_bytes"}[5m])` | `rate(typesense_system_network_received_bytes_total[5m])` |
| `typesense_metrics{key="system_cpu_active_percentage"}` | `typesense_system_cpu_active_ratio * 100`, or the ratio with a 0–1 percent unit |
| `max by (host) (typesense_metrics{key=~"system_cpu[0-9]+_active_percentage"})` | `max by (host) (typesense_system_cpu_core_active_ratio)` |
| `typesense_metrics{key="system_memory_used_bytes"} / ignoring(key) typesense_metrics{key="system_memory_total_bytes"}` | `typesense_system_memory_used_bytes / typesense_system_memory_total_bytes` |
| `typesense_stats_requests_per_second_by_collection{collection_name="products"}` | `typesense_collection_requests_per_second{collection="products"}` |
| `sum by (collection_name) (typesense_stats_requests_per_second_by_collection{action="documents/search"})` | `sum by (collection) (typesense_collection_requests_per_second{action="documents/search"})` |
| `typesense_stats_latency_ms{key="GET /health"}` | `typesense_route_latency_seconds{route="GET /health"}` |

In Grafana legends, replace `{{key}}` with `{{operation}}`, `{{quantile}}`, `{{core}}`, `{{route}}` or `{{collection}}` as appropriate.

## Grafana panel units

The new names are in base units, so most panels only need a different unit, not a `* 1000` or `* 100` in the query.

| Values | Old panel unit | New panel unit (Grafana unit id) |
|---|---|---|
| Latency (`*_seconds`) | Time / milliseconds (`ms`) | Time / seconds (`s`). Grafana still shows small values as "1.5 ms". |
| CPU and cache ratios (`*_ratio`) | Misc / Percent (0-100) (`percent`) | Misc / Percent (0.0-1.0) (`percentunit`). Set the axis max to `1`, not `100`. |
| Bytes (`*_bytes`) | Data / bytes (`bytes`) | unchanged |
| Network rate (`rate(*_bytes_total[…])`) | Data rate / bytes/sec (`Bps`) | unchanged |
| Requests per second | requests/sec (`reqps`) | unchanged |

Also update thresholds: a 100 ms latency threshold becomes `0.1`, an 80% CPU threshold becomes `0.8`.

## Recording rules for the old names

If something you can't change still needs the 0.4 names (an external dashboard, a long-term-storage query, a team that migrates later), let Prometheus rebuild them from the 1.0 names. This file recreates every 0.4 series except unknown `/stats.json` fields, with the same names, labels and units (milliseconds and percentages).

Load it **only while the exporters run with `v1`**. With `both` or `legacy` the exporters already emit these series, and the recorded copies would collide with them.

The rules ship as [`deploy/prometheus/legacy-names.rules.yml`](../deploy/prometheus/legacy-names.rules.yml) (22 rules). CI checks them with `promtool` and runs the rule tests in [`legacy-names.test.yml`](../deploy/prometheus/legacy-names.test.yml). For example, the search p95 rule reads:

```yaml
- record: typesense_stats
  expr: |
    max without (operation, quantile) (
      label_replace(typesense_request_latency_seconds{quantile="0.95"} * 1000, "key", "${1}_95Percentile_latency_ms", "operation", "(.+)")
      or ...
    )
```

Copy the file next to your `prometheus.yml`, add it to `rule_files`, and check it first:

```yaml
rule_files:
  - legacy-names.rules.yml
```

```bash
promtool check rules legacy-names.rules.yml
# or, without a local Prometheus install:
docker run --rm -v "$PWD":/r -w /r --entrypoint promtool prom/prometheus check rules legacy-names.rules.yml
```

Notes:

- The recorded series are written at Prometheus' rule evaluation interval, so they can lag the raw series by one interval.
- They have no history from before the rules were loaded. Queries over older ranges only find the series the 0.4 exporter wrote.
- The recorded network series hold the counter value, so `rate()` on them works as before.
- Recording rules add series to Prometheus. Remove the rules you don't need, and the file once nothing uses the old names.

## See also

- [Metrics reference](metrics.md): every 1.0 family with its type, unit and labels
- [Design note](metrics-v1.md): why the names changed
- [Configuration](configuration.md): `--metric-names` and the other options
