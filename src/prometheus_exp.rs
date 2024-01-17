extern crate prometheus;
use std::sync::Arc;

use crate::{
    cli::CliArgs,
    typesense::models::{
        typesense_metrics_model::TypesenseMetrics, typesense_stats_model::TypesenseStats,
    },
};
use prometheus::{register_gauge_vec_with_registry, Counter, Encoder, Opts, Registry, TextEncoder};

// #[tokio::main]

pub(crate) async fn generate_metrics(
    ts_metrics: TypesenseMetrics,
    ts_stats: TypesenseStats,
    cli_args: Arc<CliArgs>,
) -> String {
    let r = Registry::new();
    // r.register(Box::new(typesense_metrics.clone())).unwrap();
    // r.register(Box::new(typesense_stats.clone())).unwrap();
    // r.register(Box::new(typesense_stats_latency_ms.clone())).unwrap();
    // r.register(Box::new(typesense_stats_requests_per_second.clone())).unwrap();

    let typesense_metrics = register_gauge_vec_with_registry!(
        "typesense_metrics",
        "Data received through the metrics api of typesense",
        &["host", "port", "key"],
        r
    )
    .unwrap();

    // Set gauge values for typesense_metrics
    // typesense_metrics.with_label_values(&["ts-tenant.com", "8108", "system_cpu1_active_percentage"]).set(0.00);
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_cpu1_active_percentage",
        ])
        .set(
            ts_metrics
                .system_cpu1_active_percentage
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_cpu3_active_percentage",
        ])
        .set(
            ts_metrics
                .system_cpu3_active_percentage
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_cpu2_active_percentage",
        ])
        .set(
            ts_metrics
                .system_cpu2_active_percentage
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_cpu4_active_percentage",
        ])
        .set(
            ts_metrics
                .system_cpu4_active_percentage
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_cpu_active_percentage",
        ])
        .set(
            ts_metrics
                .system_cpu_active_percentage
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_disk_total_bytes",
        ])
        .set(ts_metrics.system_disk_total_bytes.parse().unwrap_or(0.0));
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_disk_used_bytes",
        ])
        .set(ts_metrics.system_disk_used_bytes.parse().unwrap_or(0.0));
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_memory_total_bytes",
        ])
        .set(ts_metrics.system_memory_total_bytes.parse().unwrap_or(0.0));
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_memory_used_bytes",
        ])
        .set(ts_metrics.system_memory_used_bytes.parse().unwrap_or(0.0));
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_network_received_bytes",
        ])
        .set(
            ts_metrics
                .system_network_received_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "system_network_sent_bytes",
        ])
        .set(ts_metrics.system_network_sent_bytes.parse().unwrap_or(0.0));
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_active_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_active_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_allocated_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_allocated_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_fragmentation_ratio",
        ])
        .set(
            ts_metrics
                .typesense_memory_fragmentation_ratio
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_mapped_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_mapped_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_metadata_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_metadata_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_resident_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_resident_bytes
                .parse()
                .unwrap_or(0.0),
        );
    typesense_metrics
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "typesense_memory_retained_bytes",
        ])
        .set(
            ts_metrics
                .typesense_memory_retained_bytes
                .parse()
                .unwrap_or(0.0),
        );

    // typesense_metrics
    //     .with_label_values(&["ts-tenant.com", "8108", "system_cpu2_active_percentage"])
    //     .set(0.00);
    // typesense_metrics
    //     .with_label_values(&["ts-tenant.com", "8108", "system_cpu_active_percentage"])
    //     .set(0.00);
    // // ... Add more metrics as needed

    // Create a GaugeVec for typesense_stats
    let typesense_stats = register_gauge_vec_with_registry!(
        "typesense_stats",
        "Data received through the stats api of typesense",
        &["host", "port", "key"],
        r
    )
    .unwrap();

    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "delete_latency_ms",
        ])
        .set(ts_stats.delete_latency_ms);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "delete_requests_per_second",
        ])
        .set(ts_stats.delete_requests_per_second);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "import_latency_ms",
        ])
        .set(ts_stats.import_latency_ms);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "import_requests_per_second",
        ])
        .set(ts_stats.import_requests_per_second);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "overloaded_requests_per_second",
        ])
        .set(ts_stats.overloaded_requests_per_second);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "pending_write_batches",
        ])
        .set(ts_stats.pending_write_batches);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "search_latency_ms",
        ])
        .set(ts_stats.search_latency_ms);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "search_requests_per_second",
        ])
        .set(ts_stats.search_requests_per_second);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "total_requests_per_second",
        ])
        .set(ts_stats.total_requests_per_second);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "write_latency_ms",
        ])
        .set(ts_stats.write_latency_ms);
    typesense_stats
        .with_label_values(&[
            &cli_args.typesense_host,
            &cli_args.typesense_port.to_string(),
            "write_requests_per_second",
        ])
        .set(ts_stats.write_requests_per_second);

    // Create a GaugeVec for typesense_stats_latency_ms
    let typesense_stats_latency_ms = register_gauge_vec_with_registry!(
        "typesense_stats_latency_ms",
        "Each endpoint latency in ms from stats api",
        &["host", "port", "key"],
        r
    )
    .unwrap();

    // Set gauge values for typesense_stats_latency_ms
    typesense_stats_latency_ms
        .with_label_values(&["ts-tenant.com", "8108", "GET /metrics.json"])
        .set(101.0);
    typesense_stats_latency_ms
        .with_label_values(&["ts-tenant.com", "8108", "GET /stats.json"])
        .set(0.0);
    // ... Add more metrics as needed

    // Create a GaugeVec for typesense_stats_requests_per_second
    let typesense_stats_requests_per_second = register_gauge_vec_with_registry!(
        "typesense_stats_requests_per_second",
        "Each endpoint rps from stats api",
        &["host", "port", "key"],
        r
    )
    .unwrap();

    // Set gauge values for typesense_stats_requests_per_second
    typesense_stats_requests_per_second
        .with_label_values(&["ts-tenant.com", "8108", "GET /metrics.json"])
        .set(0.3);
    typesense_stats_requests_per_second
        .with_label_values(&["ts-tenant.com", "8108", "GET /stats.json"])
        .set(0.2);
    // ... Add more metrics as needed

    // Expose the metrics in Prometheus exposition format
    let encoder = TextEncoder::new();
    let metric_families = r.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    // Convert the buffer to a String and print it
    let metric_line = String::from_utf8(buffer).unwrap();

    //   println!("{}", metric_line);

    return metric_line;
}
