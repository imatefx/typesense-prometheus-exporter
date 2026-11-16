# Metric naming for 1.0 (design)

Status: **implemented in 0.5.0**. 0.5.0 exports these names by default; see the [migration guide](migrating-to-1.0.md) to move dashboards and queries over, and the [metrics reference](metrics.md) for the current list.

## Why rename

Up to 0.4.x most values come out of two catch-all families:

- `typesense_metrics{key="…"}` mixes bytes, percentages and ratios.
- `typesense_stats{key="…"}` mixes milliseconds, requests per second, ratios and counts.

That causes three problems:

1. **Units are hidden in a label value.** You can't tell the unit from the metric name, and no two series share one. Prometheus convention is base units (seconds, bytes, ratios 0–1) with the unit as a name suffix.
2. **Types are wrong.** Network bytes are cumulative counters but are exported as gauges, so `rate()` is technically misuse and counter resets after a restart aren't handled.
3. **Queries are awkward.** Every panel filters on `key=~"…"`, and summing `typesense_metrics` across keys gives nonsense.

## What was checked against a live Typesense 30.1

| Value | Behaviour observed | Type in 1.0 |
|---|---|---|
| `system_network_received_bytes`, `system_network_sent_bytes` | Only increase (19 085 → 130 213 over 70 s, with or without traffic). Reset when the node restarts. | **counter** |
| `cache_hit_count`, `cache_miss_count` | Count requests in Typesense's rolling stats window (~10 s). They rise during traffic and **disappear** from the response when the window is idle. | gauge |
| `*_requests_per_second`, `*_latency_ms`, percentiles | Computed over the same rolling window. Absent or 0 when idle. | gauge |
| Everything in `/metrics.json` other than network | Point-in-time values. | gauge |

So network bytes are the only true counters Typesense exposes.

## Conventions

