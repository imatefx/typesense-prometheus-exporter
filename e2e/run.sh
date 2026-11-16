#!/usr/bin/env bash
# End-to-end test against the docker compose stack: a 3-node Typesense cluster,
# one exporter per node, Prometheus and Grafana.
#
# 1. Cluster formation: every node healthy, exactly one raft leader.
# 2. Real traffic shows up per node, in Prometheus and in Grafana.
# 3. Leader failure: the dead node is reported, a new leader is reported.
# 4. A hung node plus a dead node (quorum lost): the exporter answers within its
#    timeout, nothing reports leader, and the alerts go pending.
# 5. Recovery: the cluster re-forms and is reported healthy again.
#
# Requires: docker (with compose), curl, jq.
# KEEP_STACK=1 leaves the stack running afterwards.
set -euo pipefail

cd "$(dirname "$0")/.."

API_KEY=${TYPESENSE_API_KEY:-local-dev-key}
PROM=http://localhost:9090
GRAFANA=http://localhost:3000
GRAFANA_AUTH="admin:${GRAFANA_ADMIN_PASSWORD:-admin}"
NODES=(1 2 3)
ts_url() { echo "http://localhost:$((8107 + $1))"; }       # node n -> 8108, 8109, 8110
exporter_url() { echo "http://localhost:$((8887 + $1))"; } # node n -> 8888, 8889, 8890

failures=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; failures=$((failures + 1)); }

cleanup() {
    if [[ $failures -gt 0 ]]; then
        echo "--- exporter logs"; docker compose logs --tail 15 exporter-1 exporter-2 exporter-3 || true
    fi
    if [[ "${KEEP_STACK:-0}" != 1 ]]; then
        docker compose down -v --remove-orphans >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

# Retry `cmd` until it succeeds or `timeout` seconds pass; records a failure
# instead of aborting so every check runs.
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
# gauge_is <node> <metric> <value>: the node's exporter reports metric == value.
gauge_is() { curl -sf -m 8 "$(exporter_url "$1")/metrics" | grep -qE "^$2\{.*\} $3\$"; }
# prom_is <promql> <value>: an instant query returns exactly one sample equal to value.
prom_is() {
    curl -sf --get "$PROM/api/v1/query" --data-urlencode "query=$1" |
        jq -e --arg v "$2" '.data.result | length == 1 and .[0].value[1] == $v'
}
alert_state_is() {
    curl -sf "$PROM/api/v1/alerts" | jq -e --arg a "$1" --arg s "$2" \
        '[.data.alerts[] | select(.labels.alertname == $a and .state == $s)] | length > 0'
}
grafana() { local p=$1; shift; curl -sf -u "$GRAFANA_AUTH" "$@" "$GRAFANA$p"; }
leader_node() {
    for n in "${NODES[@]}"; do
        gauge_is "$n" typesense_raft_state 1 && { echo "$n"; return 0; }
    done
    return 1
}

echo "==> Starting stack"
docker compose up -d --build --wait --wait-timeout 240

echo "==> Cluster formation"
for n in "${NODES[@]}"; do
    eventually 60 "exporter-$n reports typesense-$n healthy" gauge_is "$n" typesense_health 1
done
eventually 60 "Prometheus sees all three nodes healthy" prom_is 'count(typesense_health == 1)' 3
eventually 60 "exactly one node reports raft leader" prom_is 'count(typesense_raft_state == 1)' 1
eventually 30 "the other two report follower" prom_is 'count(typesense_raft_state == 4)' 2
eventually 30 "system metrics captured from every node" \
    prom_is 'count(count by (host) (typesense_metrics{key="system_memory_total_bytes"}))' 3
eventually 30 "all three exporters scraped" prom_is 'sum(up{job="typesense-exporter"})' 3

echo "==> Traffic"
ts -X DELETE "$(ts_url 1)/collections/products" >/dev/null 2>&1 || true
ts -X POST "$(ts_url 1)/collections" -H 'Content-Type: application/json' -d '{
  "name": "products",
  "fields": [{"name": "title", "type": "string"}, {"name": "price", "type": "float"}]
}' >/dev/null
printf '%s\n' \
    '{"id":"1","title":"Blue running shoes","price":79.0}' \
    '{"id":"2","title":"Red rain jacket","price":120.0}' \
    '{"id":"3","title":"Green hiking boots","price":149.0}' |
    ts -X POST "$(ts_url 1)/collections/products/documents/import?action=create" --data-binary @- >/dev/null
replicated() {
    for n in "${NODES[@]}"; do
        ts "$(ts_url "$n")/collections/products/documents/search?q=boots&query_by=title" |
            jq -e '.found == 1' || return 1
    done
}
eventually 30 "documents written to node 1 are searchable on every node" replicated
# Each node's /stats.json counts the requests that node served, so search every
# node and check each exporter reports its own per-collection traffic.
searched_on_node() {
    local n=$1
    for q in shoes jacket boots; do
        ts "$(ts_url "$n")/collections/products/documents/search?q=$q&query_by=title" >/dev/null
    done
    curl -sf "$(exporter_url "$n")/metrics" |
        grep -q 'by_collection{action="documents/search",collection_name="products"'
}
for n in "${NODES[@]}"; do
    eventually 60 "exporter-$n reports search traffic served by typesense-$n" searched_on_node "$n"
