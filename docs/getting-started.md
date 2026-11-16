# Getting started

Two ways in:
- **Try the full stack locally:** a Typesense cluster with exporters, Prometheus and Grafana, all running in Docker. Nothing to configure.
- **Monitor your own Typesense:** run the exporter next to each Typesense node and point Prometheus at it.

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
| Exporters | http://localhost:8888/metrics, `:8889`, `:8890` | one per Typesense node |
| Typesense | http://localhost:8108, `:8109`, `:8110` | API key `local-dev-key` |

Things to try:

```bash
# Who is the raft leader?
curl -s localhost:8888/metrics localhost:8889/metrics localhost:8890/metrics | grep '^typesense_raft_state'

# Kill a node and watch the Cluster Overview dashboard: a new leader takes over within seconds
docker compose stop typesense-1
docker compose start typesense-1
```

Tear down with `docker compose down -v`.

## Monitor your own Typesense

### 1. Run one exporter per Typesense node

Each node reports its own health, raft role and the traffic it served, so you need one exporter per node. Docker:

```bash
docker run -d --name typesense-exporter -p 8888:8888 \
  -e TYPESENSE_HOST=typesense-1.internal \
  -e TYPESENSE_API_KEY=<admin-api-key> \
  imatefx/typesense-prometheus-exporter:v0.4.0
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

### 3. Import the Grafana dashboards

In Grafana, go to **Dashboards → New → Import** and upload each file from [`grafana/`](../grafana). Pick your Prometheus data source when asked.

Next: [configuration](configuration.md) for every option, and [metrics](metrics.md) for what each metric means.