- Base units: latency in **seconds** (Typesense reports ms, the exporter divides by 1000), sizes in **bytes**, percentages as **ratios** 0–1 (divides by 100).
- The unit is a name suffix (`_seconds`, `_bytes`, `_ratio`). Counters end in `_total`.
- An operation is a **label**, not part of the name, so one query covers search, import, write and delete.
- Labels: every series keeps `host` and `port`. The collection label is `collection` everywhere (it's `collection_name` in 0.4).
- "Omit, don't zero" is unchanged: a value Typesense doesn't send isn't exported.

## Name mapping

### `/stats.json`: request rates, latency, cache

Operations are `search`, `import`, `write` and `delete`.

| 0.4 name | 1.0 name | Type |
|---|---|---|
| `typesense_stats{key="<op>_requests_per_second"}` | `typesense_requests_per_second{operation="<op>"}` | gauge |
| `typesense_stats{key="total_requests_per_second"}` | `typesense_all_requests_per_second` | gauge |
| `typesense_stats{key="overloaded_requests_per_second"}` | `typesense_overloaded_requests_per_second` | gauge |
| `typesense_stats{key="<op>_latency_ms"}` | `typesense_request_latency_avg_seconds{operation="<op>"}` | gauge |
| `typesense_stats{key="<op>_70Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.7"}` | gauge |
| `typesense_stats{key="<op>_95Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.95"}` | gauge |
| `typesense_stats{key="<op>_99Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0.99"}` | gauge |
| `typesense_stats{key="<op>_min_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="0"}` | gauge |
| `typesense_stats{key="<op>_max_latency_ms"}` | `typesense_request_latency_seconds{operation="<op>",quantile="1"}` | gauge |
| `typesense_stats{key="pending_write_batches"}` | `typesense_pending_write_batches` | gauge |
| `typesense_stats{key="cache_hit_count"}` | `typesense_cache_hits` | gauge (rolling window) |
| `typesense_stats{key="cache_miss_count"}` | `typesense_cache_misses` | gauge (rolling window) |
| `typesense_stats{key="cache_hit_ratio"}` | `typesense_cache_hit_ratio` | gauge |

`total_requests_per_second` gets its own family, not `operation="total"`. It includes requests outside the four operations, such as health checks, so `sum by (host) (typesense_requests_per_second)` must not count it twice.

Min and max become `quantile="0"` and `quantile="1"`, which is what they are mathematically. One query then draws the whole latency spread: `typesense_request_latency_seconds{operation="search"}`. The quantile family is a gauge, not a Prometheus summary, because Typesense provides no `_sum` or `_count`.

### `/stats.json`: per-route breakdown

| 0.4 name | 1.0 name | Type |
|---|---|---|
| `typesense_stats_latency_ms{key="<route>"}` | `typesense_route_latency_seconds{route="<route>"}` | gauge |
| `typesense_stats_requests_per_second{key="<route>"}` | `typesense_route_requests_per_second{route="<route>"}` | gauge |
| `typesense_stats_latency_ms_by_collection{method,collection_name,action}` | `typesense_collection_latency_seconds{method,collection,action}` | gauge |
| `typesense_stats_requests_per_second_by_collection{method,collection_name,action}` | `typesense_collection_requests_per_second{method,collection,action}` | gauge |

The label is `route`, not `endpoint`, because `typesense_scrape_success{endpoint}` already uses `endpoint` for the Typesense API the exporter called. Route normalization (IDs become `:id`, merged routes add up their rates and weight latency by rate) is unchanged.

### `/metrics.json`: system and process resources

| 0.4 name | 1.0 name | Type |
|---|---|---|
| `typesense_metrics{key="system_cpu_active_percentage"}` | `typesense_system_cpu_active_ratio` | gauge |
| `typesense_metrics{key="system_cpu<N>_active_percentage"}` | `typesense_system_cpu_core_active_ratio{core="<N>"}` | gauge |
| `typesense_metrics{key="system_memory_total_bytes"}` | `typesense_system_memory_total_bytes` | gauge |
| `typesense_metrics{key="system_memory_used_bytes"}` | `typesense_system_memory_used_bytes` | gauge |
| `typesense_metrics{key="system_memory_total_swap_bytes"}` | `typesense_system_swap_total_bytes` | gauge |
| `typesense_metrics{key="system_memory_used_swap_bytes"}` | `typesense_system_swap_used_bytes` | gauge |
| `typesense_metrics{key="system_disk_total_bytes"}` | `typesense_system_disk_total_bytes` | gauge |
| `typesense_metrics{key="system_disk_used_bytes"}` | `typesense_system_disk_used_bytes` | gauge |
| `typesense_metrics{key="system_network_received_bytes"}` | `typesense_system_network_received_bytes_total` | **counter** |
| `typesense_metrics{key="system_network_sent_bytes"}` | `typesense_system_network_sent_bytes_total` | **counter** |
| `typesense_metrics{key="typesense_memory_<kind>_bytes"}` | `typesense_process_memory_<kind>_bytes` | gauge |
| `typesense_metrics{key="typesense_memory_fragmentation_ratio"}` | `typesense_process_memory_fragmentation_ratio` | gauge |

`<kind>` is one of `active`, `allocated`, `mapped`, `metadata`, `resident` or `retained` (jemalloc statistics). The `process_` prefix avoids the 0.4-era `typesense_metrics{key="typesense_memory_…"}` stutter and separates Typesense's own memory from the machine's.

Overall CPU and per-core CPU are separate families, so `sum()` over cores never includes the overall figure.

### New fields from future Typesense versions

The catch-all families had one advantage: a field added in a new Typesense release showed up without an exporter release. To keep that, numeric fields the exporter doesn't recognise are exported as

```
typesense_unmapped{source="metrics.json|stats.json",key="<field>"}
```

with the raw value and no unit conversion. Dashboards shouldn't depend on it. When a field appears there, it gets a proper name in the next exporter release.

### Unchanged

These already follow the conventions and keep their names:

`typesense_health`, `typesense_raft_state`, `typesense_info{version}`, `typesense_raft_committed_index`, `typesense_raft_queued_writes`, `typesense_collection_documents{collection}`, `typesense_scrape_success{endpoint}`, `typesense_scrape_duration_seconds`.

The four alert rules use only these, so they need no change.

## Migration path

| Release | Old names (0.4) | New names (1.0) | Default |
|---|---|---|---|
| **0.5.0** | opt-in (`legacy` or `both`) | emitted | `v1` |
| **1.0.0** | removed | emitted | — |

- **0.5.0** adds `--metric-names` / `TYPESENSE_METRIC_NAMES` with the values `v1` (default), `both` or `legacy`. The default is the new names only, so **upgrading to 0.5.0 breaks dashboards and queries that use the old names** unless you set `both` (old and new together while you move queries over) or `legacy` (old names only, to put the migration off). The release notes and the migration guide lead with this.
- The bundled Grafana dashboards switch to the new names in 0.5.0. The `docker-compose.yml` stack runs with `v1`, so the e2e tests prove the dashboards don't depend on old names.
- A [migration guide](migrating-to-1.0.md) gives a before/after PromQL table for every family. Examples:

  | 0.4 query | 1.0 query |
  |---|---|
  | `typesense_stats{key="search_95Percentile_latency_ms"}` | `typesense_request_latency_seconds{operation="search",quantile="0.95"} * 1000` (or change the panel unit to seconds) |
  | `rate(typesense_metrics{key="system_network_received_bytes"}[5m])` | `rate(typesense_system_network_received_bytes_total[5m])` |
  | `typesense_metrics{key="system_cpu_active_percentage"}` | `typesense_system_cpu_active_ratio * 100` |
  | `typesense_stats_requests_per_second_by_collection{collection_name="products"}` | `typesense_collection_requests_per_second{collection="products"}` |

- **1.0.0** removes the old names and the `legacy` and `both` values. `--metric-names=v1` is still accepted and does nothing, so 0.5 configurations that set it keep starting.
- Anyone who needs old names after 1.0 can rebuild them with Prometheus recording rules. The [migration guide](migrating-to-1.0.md#recording-rules-for-the-old-names) includes a ready-made rules file.

## Several Typesense nodes from one exporter

Today it's one exporter per node, which is why the compose stack runs three. 1.0 should allow one exporter to cover a whole cluster. That change is additive, so it can ship in 1.x, but the label decisions have to be right in 1.0.

**Approach: the Prometheus multi-target pattern**, as used by `blackbox_exporter` and `snmp_exporter`:

- A new `/probe?target=node1:8108` endpoint scrapes the node it's given. Prometheus lists the nodes and relabels `__param_target` into `instance`.
- Each node is a separate Prometheus scrape, so a hung node only times out its own scrape. `typesense_scrape_success`, `up` and `scrape_duration` stay per node, and the alerts work unchanged.
- `/metrics` keeps working for the single node set with `--typesense-host`, so existing setups don't change.
- One API key covers every node by default. Per-node keys can come later from a small config file, the way `blackbox_exporter` uses modules.
- Optionally, `--allowed-targets` limits which hosts `/probe` will contact, so the exporter can't be used to reach arbitrary hosts.

The alternative, `--typesense-host a,b,c` with every node in one `/metrics` response, is simpler. But the slowest node then delays the whole response, and one Prometheus `up` covers the whole cluster, so the per-node alerts get weaker. It's not recommended.

**What 1.0 has to guarantee for this:** every series keeps the `host` and `port` labels naming the Typesense node it describes, not the exporter. The mapping above already does this. Typesense has no API that lists cluster peers, so nodes are listed in Prometheus, not discovered by the exporter.

## Implementation notes

- The exporter builds a new registry on every scrape, so counters are created with `inc_by(value)` on a fresh `CounterVec`. No extra state is needed.
- Unit conversion happens at export time. The models keep Typesense's raw values.
- Tests: unit tests for every mapping row and the `unmapped` fallback; integration tests for each `--metric-names` value; the e2e tests and dashboards on `v1`.
