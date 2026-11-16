#!/usr/bin/env bash
# End-to-end test: bring up the docker compose stack, generate real Typesense
# traffic, and check it flows through the exporter into Prometheus and Grafana.
# Then stop Typesense and check the failure is reported.
#
# Requires: docker (with compose), curl, jq.
# KEEP_STACK=1 leaves the stack running afterwards.
set -euo pipefail

cd "$(dirname "$0")/.."

API_KEY=${TYPESENSE_API_KEY:-local-dev-key}
TS=http://localhost:8108
EXPORTER=http://localhost:8888
PROM=http://localhost:9090
GRAFANA=http://localhost:3000
GRAFANA_AUTH="admin:${GRAFANA_ADMIN_PASSWORD:-admin}"

failures=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; failures=$((failures + 1)); }

cleanup() {
    if [[ $failures -gt 0 ]]; then
        echo "--- exporter logs"; docker compose logs --tail 30 exporter || true
    fi
    if [[ "${KEEP_STACK:-0}" != 1 ]]; then
        docker compose down -v --remove-orphans >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

# Retry `cmd` until it succeeds or `timeout` seconds pass.
eventually() {
    local timeout=$1 desc=$2; shift 2
    local deadline=$((SECONDS + timeout))
    until "$@" >/dev/null 2>&1; do
        if ((SECONDS >= deadline)); then fail "$desc (timed out after ${timeout}s)"; return 0; fi
        sleep 2
    done
    pass "$desc"
}

ts() { curl -sf -H "X-TYPESENSE-API-KEY: $API_KEY" "$@"; }
metrics() { curl -sf "$EXPORTER/metrics"; }
gauge_is() { metrics | grep -E "^$1\{.*\} $2\$"; }
prom_value_is() {
    curl -sf --get "$PROM/api/v1/query" --data-urlencode "query=$1" |
        jq -e --arg v "$2" '.data.result | length > 0 and all(.value[1] == $v)'
}
alert_state_is() {
    curl -sf "$PROM/api/v1/alerts" | jq -e --arg a "$1" --arg s "$2" \
        '[.data.alerts[] | select(.labels.alertname == $a and .state == $s)] | length > 0'
}
grafana() { local p=$1; shift; curl -sf -u "$GRAFANA_AUTH" "$@" "$GRAFANA$p"; }

echo "==> Starting stack"
docker compose up -d --build --wait --wait-timeout 180

echo "==> Exporter"
eventually 60 "exporter reports Typesense healthy" gauge_is typesense_health 1
eventually 30 "exporter reports raft leader" gauge_is typesense_raft_state 1
eventually 30 "exporter exports system metrics" sh -c "curl -sf $EXPORTER/metrics | grep -q 'key=\"system_memory_total_bytes\"'"

echo "==> Generating Typesense traffic"
ts -X DELETE "$TS/collections/products" >/dev/null 2>&1 || true
ts -X POST "$TS/collections" -H 'Content-Type: application/json' -d '{
  "name": "products",
  "fields": [{"name": "title", "type": "string"}, {"name": "price", "type": "float"}]
}' >/dev/null
printf '%s\n' \
    '{"id":"1","title":"Blue running shoes","price":79.0}' \
    '{"id":"2","title":"Red rain jacket","price":120.0}' \
    '{"id":"3","title":"Green hiking boots","price":149.0}' |
    ts -X POST "$TS/collections/products/documents/import?action=create" --data-binary @- >/dev/null
search_traffic() {
    for q in shoes jacket boots blue red; do
        ts "$TS/collections/products/documents/search?q=$q&query_by=title" >/dev/null
    done
}
search_traffic
eventually 60 "per-collection search metrics appear for 'products'" \
    sh -c "$(declare -f search_traffic ts); API_KEY=$API_KEY TS=$TS search_traffic; curl -sf $EXPORTER/metrics | grep -q 'by_collection{action=\"documents/search\",collection_name=\"products\"'"

echo "==> Prometheus"
eventually 60 "scrape target is up" prom_value_is 'up{job="typesense-exporter"}' 1
eventually 30 "typesense_health stored" prom_value_is typesense_health 1
eventually 30 "typesense_raft_state stored" prom_value_is typesense_raft_state 1
eventually 30 "alert rules loaded" sh -c "curl -sf $PROM/api/v1/rules | jq -e '[.data.groups[].rules[].name] | contains([\"TypesenseUnhealthy\",\"TypesenseNoLeader\",\"TypesenseExporterDown\"])'"

echo "==> Grafana"
eventually 60 "Grafana is up" grafana /api/health
eventually 30 "Prometheus datasource is healthy" sh -c "curl -sf -u '$GRAFANA_AUTH' $GRAFANA/api/datasources/uid/prometheus/health | jq -e '.status == \"OK\"'"
eventually 30 "all three dashboards are provisioned" sh -c "curl -sf -u '$GRAFANA_AUTH' '$GRAFANA/api/search?query=Typesense&type=dash-db' | jq -e 'length == 3'"
panel_query_has_data() {
    grafana /api/ds/query -X POST -H 'Content-Type: application/json' -d '{
      "from": "now-5m", "to": "now",
      "queries": [{"refId": "A", "datasource": {"uid": "prometheus"},
                   "expr": "typesense_metrics{key=~\"system_cpu[0-9]+_active_percentage\"}",
                   "instant": true}]
    }' | jq -e '.results.A.frames | length > 0'
}
eventually 30 "dashboard per-core CPU query returns data through Grafana" panel_query_has_data

echo "==> Failure: stopping Typesense"
docker compose stop typesense >/dev/null
eventually 30 "exporter reports Typesense unhealthy" gauge_is typesense_health 0
eventually 30 "exporter omits metrics it cannot fetch" sh -c "! curl -sf $EXPORTER/metrics | grep -q '^typesense_metrics{'"
eventually 30 "exporter still serves /metrics" metrics
eventually 60 "TypesenseUnhealthy alert is pending" alert_state_is TypesenseUnhealthy pending
eventually 30 "TypesenseNoLeader alert is pending" alert_state_is TypesenseNoLeader pending

echo "==> Recovery: starting Typesense"
docker compose start typesense >/dev/null
eventually 90 "exporter reports Typesense healthy again" gauge_is typesense_health 1

echo
if [[ $failures -gt 0 ]]; then
    echo "E2E FAILED: $failures check(s) failed"
    exit 1
fi
echo "E2E PASSED"
