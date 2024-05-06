use std::sync::Arc;

use crate::{cli::CliArgs, typesense::models::typesense_stats_model::TypesenseStats};
use axum::Error;

pub async fn get_typesense_stats(args: Arc<CliArgs>) -> Result<TypesenseStats, Error> {
    let mut stats_data: TypesenseStats = TypesenseStats::default();

    let client = reqwest::Client::new();

    let url = format!(
        "{}://{}:{}/stats.json",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    let res = client
        .get(url)
        .header("X-TYPESENSE-API-KEY", format!("{}", args.typesense_api_key))
        .send()
        .await
        .unwrap();

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
            panic!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(stats_data)
}
