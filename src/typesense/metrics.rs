use axum::Error;
use crate::typesense::models::typesense_metrics_model::TypesenseMetrics;

pub async fn get_typesense_metrics() -> Result<TypesenseMetrics, Error> {
    let mut stats_data: TypesenseMetrics = TypesenseMetrics {
        system_cpu1_active_percentage: Some("".to_string()),
        system_cpu2_active_percentage: Some("".to_string()),
        system_cpu3_active_percentage: Some("".to_string()),
        system_cpu4_active_percentage: Some("".to_string()),
        system_cpu_active_percentage: Some("".to_string()),
        system_disk_total_bytes: Some("".to_string()),
        system_disk_used_bytes: Some("".to_string()),
        system_memory_total_bytes: Some("".to_string()),
        system_memory_used_bytes: Some("".to_string()),
        system_network_received_bytes: Some("".to_string()),
        system_network_sent_bytes: Some("".to_string()),
        typesense_memory_active_bytes: Some("".to_string()),
        typesense_memory_allocated_bytes: Some("".to_string()),
        typesense_memory_fragmentation_ratio: Some("".to_string()),
        typesense_memory_mapped_bytes: Some("".to_string()),
        typesense_memory_metadata_bytes: Some("".to_string()),
        typesense_memory_resident_bytes: Some("".to_string()),
        typesense_memory_retained_bytes: Some("".to_string()),
    };

    let client = reqwest::Client::new();

    let res = client
        .get("http://<host>:8108/metrics.json")
        .header("X-TYPESENSE-API-KEY", "<key>")
        .send()
        .await
        .unwrap();

    match res.status() {
        reqwest::StatusCode::OK => {
            match res.json::<TypesenseMetrics>().await {
                Ok(parsed) => {
                    stats_data = parsed;
                    println!("Success! {:#?}", stats_data)
                }
                Err(_) => println!("Hm, the response didn't match the shape we expected."),
            };
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            println!("Need to grab a new token");
        }
        _ => {
            panic!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(stats_data)
}