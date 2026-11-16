pub mod models;

use std::time::Duration;

use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::cli::CliArgs;
use models::{
    typesense_debug_model::TypesenseDebug, typesense_health_model::TypesenseHealth,
    typesense_metrics_model::TypesenseMetrics, typesense_stats_model::TypesenseStats,
};

/// Thin HTTP client for the Typesense monitoring endpoints.
pub struct TypesenseClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl TypesenseClient {
    pub fn new(args: &CliArgs) -> reqwest::Result<Self> {
        let mut builder = Client::builder();
        if let Ok(secs @ 1..) = u64::try_from(args.typesense_timeout) {
            builder = builder.timeout(Duration::from_secs(secs));
        }
        Ok(Self {
            http: builder.build()?,
            base_url: format!(
                "{}://{}:{}",
                args.typesense_protocol, args.typesense_host, args.typesense_port
            ),
            api_key: args.typesense_api_key.clone(),
        })
    }

    pub async fn metrics(&self) -> reqwest::Result<TypesenseMetrics> {
        self.get_json("/metrics.json").await
    }

    pub async fn stats(&self) -> reqwest::Result<TypesenseStats> {
        self.get_json("/stats.json").await
    }

    pub async fn health(&self) -> reqwest::Result<TypesenseHealth> {
        self.get_json("/health").await
    }

    pub async fn debug(&self) -> reqwest::Result<TypesenseDebug> {
        self.get_json("/debug").await
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> reqwest::Result<T> {
        self.http
            .get(format!("{}{}", self.base_url, path))
            .header("X-TYPESENSE-API-KEY", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
}
