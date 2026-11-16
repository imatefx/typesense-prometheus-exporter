# Testing

Three layers, from fast and isolated to slow and real:

| Layer | Where | Runs against | Count | Time | Needs |
|---|---|---|---|---|---|
| Unit | `src/**` (`#[cfg(test)]`) | Plain functions | 95 | < 1s | Rust |
| Integration | [`tests/exporter.rs`](../tests/exporter.rs), [`tests/config_env.rs`](../tests/config_env.rs) | The real HTTP server and a mock Typesense ([wiremock](https://docs.rs/wiremock)) | 59 | ~1s | Rust |
| End-to-end | [`tests/e2e.rs`](../tests/e2e.rs) | A real 3-node Typesense cluster, one exporter, Prometheus and Grafana in Docker | 9 | ~3–5 min | Rust, Docker |

```bash
make test               # lint + unit + integration
make test-unit          # cargo test --lib
make test-integration   # cargo test --test exporter
make coverage           # unit + integration coverage, fails under 90% of lines
make coverage-html      # HTML coverage report, opens in a browser
make e2e                # starts the stack, runs the e2e tests, tears the stack down
```

The config-file unit tests read the real process environment (through `CliArgs`, which reads `TYPESENSE_*`), so a developer with `TYPESENSE_*` variables exported may see failures; run them with those unset (`env -u TYPESENSE_HOST ...`). `tests/config_env.rs` sets and clears its own variables and serialises its tests.

A plain `cargo test` runs unit and integration tests only. The e2e tests sit behind the `e2e` cargo feature, so they never need Docker by accident.

## Unit tests

Unit tests build a `Scrape` by hand. `Scrape::default()` uses the default `--metric-names=v1`; tests about the 0.4 output set `metric_names: MetricNames::Legacy` (marked **legacy** below) or `Both`.

### Metric rendering: `src/prometheus_exp.rs`

Shared behaviour and the 0.4 names:

| Test | Verifies |
|---|---|
| `parses_collection_endpoint` | `GET /collections/books/documents/search` splits into method, collection and action; `GET /health` doesn't match |
| `exports_all_metric_families` | **legacy.** A full scrape renders every 0.4 family with the right labels. Per-core CPU covers any core count (core 16). Unnumbered keys, unreported keys and unknown fields are left out. Swap, cache hit ratio, health and raft state are present. |
| `failed_endpoints_are_omitted_not_zeroed` | **both.** With nothing fetched, every 0.4 and 1.0 `/metrics.json` and `/stats.json` family and `typesense_unmapped` are absent; health and raft state are 0 |
| `unknown_raft_states_are_reported_as_zero` | Raft state 1 → 1 and 4 → 4; any other state → 0 |
| `unparseable_values_are_omitted` | **legacy.** A non-numeric value (`"n/a"`) in `/metrics.json` or a per-endpoint stat is left out, not exported as 0 |
| `normalizes_resource_ids_but_keeps_collections_and_actions` | Document, synonym, key, alias and set IDs become `:id`; collection names, `search`/`import`/`export` and query strings are handled |
| `merged_endpoints_sum_rates_and_weight_latency` | Entries that collapse to one endpoint sum their request rates and average latency weighted by rate |
| `exports_scrape_health` | `typesense_scrape_success{endpoint}` and `typesense_scrape_duration_seconds` are rendered |
| `non_collection_paths_skip_by_collection_metrics` | `/health`, `/collections` and `/collections/books` give no per-collection labels; `DELETE …/documents/42` does |
| `output_has_help_and_type_lines` | Output has `# HELP` and `# TYPE … gauge` lines |
| `exports_version_replication_and_collection_documents` | `typesense_info{version}`, committed index, queued writes and per-collection document counts are rendered; collections without a count are skipped |
| `new_families_are_omitted_when_their_endpoint_failed` | No `/debug`, `/status` or `/collections` data means no info, raft index, queued writes or document metrics |
| `status_fields_missing_from_older_typesense_are_omitted` | A `/status` without `committed_index` or `queued_writes` doesn't produce fake zeros |
| `exports_latency_percentiles_and_cache_counts` | **legacy.** p95/p99, max latency and cache hit/miss counts are exported as `typesense_stats` keys |

The 1.0 names ([design](metrics-v1.md)) and the `--metric-names` modes:

| Test | Verifies |
|---|---|
| `default_mode_is_v1` | `Scrape::default()` uses the 1.0 names |
| `maps_every_metrics_json_field` | Every `/metrics.json` field lands in its 1.0 family with the expected value: CPU and per-core CPU as ratios, memory, swap, disk, network and allocator values; only the unknown fields go to `typesense_unmapped` |
| `maps_every_stats_json_field` | Every `/stats.json` scalar lands in its 1.0 family: per-operation rates and average latency, all and overloaded rates, the latency quantiles (min is `quantile="0"`, max `quantile="1"`), pending write batches and cache counts; `total` is not an `operation` label |
| `maps_every_quantile_for_every_operation` | Every quantile field of every operation maps to `typesense_request_latency_seconds{operation,quantile}`, converted to seconds, with nothing unmapped |
| `maps_routes_and_collections` | Per-route latency and rate become `typesense_route_*{route}` and `typesense_collection_*{collection,method,action}`; non-collection routes stay out of the collection families; no `collection_name` or `endpoint` labels |
| `merged_routes_keep_the_normalisation_in_v1` | Document IDs still collapse to `:id` with summed rates and rate-weighted latency |
| `converts_to_base_units` | Milliseconds become seconds and percentages become ratios; ratios, bytes and rates are passed through |
| `network_bytes_are_counters_and_the_rest_gauges` | The two network byte families are counters (`_total`); every other 1.0 family is a gauge |
| `negative_or_unparseable_counters_are_omitted` | A negative or non-numeric network counter is left out, not exported or sent to `typesense_unmapped` |
| `unknown_numeric_fields_fall_back_to_unmapped` | Numeric fields without a 1.0 name become `typesense_unmapped{source,key}` with the raw value; non-numeric and null fields are left out |
| `unmapped_is_only_emitted_with_v1_names` | `typesense_unmapped` appears in `v1` and `both`, never in `legacy` |
| `v1_families_are_omitted_not_zeroed` | Failed endpoints, missing fields and non-numeric values produce no 1.0 series; health and raft state are still 0 |
| `each_mode_emits_only_its_names` | `v1` has only the 1.0 families, `legacy` only the 0.4 ones, `both` has all of them |
| `unchanged_families_are_emitted_once_in_every_mode` | Health, raft, info, documents and scrape-health families appear exactly once in every mode |
| `both_mode_keeps_legacy_output_byte_for_byte` | Every `legacy` line, `# HELP` and `# TYPE` included, appears unchanged in `both`; `legacy` has the 0.4 shape |

### Probe targets: `src/target.rs`

| Test | Verifies |
|---|---|
| `bare_host_takes_defaults` | `host` takes the default scheme and port |
| `host_with_port` | `host:port` for names and IPv4 |
| `explicit_schemes` | `http://` and `https://` (any case), with or without a port |
| `hosts_are_lowercased` | Upper-case hosts are normalised |
| `ipv6_in_brackets` | `[::1]`, `[::1]:8108`, `https://[2001:DB8::1]:443`; addresses are written canonically |
| `base_url_brackets_ipv6` | The URL the exporter calls brackets IPv6 hosts |
| `surrounding_whitespace_is_trimmed` | Whitespace around a target is ignored |
| `rejects_bad_targets` | Empty, other schemes, paths, queries, fragments, credentials, bad or zero ports, unbracketed IPv6, bad characters |
| `a_trailing_dot_is_the_same_host` | `ts.` parses as `ts`; a bare `.` is invalid |
| `normalizes_and_detects_loopback` | Brackets, case, a trailing dot and IPv6 spelling (`0:0::1` is `::1`) are normalised; `localhost`, `127.0.0.1` and `::1` are loopback |
| `allow_list_matches_host_and_port_ignoring_scheme_and_case` | An entry allows its host and port in any scheme or case; a different port is refused |
| `allow_list_wildcard_allows_everything` | `*` allows any target |
| `empty_allow_list_allows_nothing_until_a_node_is_added` | Blank entries are skipped; the configured host can be added |
| `allow_list_rejects_bad_entries` | Entries with a scheme, path, credentials, a zero port or bare IPv6 are errors |
| `has_remote_ignores_loopback_entries` | Only non-loopback entries (or `*`) count as remote |
| `lists_matches_exact_nodes_only` | `AllowList::lists` matches host (case-insensitive) and port, and `*` is not a listed node |

### Config file: `src/config.rs`

| Test | Verifies |
|---|---|
| `every_option_can_be_set_in_the_file` | Every option, a key file, and a cluster with an `api_key_env`, can come from the file; the cluster inherits the file's port and protocol |
| `relative_paths_are_relative_to_the_config_file` | A relative key file path resolves from the config file's directory; absolute paths are unchanged |
| `flags_beat_the_file_and_the_file_beats_defaults` | A flag wins (even one equal to the default); the file replaces built-in defaults |
| `a_flag_key_replaces_the_files_key_settings` | A key from a flag skips the file's key settings; `typesense_api_key_env` is read and trimmed |
| `rejects_bad_key_settings_in_the_file` | Unset or empty `typesense_api_key_env`, and both key sources, are errors naming the setting |
| `rejects_unknown_keys_and_inline_secrets_without_echoing_them` | A typo, an inline `typesense_api_key` or cluster `api_key`, and unknown tables are errors; the hint points to the file/env options; the secret never appears |
| `invalid_values_name_the_file_and_the_line` | Wrong types, out-of-range numbers, bad `metric_names`, `probe_max_concurrency = 0` and invalid TOML name the file and line |
| `errors_never_contain_the_values_they_reject` | A secret-looking string in every typed field, a `*_env` setting and a protocol never appears in the error; an invalid variable name is reported without echoing it |
| `the_protocol_is_validated_and_lowercased_from_every_source` | `ftp` from file, flag or no config is an error; `HTTPS`/`HTTP` are stored lowercase and clusters inherit it |
| `the_example_file_loads_once_a_key_is_given` | `deploy/exporter.example.toml` loads as-is (clusters commented out) |
| `a_missing_config_file_is_an_error_and_no_file_changes_nothing` | A missing file names its path; without `--config` nothing changes |

### Clusters: `src/probe_config.rs`

| Test | Verifies |
|---|---|
| `reads_keys_from_a_file_and_from_the_environment` | Both key sources, trimming, lookup by host and port (case-insensitive) |
| `protocol_overrides_the_default_and_ports_default` | A cluster's protocol overrides the default; ports default; IPv6 targets |
| `rejects_missing_empty_or_doubled_keys_naming_the_cluster` | No key, unset/empty variable, empty or missing key file, and both sources are errors naming the cluster (and file) |
| `rejects_bad_names_targets_protocols_and_empty_configs` | Cluster names outside `[A-Za-z0-9_-]+`, bad targets, no targets and a bad protocol |
| `rejects_a_node_in_two_clusters_but_not_twice_in_one` | A node in two clusters names both; repeating it in one is harmless |
| `debug_never_prints_the_key` | `Debug` output redacts the key |

### API key reload: `src/key.rs`

| Test | Verifies |
|---|---|
| `fixed_key_never_changes` | A flag or environment key is returned as is; `Debug` redacts it |
| `changed_contents_give_the_new_key` | A rewritten key file gives the new key, trimmed |
| `empty_or_missing_file_keeps_the_old_key_and_warns_once` | An empty or deleted file keeps the old key, records one warning per distinct problem, and a usable file afterwards takes effect and clears it |
| `a_rewrite_with_the_same_length_and_mtime_is_noticed_on_unix` | A same-length rewrite with the mtime restored (`cp -p`, `rsync -t`) is seen through the inode/change time on unix; elsewhere it is not (documented) |
| `a_failed_read_is_retried_so_a_permission_fix_takes_effect` | An unreadable file keeps the old key and its signature is not remembered, so a later `chmod` (change time only) takes effect (unix only) |
| `a_key_that_is_not_a_header_value_is_rejected_without_naming_it` | A BOM-prefixed or control-character key fails at startup and is ignored on reload; the error names the file, never the key |
| `a_slow_file_system_does_not_block_other_callers` | While one caller is stuck reading the key file (a FIFO), another gets the cached key at once (unix only) |
| `checks_at_most_once_per_interval` | Within the first second a change is not looked for |
| `symlink_swap_like_a_kubernetes_secret_is_followed` | Swapping a `..data` symlink to a directory with a new key, with the same key length, is noticed (unix only) |
| `startup_fails_on_an_unusable_file` | A missing or empty key file is an error at startup |

### Models: `src/typesense/models/`

| Test | Verifies |
|---|---|
| `exports_every_numbered_core` | All 32 cores of a 32-core host are exported |
| `ignores_non_core_and_non_string_values` | `system_cpuX_…`, keys with extra suffixes, non-string values and unrelated keys are not exported as cores |
| `recognises_core_numbers` | `core_number` returns the core of `system_cpu12_active_percentage` and nothing for the overall CPU, `cpuX`, suffixed or unrelated keys |
| `fixed_entries_leave_out_cores_and_unknown_fields` | `fixed_entries()` gives only the fixed `/metrics.json` fields, not per-core values or unknown keys |
| `unreported_metrics_are_skipped` | Only the keys Typesense sent are exported; `{}` exports nothing |
| `exports_every_numeric_field` | Every numeric `/stats.json` field is exported: averages, percentiles, max, cache counts, and fields the exporter has never seen |
| `unreported_null_and_non_numeric_fields_are_omitted` | `null`, strings and missing fields are left out instead of becoming 0 |
| `per_endpoint_maps_are_not_scalars` | `latency_ms` and `requests_per_second` (per-endpoint objects) aren't treated as scalar stats |
| `parses_name_and_document_count_ignoring_the_schema` | `/collections` entries parse to name and `num_documents`, ignoring the schema; a missing count is `None` |

### CLI: `src/cli.rs`

| Test | Verifies |
|---|---|
| `applies_defaults` | Defaults: protocol `http`, port 8108, bind `0.0.0.0:8888`, timeout 5s, metric names `v1` |
| `accepts_overrides` | Every flag overrides its default |
| `probe_concurrency_defaults_to_16_and_must_be_positive` | `--probe-max-concurrency` defaults to 16, takes a value, and rejects 0 |
| `rejects_invalid_port_and_negative_timeout` | Port 99999 and a negative timeout are rejected |
| `rejects_two_key_sources` | `--typesense-api-key` and `--typesense-api-key-file` can't both be set |
| `reads_and_trims_the_key_file` | The key file is read and trimmed; an empty or missing file is an error |
| `never_prints_the_key` | Debug output redacts the key, and `--help` hides its environment value |
| `flags_plaintext_keys_to_remote_hosts` | Plain HTTP to a remote host is flagged; localhost and HTTPS are not |
| `parses_metric_names` | `--metric-names` accepts `v1`, `both` and `legacy` and rejects anything else |
| `metric_names_select_the_name_sets` | `v1` selects the 1.0 names, `legacy` the 0.4 names, `both` both |
| `metric_names_reads_its_environment_variable` | `--metric-names` is also read from `TYPESENSE_METRIC_NAMES` |
| `parses_probe_allowed_targets` | `--probe-allowed-targets` is empty by default and splits on commas |
| `probe_allowed_targets_reads_its_environment_variable` | Also read from `TYPESENSE_PROBE_ALLOWED_TARGETS` |
| `host_is_optional_with_an_allow_list` | `--typesense-host` can be left out when an allow list is set |
| `config_reads_its_environment_variable` | `--config` is also read from `TYPESENSE_EXPORTER_CONFIG`; with only `--config`, host and key stay unset until merged |
| `load_from_merges_the_file_and_reports_errors_as_clap_errors` | `CliArgs::load_from` fills in the file's values and turns config errors into a clap error; unknown flags still fail |
| `flags_plaintext_keys_sent_by_probe` | Plain HTTP with `*` or a non-loopback allowed host is flagged; loopback, no allow list and HTTPS are not |

## Integration tests: `tests/exporter.rs`

Each test starts a [wiremock](https://docs.rs/wiremock) server playing a Typesense node, using fixtures shaped like real responses ([`tests/fixtures/`](../tests/fixtures)). It then serves the real exporter router on an ephemeral port and scrapes it over HTTP. The mocks only answer requests that carry the API key.

The exporter runs with the default `--metric-names=v1` unless a test says otherwise. "The `/metrics.json` families" and "the `/stats.json` families" mean the 1.0 families those endpoints feed (`has_metrics_json` / `has_stats_json` in the test file).

| Test | Scenario | Expected |
|---|---|---|
| `healthy_leader_exports_everything` | All six endpoints OK, raft leader | 200; health 1, raft 1, 1.0 system, swap, per-core CPU, cache and per-collection metrics; no 0.4 names |
| `follower_reports_raft_state_four` | `/debug` reports state 4 | `typesense_raft_state 4` |
| `sends_api_key_to_typesense` | Count the requests | Exactly 6 requests, one per endpoint, each with `X-TYPESENSE-API-KEY`; `/collections` asks Typesense to leave out schemas |
| `unreachable_typesense_reports_unhealthy_and_omits_metrics` | Nothing listening on the port | 200; health 0, raft 0, the `/metrics.json` and `/stats.json` families absent |
| `unauthorized_endpoints_are_omitted` | 401 on metrics, stats and debug; health OK | Health 1, raft 0, the `/metrics.json` and `/stats.json` families absent |
| `unhealthy_node_reports_zero_health` | `/health` returns 503 `{"ok": false}` | Health 0 |
| `health_ok_false_with_200_is_unhealthy` | `/health` returns 200 `{"ok": false}` | Health 0 |
| `malformed_response_only_drops_that_endpoint` | `/metrics.json` returns invalid JSON | Only the `/metrics.json` families are absent; the `/stats.json` families and health are present |
| `timeout_bounds_a_hung_typesense` | Every endpoint takes 10s; timeout 1s | Answers in under 5s, health 0 |
| `zero_timeout_means_no_timeout` | `TYPESENSE_TIMEOUT=0` | Requests succeed (0 disables the timeout instead of failing everything) |
| `exports_version_replication_documents_and_percentiles` | Typesense 30.1-shaped responses from all six endpoints | Version, committed index, queued writes, documents per collection, latency quantiles and cache counts |
| `older_typesense_without_status_still_exports_the_rest` | `/status` returns 404 and `/debug` has no version | Only the raft index and info metrics disappear; everything else is exported |
| `root_points_to_metrics` | `GET /` | Mentions `/metrics` |
| `serve_runs_the_exporter_on_a_given_listener` | `server::serve` on a pre-bound listener (no port race) | Serves `/metrics` |
| `scrape_health_reports_each_endpoint_and_the_duration` | Healthy node | `typesense_scrape_success` 1 for all six endpoints; a scrape duration |
| `scrape_health_flags_the_failing_endpoints` | 401, malformed JSON and a missing endpoint | Those endpoints report 0; the working one reports 1 |
| `document_ids_do_not_create_new_series` | 50 document IDs plus a search in `/stats.json` | Exactly 2 series: `documents/:id` (rates summed) and `documents/search`; no `key` label on the by-collection metrics |
| `reads_the_api_key_from_a_file` | Key supplied with `TYPESENSE_API_KEY_FILE` | Requests carry the key from the file |
| `a_rotated_key_file_is_used_by_the_next_scrape` | Scrape, rewrite the key file, scrape again after more than a second, then empty the file and scrape once more | The mock sees the first key, then the new key, then the new key again (an empty file keeps the last good key) |
| `a_client_keeps_the_key_it_was_made_with_for_all_its_requests` | Make a client from the current key, rotate the file, send another request | Both requests carry the old key: a scrape never mixes keys |
| `a_rotated_cluster_key_file_is_used_by_the_next_probe` | Same for a cluster's `api_key_file`, through `/probe` | The mock sees the old key, then the new one |
| `startup_fails_without_a_readable_key_file` | Key file missing | `server::router` returns an error |
| `run_fails_when_the_port_is_taken` | Bind port already in use | `run` returns `AddrInUse` instead of panicking |
| `v1_names_are_the_default_with_base_units_and_counters` | Default settings, healthy node | Every 1.0 family and no 0.4 family; network bytes are counters; values in seconds and ratios; nothing in `typesense_unmapped` |
| `legacy_names_reproduce_the_0_4_output` | `--metric-names=legacy` | Every 0.4 family with the 0.4 keys and labels, no 1.0 family, no `typesense_unmapped`, only gauges |
| `both_names_emit_old_and_new_and_shared_families_once` | `--metric-names=both` | Both name sets; shared families (health, raft, info, documents, scrape health) once; every 0.4 series identical to `legacy` |
| `v1_unknown_fields_fall_back_to_unmapped` | Fields with no 1.0 name in `/metrics.json` and `/stats.json` | `typesense_unmapped{source,key}` with the raw value; fields Typesense didn't send are absent |

`/probe`, using two or three mock nodes and an exporter without `--typesense-host`:

| Test | Scenario | Expected |
|---|---|---|
| `probe_scrapes_each_node_with_its_own_labels` | Probe a leader and a follower | Each answer has its own raft state and `host`/`port` labels; each node got exactly 6 requests, all with the API key |
| `probe_output_matches_metrics_for_the_same_node` | `/metrics` and `/probe` for the configured node | Identical output apart from the scrape duration |
| `a_hung_node_only_affects_its_own_probe` | One healthy, one hung, one unreachable node, probed together | The healthy probe is quick and healthy; the others return 200 with health 0 and no metrics |
| `redirects_are_not_followed_so_the_key_stays_with_the_node` | An allowed node answers every request with a 302 to another server | The probe still returns 200 with `typesense_health` 0; the redirect target received no request, so the API key didn't follow |
| `probe_refuses_downgrading_an_https_default_to_http` | Default protocol https, probe `http://` an allowed node | 403 `must be https`; the node received no request |
| `probe_concurrency_is_capped` | `--probe-max-concurrency 1`, two slow nodes probed at once | Both return 200, one after the other |
| `probe_refuses_targets_that_are_not_allowed_without_contacting_them` | Probe a node, with another port or scheme, that isn't in the allow list | 403 `not allowed`; the node received no request, so no API key left |
| `probe_matches_allowed_targets_case_and_scheme_insensitively` | Allow `LocalHost:port`, probe `localhost`, `LOCALHOST` and `http://LocalHost` | All allowed |
| `probe_always_allows_the_configured_host` | `--typesense-host` set, allow list names another node | Probing the configured host works |
| `a_wildcard_allows_any_target` | `--probe-allowed-targets=*` | Any node can be probed |
| `probe_rejects_malformed_targets` | Empty, other scheme, path, credentials, bad port, bare IPv6, missing parameter | 400 with a reason; nothing is scraped |
| `metrics_is_404_without_a_host_and_points_to_probe` | No `--typesense-host` | `/metrics` is 404 and mentions `/probe?target=` |
| `metrics_is_unchanged_when_a_host_is_set` | `--typesense-host` and an allow list | `/metrics` works as before |
| `root_mentions_probe` | `GET /` | Mentions `/metrics` and `/probe` |
| `router_rejects_a_malformed_allowed_target` | `--probe-allowed-targets` entry with a path | `server::router` returns an error |
| `router_needs_a_host_or_a_non_empty_allow_list` | No host, no clusters and an empty allow list (clap accepts an empty `TYPESENSE_PROBE_ALLOWED_TARGETS`) | `server::router` returns an error |
| `each_cluster_is_probed_with_only_its_own_key` | Two clusters (two mocks) with different keys, probed through one exporter with no default key | Both healthy; each mock received 6 requests, all with its own key and never the other |
| `a_node_in_no_cluster_is_refused_without_a_request` | Probe a node that no cluster lists | 403; the node received no request |
| `a_cluster_with_https_refuses_http_targets` | Cluster `protocol = "https"`, probe `http://node` | 403 `must be https`; no request |
| `a_cluster_protocol_overrides_the_default_for_bare_targets` | Default protocol https, cluster http, bare target | 200 and healthy |
| `probe_config_works_alongside_the_default_key_and_host` | `--typesense-host`, `*` and a cluster together | `/metrics` and other nodes use the default key; the cluster node uses only its own, despite `*` |
| `probe_config_alone_leaves_metrics_404` | Only clusters configured | `/metrics` is 404 and points to `/probe` |
| `router_rejects_a_node_that_has_two_keys` | A cluster node also in `--probe-allowed-targets`, or equal to `--typesense-host` | Startup error naming the cluster and the other place |
| `bad_config_files_are_reported_without_leaking_keys` | Missing file, inline `api_key`, unreadable key file | Errors name the file/cluster; the inline secret isn't echoed |
| `the_exporter_starts_from_a_config_file_alone` | Host, port, key file and timeout all in the file, no flags | `/metrics` works |
| `router_needs_the_default_key_for_a_host_or_allow_list` | Clusters plus `--typesense-host` or an allow list but no default key | `server::router` returns an error; clusters alone are fine |
| `router_rejects_an_ipv6_host_spelled_differently_from_a_cluster_node` | `--typesense-host=0:0::1` and a cluster node `[::1]` | Startup error: one node, one key |
| `router_rejects_an_empty_host_and_ignores_an_empty_key` | Empty `--typesense-host`; empty API key | `server::router` returns an error for each |
| `every_blank_environment_variable_counts_as_unset` (`tests/config_env.rs`) | Every option's variable set to a blank value | The file's value for every option; without a file, the built-in defaults |
| `empty_environment_variables_fall_back_to_the_file` (`tests/config_env.rs`) | Empty `TYPESENSE_HOST`, `TYPESENSE_PROBE_ALLOWED_TARGETS` and `TYPESENSE_API_KEY` | The file's host, list and key are used |
| `an_environment_key_beats_the_files_key_settings` (`tests/config_env.rs`) | `TYPESENSE_API_KEY` with a file `typesense_api_key_file` or `_env` | The environment key is used |
| `list_and_enum_options_follow_the_same_precedence` (`tests/config_env.rs`) | `TYPESENSE_PROBE_ALLOWED_TARGETS` and `TYPESENSE_METRIC_NAMES` against the file and flags | Flag > environment > file |
| `flag_beats_environment_beats_file_beats_default` (`tests/config_env.rs`) | Real environment variables, flags and a file | Flag > environment > file > default; `--config` read from `TYPESENSE_EXPORTER_CONFIG` |

**Checking that the tests catch bugs.** The suite was checked by breaking the code on purpose (re-measured for 1.0.0):
- Removing the API key header failed 17 of the 24 integration tests.
- Treating any `/health` response as healthy failed `health_ok_false_with_200_is_unhealthy`, and nothing else.

## End-to-end tests: `tests/e2e.rs`

These run against the [Docker Compose stack](architecture.md#docker-compose-stack): three Typesense 30.1 nodes peering by fixed IP, one exporter probing every node (`TYPESENSE_PROBE_ALLOWED_TARGETS`, `TYPESENSE_METRIC_NAMES=v1`), Prometheus scraping `/probe` for each node, with the alert rules, and Grafana with the dashboards provisioned. The checks use the 1.0 names.

| Test | What it does | Checks |
|---|---|---|
| `cluster_has_one_leader_and_two_followers` | Probes every node through the exporter and reads Prometheus | All nodes healthy; roles are one leader and two followers; each probe reports only its own node; 1.0 system metrics (`typesense_system_memory_total_bytes`) and no 0.4 `typesense_metrics`/`typesense_stats`; Typesense version, committed raft index and queued writes from every node; every probe reads all six endpoints (`typesense_scrape_success` 1); all 3 probes up |
| `traffic_is_reported_by_the_node_that_served_it` | Creates a collection through node 1 and searches every node, some searches with `use_cache=true` | Documents replicate to every node; every exporter reports 3 documents in the collection; reads by document ID show up as one `typesense_route_requests_per_second` series with `documents/:id`, never raw IDs; each exporter reports the search traffic its own node served; `typesense_request_latency_seconds` reports search quantiles 0.95, 0.99 and 1 (max); `cache_hit_ratio` goes above 0 |
| `followers_catch_up_and_replication_lag_returns_to_zero` | Writes 20 documents through node 1 | Every node's committed raft index moves past the writes; replication lag (max − min committed index) returns to 0; every node reports 23 documents |
| `prometheus_loads_the_alert_rules` | Reads Prometheus rules and alerts | All four rules loaded; no alerts while the cluster is healthy |
| `grafana_dashboards_show_every_node` | Queries Grafana's data source, the same path the panels use | Data source healthy; all four dashboards provisioned; per-core CPU (`typesense_system_cpu_core_active_ratio`), network received rate, raft state and health return data for all three nodes |
| `leader_failure_is_reported_and_a_new_leader_is_elected` | Stops the current leader | Stopped node reports health 0 and raft 0; a different node becomes leader; `TypesenseUnhealthy` goes pending; `TypesenseNoLeader` never fires during the election and clears afterwards |
| `hung_node_and_quorum_loss_raise_no_leader` | Stops one follower and **pauses** (freezes) the other, so 2 of 3 nodes are gone | The frozen node's probe answers in under 4s (the scrape timeout), reports health 0 and every endpoint as failed; all probes still up; no node reports leader; `TypesenseNoLeader` goes pending |
| `probe_refuses_targets_outside_the_allow_list` | Probes nodes and hosts that aren't allowed, and a target with a path | 403 for each target outside the allow list, 400 for the path; `/metrics` is 404 because no host is configured |
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
| `audit` | `cargo audit -D warnings`: fails on any known vulnerability or warning in the dependency tree |
| `test` | Unit and integration tests under `cargo llvm-cov` (90% minimum); uploads `lcov.info`; publishes the coverage badge on `main` |
| `rules` | `promtool check rules` on the alert and recording rules, and `promtool test rules` on [`legacy-names.test.yml`](../deploy/prometheus/legacy-names.test.yml) |
| `kubernetes` | [`scripts/validate-kubernetes.sh`](../scripts/validate-kubernetes.sh): renders both [`deploy/kubernetes`](../deploy/kubernetes) folders with kustomize and validates them (and each `secret.example.yaml`) with kubeconform, including the `ServiceMonitor` and `Probe` schemas (CRD schemas pinned to a commit) |
| `e2e` | `cargo nextest run --features e2e --test e2e --profile ci`; uploads a JUnit report; prints exporter logs on failure; always tears the stack down |

Two more workflows check every pull request:
- [`rust.yml`](../.github/workflows/rust.yml) builds the release binaries: Linux amd64 and arm64 (arm64 on a native `ubuntu-24.04-arm` runner), macOS amd64 and arm64, and Windows.
- [`docker.yml`](../.github/workflows/docker.yml) builds the Docker image for amd64 and arm64, each on a native runner, without pushing. It runs when a pull request touches the Dockerfile, sources or the workflow.

On a version tag, both workflows run `cargo test --locked` first, and publish nothing if it fails.

[Dependabot](../.github/dependabot.yml) opens grouped weekly updates for Rust crates, GitHub Actions and the Docker base image, so they go through all of the above. Actions are pinned to commit SHAs, and Dependabot keeps those current.

## Adding tests

- **Pure logic** (parsing, rendering): add a unit test next to the code.
- **Anything involving HTTP or Typesense responses:** add a case to `tests/exporter.rs`, mocking Typesense with `mount(&mock, route, response)`.
- **Behaviour that needs a real cluster** (raft, replication, alerts, dashboards): add a scenario to `tests/e2e.rs`.
  - Start with `let _stack = stack().await; wait_for_healthy_cluster().await;`.
  - Use `eventually(...)` for anything that takes time to propagate.
  - Restore anything you break with `Restore`.
