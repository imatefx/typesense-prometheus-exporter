FROM docker.io/library/rust:1.88-alpine AS builder
# musl-dev provides the C toolchain needed by rustls' crypto backend.
RUN apk add --no-cache musl-dev

WORKDIR /wd
COPY . .
RUN cargo build --bins --release

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
COPY --from=builder /wd/target/release/typesense-prometheus-exporter /
CMD ["./typesense-prometheus-exporter"]


