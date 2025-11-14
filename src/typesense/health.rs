use std::sync::Arc;
use std::time::Duration;

use crate::{cli::CliArgs, typesense::models::typesense_health_model::TypesenseHealth};
use axum::Error;

pub async fn get_typesense_health(args: Arc<CliArgs>) -> Result<TypesenseHealth, Error> {
    let mut health_data: TypesenseHealth = TypesenseHealth::default();

    let mut client_builder = reqwest::Client::builder();
    if args.typesense_timeout >= 0 {
        client_builder = client_builder.timeout(Duration::from_secs(args.typesense_timeout as u64));
    }
    let client = client_builder.build().unwrap();

    let url = format!(
        "{}://{}:{}/health",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    let res = match client
        .get(url)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            println!("Health endpoint: request failed (timeout/connection error): {:?}", e);
            return Ok(health_data);
        }
    };

    match res.status() {
        reqwest::StatusCode::OK => {
            match res.json::<TypesenseHealth>().await {
                Ok(parsed) => {
                    health_data = parsed;
                }
                Err(er) => println!(
                    "Hm, the response didn't match the shape we expected. {:?}",
                    er
                ),
            };
        }
        _ => {
            println!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(health_data)
}

