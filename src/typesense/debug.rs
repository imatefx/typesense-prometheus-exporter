use std::sync::Arc;

use crate::{cli::CliArgs, typesense::models::typesense_debug_model::TypesenseDebug};
use axum::Error;

pub async fn get_typesense_debug(args: Arc<CliArgs>) -> Result<TypesenseDebug, Error> {
    let mut debug_data: TypesenseDebug = TypesenseDebug::default();

    let client = reqwest::Client::new();

    let url = format!(
        "{}://{}:{}/debug",
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
            match res.json::<TypesenseDebug>().await {
                Ok(parsed) => {
                    debug_data = parsed;
                }
                Err(er) => println!(
                    "Hm, the response didn't match the shape we expected. {:?}",
                    er
                ),
            };
        }
        reqwest::StatusCode::UNAUTHORIZED => {
            println!("Debug endpoint: Need to grab a new token");
        }
        _ => {
            panic!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(debug_data)
}

