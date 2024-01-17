use std::sync::Arc;

use crate::{cli::CliArgs, typesense::models::typesense_metrics_model::TypesenseMetrics};
use axum::Error;

pub async fn get_typesense_metrics(args: Arc<CliArgs>) -> Result<TypesenseMetrics, Error> {
    let mut stats_data: TypesenseMetrics = TypesenseMetrics::default();

    let client = reqwest::Client::new();

    let url: String = format!(
        "{}://{}:{}/metrics.json",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    // println!("url : {:?}", url);

    let res = client
        .get(url)
        .header("X-TYPESENSE-API-KEY", format!("{}", args.typesense_api_key))
        .send()
        .await
        .unwrap();

    match res.status() {
        reqwest::StatusCode::OK => {
            // let json_str = res.text().await.unwrap();
            //     println!("json_str {:?}", json_str);

            //  let deserialized_metrics: TypesenseMetrics = serde_json::from_str(json_str.as_str()).unwrap();
            //     println!("deserialized_metrics  {:?}", deserialized_metrics);

            match res.json::<TypesenseMetrics>().await {
                Ok(parsed) => {
                    stats_data = parsed;
                    //                println!("Success! {:#?}", stats_data)
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
            panic!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(stats_data)
}
