FROM docker.io/library/rust:1.75-alpine as builder
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconf libpq-dev

# Set `SYSROOT` to a dummy path (default is /usr) because pkg-config-rs *always*
# links those located in that path dynamically but we want static linking, c.f.
# https://github.com/rust-lang/pkg-config-rs/blob/54325785816695df031cef3b26b6a9a203bbc01b/src/lib.rs#L613
ENV SYSROOT=/dummy

# The env var tells pkg-config-rs to statically link libpq.
ENV LIBPQ_STATIC=1

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


