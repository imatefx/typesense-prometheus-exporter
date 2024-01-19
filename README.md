# Typesense Prometheus Exporter
## Screenshots
### Typesense Metrics Dashboard
![Typesense-Metrics-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/36daa26f-b217-4c1c-9404-2a480a19efe2)

### Typesense Metrics By Collection Dashboard
![Typesense-By-Collection-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/026e6ed9-2a9b-4800-8ae6-2f90aca9fcb7)

### Typesense Stats Dashboard
![Typesense-Stats-Dashboards-Dashboards-Grafana](https://github.com/imatefx/typesense-prometheus-exporter/assets/3025904/afc7948a-de7a-4b8f-a1e6-a8394c61859f)

## Introduction

This repository contains the Typesense-Prometheus-Exporter, a tool designed to expose Typesense metrics and stats in Prometheus format.

### About Typesense

[Typesense](https://typesense.org) is an open-source, typo-tolerant search engine that delivers fast and relevant search results. It's designed for simplicity and ease of use, providing a scalable, highly available, and easy-to-scale search solution.

### About Prometheus

[Prometheus](https://prometheus.io/) is an open-source systems monitoring and alerting toolkit. Known for its multi-dimensional data model and flexible query language, Prometheus integrates with various external systems for comprehensive monitoring.

### About Prometheus Exporters

Prometheus exporters are tools for translating metrics from third-party systems into Prometheus-readable format, crucial for monitoring non-Prometheus systems with Prometheus.

## What It Does

This exporter fetches metrics and stats from a configured Typesense instance and exports them in Prometheus format, enabling their consumption by a Prometheus time series database server.

## Docker Deployment

### Pulling the Docker Image

To pull the Typesense-Prometheus-Exporter image from Docker Hub:

```
docker pull imatefx/typesense-prometheus-exporter
```

### Running the Docker Container

To run the exporter in a Docker container, use the following command:

```
docker run --name="example-ts-prom-exp" \
           -e "TYPESENSE_HOST=tshost.example.com" \
           -e "TYPESENSE_PROTOCOL=http" \
           -e "TYPESENSE_API_KEY=tsAPIKEY" \
           -e "TYPESENSE_PORT=8108" \
           -p 8888:8888 \
           imatefx/typesense-prometheus-exporter
```

This command sets up the Typesense-Prometheus-Exporter with the necessary environment variables and starts the exporter service on exposed port `8888`.


## Usage

### Exposing Typesense Metrics and Stats in Prometheus Format

Run the exporter with the necessary options:

```
typesense-prometheus-exporter [OPTIONS] --typesense-host <TYPESENSE_HOST> --typesense-api-key <TYPESENSE_API_KEY>
```

### Options

- `--typesense-host <TYPESENSE_HOST>`: Typesense Host URL (env: TYPESENSE_HOST).
- `--typesense-protocol <TYPESENSE_PROTOCOL>`: Typesense protocol (env: TYPESENSE_PROTOCOL, default: http).
- `--typesense-api-key <TYPESENSE_API_KEY>`: Typesense API key (env: TYPESENSE_API_KEY).
- `--typesense-port <TYPESENSE_PORT>`: Typesense port number (env: TYPESENSE_PORT, default: 8108).
- `--exporter-bind-address <EXPORTER_BIND_ADDRESS>`: Internal server bind address (env: EXPORTER_BIND_ADDRESS, default: 0.0.0.0).
- `--exporter-bind-port <EXPORTER_BIND_PORT>`: Internal server bind port (env: EXPORTER_BIND_PORT, default: 8888).

### Help and Version

- `-h, --help`: Display help.
- `-V, --version`: Display version.

## Todo
- Add logging
- Code cleanup
