use std::sync::Arc;

use crate::{cli::CliArgs, typesense::models::typesense_health_model::TypesenseHealth};
use axum::Error;

pub async fn get_typesense_health(args: Arc<CliArgs>) -> Result<TypesenseHealth, Error> {
    let mut health_data: TypesenseHealth = TypesenseHealth::default();

    let client = reqwest::Client::new();

    let url = format!(
        "{}://{}:{}/health",
        args.typesense_protocol, args.typesense_host, args.typesense_port
    );

    let res = client
        .get(url)
        .send()
        .await
        .unwrap();

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
            panic!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(health_data)
}

