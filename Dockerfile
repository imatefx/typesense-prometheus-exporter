FROM docker.io/library/rust:1.88-alpine AS builder
# musl-dev provides the C toolchain needed by rustls' crypto backend.
RUN apk add --no-cache musl-dev

WORKDIR /wd
COPY . .
# Cache the crate registry and build directory between builds (BuildKit).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/wd/target \
    cargo build --bins --release && \
    cp target/release/typesense-prometheus-exporter /typesense-prometheus-exporter

FROM scratch
ARG version=unknown
ARG release=unreleased
LABEL name="Typesense Prometheus Exporter" \
      maintainer="letter2stalin@gmail.com" \
      version=${version} \
      release=${release} \
      summary="Metrics exporter for Typesense" \
      description="Exposes Typesense metrics in Prometheus format"

EXPOSE 8888

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /typesense-prometheus-exporter /
# Run as an unprivileged user (nobody); the image has no shell or other files.
USER 65534:65534
CMD ["./typesense-prometheus-exporter"]
