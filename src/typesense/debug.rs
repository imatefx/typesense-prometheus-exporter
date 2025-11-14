use std::sync::Arc;
use std::time::Duration;

use crate::{cli::CliArgs, typesense::models::typesense_debug_model::TypesenseDebug};
use axum::Error;

pub async fn get_typesense_debug(args: Arc<CliArgs>) -> Result<TypesenseDebug, Error> {
    let mut debug_data: TypesenseDebug = TypesenseDebug::default();

    let mut client_builder = reqwest::Client::builder();
    if args.typesense_timeout >= 0 {
        client_builder = client_builder.timeout(Duration::from_secs(args.typesense_timeout as u64));
    }
    let client = client_builder.build().unwrap();

    let url = format!(
        "{}://{}:{}/debug",
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
            println!("Debug endpoint: request failed (timeout/connection error): {:?}", e);
            return Ok(debug_data);
        }
    };

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
            println!("Uh oh! Something unexpected happened.");
        }
    };

    Ok(debug_data)
}

