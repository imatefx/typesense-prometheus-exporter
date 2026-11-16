# Getting started

Two ways in:
- **Try the full stack locally:** a Typesense cluster with one exporter, Prometheus and Grafana, all running in Docker. Nothing to configure.
- **Monitor your own Typesense:** run one exporter (or one per node) and point Prometheus at it.

## Try the full stack (5 minutes)

Requirements: Docker with Compose v2.

```bash
git clone https://github.com/imatefx/typesense-prometheus-exporter.git
cd typesense-prometheus-exporter
docker compose up -d --build        # first build takes a few minutes
./scripts/traffic.sh 300            # optional: 5 minutes of search/write traffic
```

Then open:

| | URL | Notes |
|---|---|---|
| Grafana | http://localhost:3000 | `admin` / `admin`; dashboards are in the **Typesense** folder |
| Prometheus | http://localhost:9090 | **Alerts** shows the three Typesense alert rules |
| Exporter | http://localhost:8888/probe?target=typesense-1 (also `-2`, `-3`) | one exporter, probing every node |
| Typesense | http://localhost:8108, `:8109`, `:8110` | API key `local-dev-key` |

Things to try:

```bash
# Who is the raft leader?
for n in 1 2 3; do curl -s "localhost:8888/probe?target=typesense-$n"; done | grep '^typesense_raft_state'

# Kill a node and watch the Cluster Overview dashboard: a new leader takes over within seconds
docker compose stop typesense-1
docker compose start typesense-1
```

Tear down with `docker compose down -v`.

## Monitor your own Typesense

### 1. Run the exporter

Each node reports its own health, raft role and the traffic it served, so Prometheus has to scrape every node separately. Either run one exporter per node (this section), or one exporter for the whole cluster ([below](#one-exporter-for-the-whole-cluster)). Docker:

```bash
docker run -d --name typesense-exporter -p 8888:8888 \
  -e TYPESENSE_HOST=typesense-1.internal \
  -e TYPESENSE_API_KEY=<admin-api-key> \
  imatefx/typesense-prometheus-exporter:v1.2.0
```

Or the binary. Build it with `cargo build --release`, or use the static Linux builds from `./build-cross.sh`.

```bash
typesense-prometheus-exporter \
  --typesense-host typesense-1.internal \
  --typesense-api-key <admin-api-key>
```

Check it:

```bash
curl -s localhost:8888/metrics | grep -E '^typesense_(health|raft_state)'
# typesense_health{host="typesense-1.internal",port="8108"} 1
# typesense_raft_state{host="typesense-1.internal",port="8108"} 1
```

If `typesense_health` is 0, check the exporter's logs. Each failed Typesense call is logged with the reason, e.g. a 401 or a connection refused.

**Keep the key out of the command line.** Put the key in a file readable only by the exporter, and pass its path instead. The exporter re-reads the file when it changes, so you can rotate the key without a restart ([rotating keys](configuration.md#rotating-keys)):

```bash
docker run -d --name typesense-exporter -p 8888:8888 \
  -v /etc/typesense-exporter/api-key:/run/secrets/typesense/api-key:ro \
  -e TYPESENSE_HOST=typesense-1.internal \
  -e TYPESENSE_API_KEY_FILE=/run/secrets/typesense/api-key \
  imatefx/typesense-prometheus-exporter:v1.2.0
```

**Or use a config file.** Every option can go in a TOML file, and flags and environment variables still override it. Keys are never written inline, only as a key file or an environment variable name. Start from [`deploy/exporter.example.toml`](../deploy/exporter.example.toml), which lists every option with its default:

```toml
# exporter.toml
typesense_host = "typesense-1.internal"
typesense_protocol = "https"
typesense_api_key_file = "/run/secrets/typesense/api-key"
```

```bash
typesense-prometheus-exporter --config exporter.toml   # or TYPESENSE_EXPORTER_CONFIG=exporter.toml
```

See [the config file](configuration.md#config-file) for the precedence rules, and [several clusters with different keys](configuration.md#several-clusters-with-different-keys) for one exporter watching more than one cluster.

### 2. Scrape it with Prometheus

```yaml
scrape_configs:
  - job_name: typesense-exporter
    scrape_timeout: 10s   # must be above TYPESENSE_TIMEOUT (5s by default)
    static_configs:
      - targets:
          - exporter-1.internal:8888
          - exporter-2.internal:8888
          - exporter-3.internal:8888

rule_files:
  - alerts.yml            # copy deploy/prometheus/alerts.yml
```

### One exporter for the whole cluster

Instead of one exporter per node, run a single exporter without `--typesense-host`, and list the nodes it may probe:

```bash
docker run -d --name typesense-exporter -p 8888:8888 \
  -e TYPESENSE_PROBE_ALLOWED_TARGETS=typesense-1.internal,typesense-2.internal,typesense-3.internal \
  -e TYPESENSE_API_KEY=<admin-api-key> \
  imatefx/typesense-prometheus-exporter:v1.2.0
```

Prometheus scrapes the exporter once per node and passes the node as `target`. Spelled out for two nodes, each entry's address is the exporter, `__param_target` becomes `?target=`, and `instance` names the node:

```yaml
scrape_configs:
  - job_name: typesense-exporter
    metrics_path: /probe
    scrape_timeout: 10s   # must be above TYPESENSE_TIMEOUT (5s by default)
    static_configs:
      - targets: [exporter.internal:8888]
        labels: {__param_target: typesense-1.internal:8108, instance: typesense-1.internal:8108}
      - targets: [exporter.internal:8888]
        labels: {__param_target: typesense-2.internal:8108, instance: typesense-2.internal:8108}
```

With more nodes, or with service discovery, list only the nodes and let relabeling build the same scrapes, like `blackbox_exporter`:

```yaml
scrape_configs:
  - job_name: typesense-exporter
    metrics_path: /probe
    scrape_timeout: 10s   # must be above TYPESENSE_TIMEOUT (5s by default)
    static_configs:
      - targets:          # the Typesense nodes, not the exporter
          - typesense-1.internal:8108
          - typesense-2.internal:8108
          - typesense-3.internal:8108
    relabel_configs:
      - source_labels: [__address__]
        target_label: __param_target
      - source_labels: [__param_target]
        target_label: instance
      - target_label: __address__
        replacement: exporter.internal:8888   # the exporter
```

Check it with `curl -s 'localhost:8888/probe?target=typesense-1.internal' | grep -E '^typesense_(health|raft_state)'`. The `host` and `port` labels on the metrics name the Typesense node, so the dashboards and alerts work unchanged. Targets that aren't in `TYPESENSE_PROBE_ALLOWED_TARGETS` get a 403, because the exporter sends its API key to every node it probes; see [the security note](configuration.md#security-the-allow-list). `up` is now per probe, so `TypesenseExporterDown` fires for every node at once when the exporter is down.

### On Kubernetes

[`deploy/kubernetes/`](../deploy/kubernetes) has ready-to-apply manifests for both setups above (`kubectl apply -k`), with a Prometheus Operator `ServiceMonitor` or `Probe`. See its [README](../deploy/kubernetes/README.md) for pointing it at an existing Typesense install and for rotating the API key without a restart.

### 3. Import the Grafana dashboards

In Grafana, go to **Dashboards → New → Import** and upload each file from [`grafana/`](../grafana). Pick your Prometheus data source when asked.

Next: [configuration](configuration.md) for every option, and [metrics](metrics.md) for what each metric means.
