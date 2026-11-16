use std::sync::Arc;
use std::time::Duration;

use crate::{cli::CliArgs, typesense::models::typesense_metrics_model::TypesenseMetrics};
use axum::Error;

pub async fn get_typesense_metrics(args: Arc<CliArgs>) -> Result<TypesenseMetrics, Error> {
    let mut stats_data: TypesenseMetrics = TypesenseMetrics::default();

    let mut client_builder = reqwest::Client::builder();
    if args.typesense_timeout >= 0 {
        client_builder = client_builder.timeout(Duration::from_secs(args.typesense_timeout as u64));
    }
    let client = client_builder.build().unwrap();

    let url: String = format!(
        "{}://{}:{}/metrics.json",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    let res = match client
        .get(url)
        .header("X-TYPESENSE-API-KEY", args.typesense_api_key.to_string())
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            println!("Metrics endpoint: request failed (timeout/connection error): {:?}", e);
            return Ok(stats_data);
        }
    };

    match res.status() {
        reqwest::StatusCode::OK => {
            match res.json::<TypesenseMetrics>().await {
                Ok(parsed) => {
                    stats_data = parsed;
                }
                Err(er) => println!(
                    "Hm, the response didn't match the shape we expected. {:?}",
                    er
                ),
            };
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            println!("Need to grab a new token");
        }
        _ => {
            println!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(stats_data)
}
