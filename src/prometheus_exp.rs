extern crate prometheus;
use std::sync::Arc;

use crate::{
    cli::CliArgs,
    typesense::models::{
        typesense_metrics_model::TypesenseMetrics, typesense_stats_model::TypesenseStats,
    },
};
use prometheus::{register_gauge_vec_with_registry, Encoder, Registry, TextEncoder};
use regex::Regex;

pub(crate) async fn generate_metrics(
    ts_metrics: TypesenseMetrics,
    ts_stats: TypesenseStats,
    cli_args: Arc<CliArgs>,
) -> String {
    let registry = Registry::new();

    let typesense_metrics = register_gauge_vec_with_registry!(
        "typesense_metrics",
        "Data received through the metrics api of typesense",
        &["host", "port", "key"],
        registry
    )
    .unwrap();

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

    let typesense_stats = register_gauge_vec_with_registry!(
        "typesense_stats",
        "Data received through the stats api of typesense",
        &["host", "port", "key"],
        registry
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

    let typesense_stats_latency_ms = register_gauge_vec_with_registry!(
        "typesense_stats_latency_ms",
        "Each endpoint latency in ms from stats api",
        &["host", "port", "key"],
        registry
    )
    .unwrap();

    let typesense_stats_latency_ms_by_collection = register_gauge_vec_with_registry!(
        "typesense_stats_latency_ms_by_collection",
        "Each endpoint latency in ms from stats api",
        &["host", "port", "key", "method", "collection_name", "action"],
        registry
    )
    .unwrap();
    for (key, value) in ts_stats.latency_ms.iter() {
        typesense_stats_latency_ms
            .with_label_values(&[
                &cli_args.typesense_host,
                &cli_args.typesense_port.to_string(),
                key,
            ])
            .set(value.to_string().parse::<f64>().unwrap_or(0.0));

        match parse_collection_action_line(key) {
            Some((method, collection_name, action)) => {
                println!("Method: {}", method);
                println!("Collection Name: {}", collection_name);
                println!("Action: {}", action);

                typesense_stats_latency_ms_by_collection
                    .with_label_values(&[
                        &cli_args.typesense_host,
                        &cli_args.typesense_port.to_string(),
                        key,
                        method.as_str(),
                        collection_name.as_str(),
                        action.as_str(),
                    ])
                    .set(value.to_string().parse::<f64>().unwrap_or(0.0));
            }
            None => {
                println!("No match found");
            }
        }
    }

    let typesense_stats_requests_per_second = register_gauge_vec_with_registry!(
        "typesense_stats_requests_per_second",
        "Each endpoint rps from stats api",
        &["host", "port", "key"],
        registry
    )
    .unwrap();

    let typesense_stats_requests_per_second_by_collection = register_gauge_vec_with_registry!(
        "typesense_stats_requests_per_second_by_collection",
        "Each endpoint rps from stats api",
        &["host", "port", "key", "method", "collection_name", "action"],
        registry
    )
    .unwrap();

    for (key, value) in ts_stats.requests_per_second.iter() {
        typesense_stats_requests_per_second
            .with_label_values(&[
                &cli_args.typesense_host,
                &cli_args.typesense_port.to_string(),
                key,
            ])
            .set(value.to_string().parse::<f64>().unwrap_or(0.0));

        match parse_collection_action_line(key) {
            Some((method, collection_name, action)) => {
                // println!("Method: {}", method);
                // println!("Collection Name: {}", collection_name);
                // println!("Action: {}", action);

                typesense_stats_requests_per_second_by_collection
                    .with_label_values(&[
                        &cli_args.typesense_host,
                        &cli_args.typesense_port.to_string(),
                        key,
                        method.as_str(),
                        collection_name.as_str(),
                        action.as_str(),
                    ])
                    .set(value.to_string().parse::<f64>().unwrap_or(0.0));
            }
            None => {
                // println!("No match found");
            }
        }
    }

    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    let metric_line = String::from_utf8(buffer).unwrap();

    return metric_line;
}

fn parse_collection_action_line(input: &str) -> Option<(String, String, String)> {
    let pattern = r"(\w+)\s+/collections/([^/]+)/([^/]+/[^/]+)";
    let re = Regex::new(pattern).unwrap();

    re.captures(input).map(|caps| {
        let method = caps.get(1).unwrap().as_str().to_string();
        let collection_name = caps.get(2).unwrap().as_str().to_string();
        let action = caps.get(3).unwrap().as_str().to_string();
        (method, collection_name, action)
    })
}
