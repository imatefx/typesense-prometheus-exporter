#!/usr/bin/env bash
# Generate steady, varied Typesense traffic against the compose cluster so the
# Grafana dashboards have data to show. Writes go to node 1 (Typesense forwards
# them to the leader); searches are spread across all three nodes.
# Usage: scripts/traffic.sh [seconds]
set -euo pipefail

TS=${TS:-http://localhost:8108}
NODES=(http://localhost:8108 http://localhost:8109 http://localhost:8110)
KEY=${TYPESENSE_API_KEY:-local-dev-key}
DURATION=${1:-300}
ts() { curl -sf -o /dev/null -H "X-TYPESENSE-API-KEY: $KEY" "$@"; }

for c in products articles; do
    ts -X DELETE "$TS/collections/$c" || true
    ts -X POST "$TS/collections" -H 'Content-Type: application/json' \
        -d "{\"name\":\"$c\",\"fields\":[{\"name\":\"title\",\"type\":\"string\"},{\"name\":\"n\",\"type\":\"int32\"}]}"
done

words=(red blue green running shoes jacket boots wool cotton trail light heavy summer winter)
end=$((SECONDS + DURATION)); i=0
while ((SECONDS < end)); do
    i=$((i + 1))
    batch=""
    for _ in $(seq 1 $((RANDOM % 20 + 1))); do
        id=$((RANDOM * 100 + RANDOM))
        batch+="{\"id\":\"$id\",\"title\":\"${words[RANDOM % 14]} ${words[RANDOM % 14]}\",\"n\":$id}"$'\n'
    done
    printf '%s' "$batch" | ts -X POST "$TS/collections/products/documents/import?action=upsert" --data-binary @-
    ((i % 3 == 0)) && printf '%s' "$batch" | ts -X POST "$TS/collections/articles/documents/import?action=upsert" --data-binary @-
    for _ in $(seq 1 $((RANDOM % 15 + 5))); do
        c=$([[ $((RANDOM % 3)) == 0 ]] && echo articles || echo products)
        node=${NODES[RANDOM % 3]}
        # Half the searches opt into Typesense's search cache so cache_hit_ratio moves.
        cache=$([[ $((RANDOM % 2)) == 0 ]] && echo "&use_cache=true" || echo "")
        ts "$node/collections/$c/documents/search?q=${words[RANDOM % 14]}&query_by=title$cache" &
    done
    ((i % 10 == 0)) && ts -X DELETE "$TS/collections/products/documents?filter_by=n:<$((RANDOM * 50))" || true
    wait
    sleep 0.$((RANDOM % 5 + 3))
done
