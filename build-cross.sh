#!/usr/bin/env bash
# Build static Linux binaries (amd64 + arm64) using the project Dockerfile.
# Output: releases/typesense-prometheus-exporter-linux-<arch>-musl
set -euo pipefail

cd "$(dirname "$0")"
OUT_DIR=releases
NAME=typesense-prometheus-exporter
mkdir -p "$OUT_DIR"

for arch in amd64 arm64; do
    echo "==> Building linux/$arch"
    tmp=$(mktemp -d)
    docker buildx build --platform "linux/$arch" --output "type=local,dest=$tmp" .
    cp "$tmp/$NAME" "$OUT_DIR/$NAME-linux-$arch-musl"
    rm -rf "$tmp"
done

ls -l "$OUT_DIR"
