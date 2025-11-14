use std::sync::Arc;
use std::time::Duration;

use crate::{cli::CliArgs, typesense::models::typesense_stats_model::TypesenseStats};
use axum::Error;

pub async fn get_typesense_stats(args: Arc<CliArgs>) -> Result<TypesenseStats, Error> {
    let mut stats_data: TypesenseStats = TypesenseStats::default();

    let mut client_builder = reqwest::Client::builder();
    if args.typesense_timeout >= 0 {
        client_builder = client_builder.timeout(Duration::from_secs(args.typesense_timeout as u64));
    }
    let client = client_builder.build().unwrap();

    let url = format!(
        "{}://{}:{}/stats.json",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    let res = match client
        .get(url)
        .header("X-TYPESENSE-API-KEY", format!("{}", args.typesense_api_key))
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            println!("Stats endpoint: request failed (timeout/connection error): {:?}", e);
            return Ok(stats_data);
        }
    };

    match res.status() {
        reqwest::StatusCode::OK => {
            match res.json::<TypesenseStats>().await {
                Ok(parsed) => {
                    stats_data = parsed;
                }
                Err(_) => println!("Hm, the response didn't match the shape we expected."),
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
