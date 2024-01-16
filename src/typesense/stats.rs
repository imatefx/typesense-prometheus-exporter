use serde_json::Map;
use axum::Error;
use crate::typesense::models::typesense_stats_model::TypesenseStats;

pub async fn get_typesense_stats() -> Result<TypesenseStats, Error> {
    let mut stats_data: TypesenseStats = TypesenseStats {
        delete_latency_ms: Some(0.0),
        delete_requests_per_second: Some(0.0),
        import_latency_ms: Some(0.0),
        import_requests_per_second: Some(0.0),
        latency_ms: Some(Map::new()),
        overloaded_requests_per_second: Some(0.0),
        pending_write_batches: Some(0.0),
        requests_per_second: Some(Map::new()),
        search_latency_ms: Some(0.0),
        search_requests_per_second: Some(0.0),
        total_requests_per_second: Some(0.0),
        write_latency_ms: Some(0.0),
        write_requests_per_second: Some(0.0),
    };

    println!("{:?}", stats_data);

    let client = reqwest::Client::new();

    let res = client
        .get("http://<host>:8108/stats.json")
        .header("X-TYPESENSE-API-KEY", "<key>")
        .send()
        .await
        .unwrap();

    match res.status() {
        reqwest::StatusCode::OK => {
            match res.json::<TypesenseStats>().await {
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