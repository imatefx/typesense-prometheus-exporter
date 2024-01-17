# Typesense Prometheus Exporter

## Introduction

This repository contains the Typesense-Prometheus-Exporter, a tool designed to expose Typesense metrics and stats in Prometheus format.

### About Typesense

[Typesense](https://typesense.org) is an open-source, typo-tolerant search engine that delivers fast and relevant search results. It is designed for simplicity and ease of use and provides a scalable, highly available, and easy-to-scale search solution.

### About Prometheus

[Prometheus](https://prometheus.io/) is an open-source systems monitoring and alerting toolkit originally built at SoundCloud. It has a multi-dimensional data model with time series data identified by metric name and key/value pairs, a flexible query language, and integrates with various external systems.

### About Prometheus Exporters

Prometheus exporters are tools that let you translate metrics from third-party systems into a format that Prometheus can understand. They are essential for using Prometheus to monitor non-Prometheus systems.

## What It Does

This exporter is specifically designed for Typesense. It fetches metrics and stats from a configured Typesense instance and exports them in the Prometheus format. This allows these metrics to be consumed by a Prometheus time series database server, facilitating efficient monitoring and alerting for Typesense instances.

## Usage

### Exposing Typesense Metrics and Stats in Prometheus Format

To use the Typesense-Prometheus-Exporter, execute the binary with the required options:

```
typesense-prometheus-exporter [OPTIONS] --typesense-host <TYPESENSE_HOST> --typesense-api-key <TYPESENSE_API_KEY>
```

### Options

- `--typesense-host <TYPESENSE_HOST>`: The URL of the Typesense Host (environment variable: TYPESENSE_HOST).
- `--typesense-protocol <TYPESENSE_PROTOCOL>`: The protocol for Typesense (environment variable: TYPESENSE_PROTOCOL). Default: `http`.
- `--typesense-api-key <TYPESENSE_API_KEY>`: The API key for Typesense (environment variable: TYPESENSE_API_KEY).
- `--typesense-port <TYPESENSE_PORT>`: The port number for Typesense (environment variable: TYPESENSE_PORT). Default: `8108`.
- `--exporter-bind-address <EXPORTER_BIND_ADDRESS>`: The bind address for the internal server (environment variable: EXPORTER_BIND_ADDRESS). Default: `0.0.0.0`.
- `--exporter-bind-port <EXPORTER_BIND_PORT>`: The bind port for the internal server (environment variable: EXPORTER_BIND_PORT). Default: `8888`.

### Help and Version Information

- `-h, --help`: Print help information.
- `-V, --version`: Print the version information.

For more detailed information, please refer to the documentation within the repository.