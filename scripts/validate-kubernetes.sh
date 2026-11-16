#!/usr/bin/env bash
# Render each kustomization under deploy/kubernetes and validate the output
# with kubeconform (also the ServiceMonitor and Probe custom resources).
# Needs docker. CI runs the same script.
set -euo pipefail
cd "$(dirname "$0")/.."

KUSTOMIZE=registry.k8s.io/kustomize/kustomize:v5.4.3
KUBECONFORM=ghcr.io/yannh/kubeconform:v0.6.7
# Pinned to a commit so a change in the catalog can't break CI.
CRDS='https://raw.githubusercontent.com/datreeio/CRDs-catalog/3e8a8916ca1b7af6464606c2c2f5869dbafa9653/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json'

for dir in deploy/kubernetes/*/; do
  dir=${dir%/}
  echo "== $dir"
  docker run --rm -v "$PWD":/w -w /w "$KUSTOMIZE" build "$dir" |
    docker run --rm -i "$KUBECONFORM" -strict -summary \
      -schema-location default -schema-location "$CRDS" -
  # Not in the kustomization, so checked on its own.
  docker run --rm -i "$KUBECONFORM" -strict -summary -schema-location default \
    - <"$dir/secret.example.yaml"
done
