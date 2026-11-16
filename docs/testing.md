# Testing

Three layers, from fast and isolated to slow and real:

| Layer | Where | Runs against | Count | Time | Needs |
|---|---|---|---|---|---|
| Unit | `src/**` (`#[cfg(test)]`) | Plain functions | 21 | < 1s | Rust |
| Integration | [`tests/exporter.rs`](../tests/exporter.rs) | The real HTTP server and a mock Typesense ([wiremock](https://docs.rs/wiremock)) | 15 | ~1s | Rust |
| End-to-end | [`tests/e2e.rs`](../tests/e2e.rs) | A real 3-node Typesense cluster, exporters, Prometheus and Grafana in Docker | 8 | ~3–5 min | Rust, Docker |

```bash
make test               # lint + unit + integration
make test-unit          # cargo test --lib
make test-integration   # cargo test --test exporter
make coverage           # unit + integration coverage, fails under 90% of lines
make coverage-html      # HTML coverage report, opens in a browser
make e2e                # starts the stack, runs the e2e tests, tears the stack down
```

A plain `cargo test` runs unit and integration tests only. The e2e tests sit behind the `e2e` cargo feature, so they never need Docker by accident.

## Unit tests

### Metric rendering: `src/prometheus_exp.rs`

| Test | Verifies |
|---|---|
| `parses_collection_endpoint` | `GET /collections/books/documents/search` splits into method, collection and action; `GET /health` doesn't match |
| `exports_all_metric_families` | A full scrape renders every family with the right labels. Per-core CPU covers any core count (core 16). Unnumbered keys, unreported keys and unknown fields are left out. Swap, cache hit ratio, health and raft state are present. |
| `failed_endpoints_are_omitted_not_zeroed` | With nothing fetched, `typesense_metrics` and `typesense_stats` are absent; health and raft state are 0 |
| `unknown_raft_states_are_reported_as_zero` | Raft state 1 → 1 and 4 → 4; any other state → 0 |
| `unparseable_values_are_exported_as_zero` | A reported but non-numeric value (`"n/a"`) becomes 0, and a per-collection string value still gets its labels |
| `non_collection_paths_skip_by_collection_metrics` | `/health`, `/collections` and `/collections/books` give no per-collection labels; `DELETE …/documents/42` does |
| `output_has_help_and_type_lines` | Output has `# HELP` and `# TYPE … gauge` lines |
| `exports_version_replication_and_collection_documents` | `typesense_info{version}`, committed index, queued writes and per-collection document counts are rendered; collections without a count are skipped |
| `new_families_are_omitted_when_their_endpoint_failed` | No `/debug`, `/status` or `/collections` data means no info, raft index, queued writes or document metrics |
| `status_fields_missing_from_older_typesense_are_omitted` | A `/status` without `committed_index` or `queued_writes` doesn't produce fake zeros |
| `exports_latency_percentiles_and_cache_counts` | p95/p99, max latency and cache hit/miss counts are exported as `typesense_stats` keys |

### Models: `src/typesense/models/`

| Test | Verifies |
|---|---|
| `exports_every_numbered_core` | All 32 cores of a 32-core host are exported |
| `ignores_non_core_and_non_string_values` | `system_cpuX_…`, keys with extra suffixes, non-string values and unrelated keys are not exported as cores |
| `unreported_metrics_are_skipped` | Only the keys Typesense sent are exported; `{}` exports nothing |
| `exports_every_numeric_field` | Every numeric `/stats.json` field is exported: averages, percentiles, max, cache counts, and fields the exporter has never seen |
| `unreported_null_and_non_numeric_fields_are_omitted` | `null`, strings and missing fields are left out instead of becoming 0 |
| `per_endpoint_maps_are_not_scalars` | `latency_ms` and `requests_per_second` (per-endpoint objects) aren't treated as scalar stats |
| `parses_name_and_document_count_ignoring_the_schema` | `/collections` entries parse to name and `num_documents`, ignoring the schema; a missing count is `None` |

### CLI: `src/cli.rs`

| Test | Verifies |
|---|---|
| `applies_defaults` | Defaults: protocol `http`, port 8108, bind `0.0.0.0:8888`, timeout 5s |
| `accepts_overrides` | Every flag overrides its default |
| `rejects_invalid_port` | Port 99999 is rejected |

## Integration tests: `tests/exporter.rs`

Each test starts a [wiremock](https://docs.rs/wiremock) server playing a Typesense node, using fixtures shaped like real responses ([`tests/fixtures/`](../tests/fixtures)). It then serves the real exporter router on an ephemeral port and scrapes it over HTTP. The mocks only answer requests that carry the API key.

| Test | Scenario | Expected |
|---|---|---|
| `healthy_leader_exports_everything` | All six endpoints OK, raft leader | 200; health 1, raft 1, system, swap, cache and per-collection metrics |
| `follower_reports_raft_state_four` | `/debug` reports state 4 | `typesense_raft_state 4` |
| `sends_api_key_to_typesense` | Count the requests | Exactly 6 requests, one per endpoint, each with `X-TYPESENSE-API-KEY` |
| `unreachable_typesense_reports_unhealthy_and_omits_metrics` | Nothing listening on the port | 200; health 0, raft 0, no `typesense_metrics` or `typesense_stats` |
| `unauthorized_endpoints_are_omitted` | 401 on metrics, stats and debug; health OK | Health 1, raft 0, metrics and stats omitted |
| `unhealthy_node_reports_zero_health` | `/health` returns 503 `{"ok": false}` | Health 0 |
| `health_ok_false_with_200_is_unhealthy` | `/health` returns 200 `{"ok": false}` | Health 0 |
| `malformed_response_only_drops_that_endpoint` | `/metrics.json` returns invalid JSON | Only `typesense_metrics` is missing; stats and health are present |
| `timeout_bounds_a_hung_typesense` | Every endpoint takes 10s; timeout 1s | Answers in under 5s, health 0 |
| `zero_timeout_means_no_timeout` | `TYPESENSE_TIMEOUT=0` | Requests succeed (0 disables the timeout instead of failing everything) |
| `exports_version_replication_documents_and_percentiles` | Typesense 30.1-shaped responses from all six endpoints | Version, committed index, queued writes, documents per collection, latency percentiles and cache counts |
| `older_typesense_without_status_still_exports_the_rest` | `/status` returns 404 and `/debug` has no version | Only the raft index and info metrics disappear; everything else is exported |
| `root_points_to_metrics` | `GET /` | Mentions `/metrics` |
| `run_serves_on_the_configured_address` | `server::run` with a chosen port | Serves `/metrics` on that port |
| `run_fails_when_the_port_is_taken` | Bind port already in use | `run` returns `AddrInUse` instead of panicking |

**Checking that the tests catch bugs.** The suite was checked by breaking the code on purpose:
- Removing the API key header failed 5 tests.
- Treating any `/health` response as healthy failed `health_ok_false_with_200_is_unhealthy`.

## End-to-end tests: `tests/e2e.rs`

These run against the [Docker Compose stack](architecture.md#docker-compose-stack): three Typesense 30.1 nodes peering by fixed IP, one exporter per node, Prometheus with the alert rules, and Grafana with the dashboards provisioned.

| Test | What it does | Checks |
|---|---|---|
| `cluster_has_one_leader_and_two_followers` | Reads every exporter and Prometheus | All nodes healthy; roles are one leader and two followers; each exporter reports only its own node; system metrics, Typesense version, committed raft index and queued writes from every node; all 3 exporters scraped |
| `traffic_is_reported_by_the_node_that_served_it` | Creates a collection through node 1 and searches every node, some searches with `use_cache=true` | Documents replicate to every node; every exporter reports 3 documents in the collection; each exporter reports the search traffic its own node served; search p95/p99/max latency reported; `cache_hit_ratio` goes above 0 |
| `followers_catch_up_and_replication_lag_returns_to_zero` | Writes 20 documents through node 1 | Every node's committed raft index moves past the writes; replication lag (max − min committed index) returns to 0; every node reports 23 documents |
| `prometheus_loads_the_alert_rules` | Reads Prometheus rules and alerts | All three rules loaded; no alerts while the cluster is healthy |
| `grafana_dashboards_show_every_node` | Queries Grafana's data source, the same path the panels use | Data source healthy; all four dashboards provisioned; per-core CPU, raft state and health return data for all three nodes |
| `leader_failure_is_reported_and_a_new_leader_is_elected` | Stops the current leader | Stopped node reports health 0 and raft 0; a different node becomes leader; `TypesenseUnhealthy` goes pending; `TypesenseNoLeader` never fires during the election and clears afterwards |
| `hung_node_and_quorum_loss_raise_no_leader` | Stops one follower and **pauses** (freezes) the other, so 2 of 3 nodes are gone | The frozen node's exporter answers in under 4s (the scrape timeout) and reports health 0; all exporters still scraped; no node reports leader; `TypesenseNoLeader` goes pending |
| `parses_exporter_output` | No Docker needed | The test's own Prometheus-format parser handles labels containing spaces |

**How the scenarios stay independent:**
- Scenarios that stop or pause nodes restore them, even if the test fails, through a `Drop` guard.
- Every test first waits for a healthy cluster with exactly one leader.
- A lock stops scenarios from overlapping within one test process. Under nextest, a single-threaded test group does the same ([`.config/nextest.toml`](../.config/nextest.toml)).
- Run one scenario on its own with:

  ```bash
  cargo test --features e2e --test e2e leader_failure -- --nocapture
  docker compose down -v
  ```

**Bugs this suite found during development:**
- **Wrong timeout settings.** On Linux, a stopped Typesense makes requests hang. With `TYPESENSE_TIMEOUT` equal to the scrape timeout, Prometheus abandoned the scrape, so the health alerts never saw data.
- **A broken alert rule.** `TypesenseNoLeader` was first written with `count(...) == 0`, which can never fire: in PromQL, `count()` of nothing returns nothing, not 0.
- **Cluster peering by hostname.** A restarted node couldn't rejoin while a peer was down, because Docker had removed the peer's DNS name. Fixed by peering on fixed IPs.

## Coverage

```bash
make coverage        # summary; fails under 90% of lines
make coverage-html   # browsable report
```

Coverage uses [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`cargo install cargo-llvm-cov`, plus `rustup component add llvm-tools-preview`) over unit and integration tests. It is about **98% of lines**. The remaining gaps:
- `main.rs`, which the e2e suite exercises as the real binary
- the Ctrl-C / SIGTERM shutdown path

CI publishes the figure to the README badge on every push to `main`.

If the local figure looks wrong (lines marked uncovered that are blank or comments, or more "lines" than the file has), stale build output is mixed in. Run `rm -rf target/llvm-cov-target` and measure again; `cargo llvm-cov clean` alone doesn't always clear it.

## CI

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every pull request and on pushes to `main`:

| Job | Runs |
|---|---|
| `lint` | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` |
| `test` | Unit and integration tests under `cargo llvm-cov` (90% minimum); uploads `lcov.info`; publishes the coverage badge on `main` |
| `e2e` | `cargo nextest run --features e2e --test e2e --profile ci`; uploads a JUnit report; prints exporter logs on failure; always tears the stack down |

[`rust.yml`](../.github/workflows/rust.yml) also builds release binaries for Linux (amd64, arm64), macOS (amd64, arm64) and Windows on every pull request.

## Adding tests

- **Pure logic** (parsing, rendering): add a unit test next to the code.
- **Anything involving HTTP or Typesense responses:** add a case to `tests/exporter.rs`, mocking Typesense with `mount(&mock, route, response)`.
- **Behaviour that needs a real cluster** (raft, replication, alerts, dashboards): add a scenario to `tests/e2e.rs`.
  - Start with `let _stack = stack().await; wait_for_healthy_cluster().await;`.
  - Use `eventually(...)` for anything that takes time to propagate.
  - Restore anything you break with `Restore`.