done
cached_searches() {
    for _ in 1 2 3 4 5; do
        ts "$(ts_url 1)/collections/products/documents/search?q=boots&query_by=title&use_cache=true" >/dev/null
    done
    curl -sf "$(exporter_url 1)/metrics" | grep -qE '^typesense_stats\{.*key="cache_hit_ratio".*\} 0\.[0-9]*[1-9]'
}
eventually 60 "cached searches show up as a non-zero cache_hit_ratio" cached_searches

echo "==> Prometheus and Grafana"
eventually 30 "alert rules loaded" sh -c "curl -sf $PROM/api/v1/rules | jq -e '[.data.groups[].rules[].name] | contains([\"TypesenseUnhealthy\",\"TypesenseNoLeader\",\"TypesenseExporterDown\"])'"
eventually 60 "Grafana is up" grafana /api/health
eventually 30 "Prometheus datasource is healthy" sh -c "curl -sf -u '$GRAFANA_AUTH' $GRAFANA/api/datasources/uid/prometheus/health | jq -e '.status == \"OK\"'"
eventually 30 "all three dashboards are provisioned" sh -c "curl -sf -u '$GRAFANA_AUTH' '$GRAFANA/api/search?query=Typesense&type=dash-db' | jq -e 'length == 3'"
grafana_hosts() {
    grafana /api/ds/query -X POST -H 'Content-Type: application/json' -d "{
      \"from\": \"now-5m\", \"to\": \"now\",
      \"queries\": [{\"refId\": \"A\", \"datasource\": {\"uid\": \"prometheus\"},
                     \"expr\": \"$1\", \"instant\": true}]
    }" | jq -e '[.results.A.frames[] | select((.data.values[1] // []) | length > 0)
                 | .schema.fields[1].labels.host] | unique | length == 3'
}
eventually 30 "Grafana per-core CPU query returns data for all three nodes" \
    grafana_hosts 'typesense_metrics{key=~\"system_cpu[0-9]+_active_percentage\"}'
eventually 30 "Grafana raft state query returns all three nodes" grafana_hosts 'typesense_raft_state'

echo "==> Leader failure"
old_leader=$(leader_node)
echo "    stopping leader typesense-$old_leader"
docker compose stop "typesense-$old_leader" >/dev/null
eventually 30 "exporter-$old_leader reports its node unhealthy" gauge_is "$old_leader" typesense_health 0
eventually 30 "exporter-$old_leader reports no raft role" gauge_is "$old_leader" typesense_raft_state 0
eventually 60 "a new leader is elected and reported" prom_is 'count(typesense_raft_state == 1)' 1
new_leader=$(leader_node || echo none)
if [[ "$new_leader" != "$old_leader" && "$new_leader" != none ]]; then
    pass "new leader is typesense-$new_leader"
else
    fail "expected a different leader, got '$new_leader'"
fi
eventually 60 "TypesenseUnhealthy is pending for the stopped node" alert_state_is TypesenseUnhealthy pending
eventually 10 "TypesenseNoLeader is not active while a leader exists" \
    sh -c "! curl -sf $PROM/api/v1/alerts | jq -e '[.data.alerts[] | select(.labels.alertname == \"TypesenseNoLeader\")] | length > 0'"

echo "==> Hung node + quorum loss"
hung=""
for n in "${NODES[@]}"; do
    if [[ $n != "$old_leader" && $n != "$new_leader" ]]; then hung=$n; fi
done
echo "    pausing follower typesense-$hung (2 of 3 nodes now down)"
docker compose pause "typesense-$hung" >/dev/null
scrape_secs=$(curl -s -o /dev/null -m 10 -w '%{time_total}' "$(exporter_url "$hung")/metrics")
if awk -v t="$scrape_secs" 'BEGIN { exit !(t < 4) }'; then
    pass "exporter-$hung answers within its timeout for a hung node (${scrape_secs}s)"
else
    fail "exporter-$hung took ${scrape_secs}s for a hung node (scrape timeout is 4s)"
fi
eventually 30 "exporter-$hung reports the hung node unhealthy" gauge_is "$hung" typesense_health 0
eventually 30 "all three exporters are still scraped" prom_is 'sum(up{job="typesense-exporter"})' 3
eventually 60 "no node reports raft leader" prom_is 'max(typesense_raft_state == bool 1)' 0
eventually 60 "TypesenseNoLeader alert is pending" alert_state_is TypesenseNoLeader pending

echo "==> Recovery"
docker compose unpause "typesense-$hung" >/dev/null
docker compose start "typesense-$old_leader" >/dev/null
eventually 120 "all three nodes healthy again" prom_is 'count(typesense_health == 1)' 3
eventually 60 "exactly one leader again" prom_is 'count(typesense_raft_state == 1)' 1

echo
if [[ $failures -gt 0 ]]; then
    echo "E2E FAILED: $failures check(s) failed"
    exit 1
fi
echo "E2E PASSED"
