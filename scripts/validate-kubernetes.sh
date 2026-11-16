#!/usr/bin/env bash
# Render each kustomization under deploy/kubernetes and validate the output
# with kubeconform (also the ServiceMonitor and Probe custom resources).
# Needs docker. CI runs the same script.
set -euo pipefail
cd "$(dirname "$0")/.."

KUSTOMIZE=registry.k8s.io/kustomize/kustomize:v5.4.3
KUBECONFORM=ghcr.io/yannh/kubeconform:v0.6.7
CRDS='https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

for dir in deploy/kubernetes/*/; do
  dir=${dir%/}
  echo "== $dir"
  docker run --rm -v "$PWD":/w -w /w "$KUSTOMIZE" build "$dir" |
    docker run --rm -i "$KUBECONFORM" -strict -summary \
      -schema-location default -schema-location "$CRDS" -
done
