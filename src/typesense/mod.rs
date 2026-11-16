pub mod models;

use reqwest::Client;
use serde::de::DeserializeOwned;

use models::{
    typesense_collection_model::TypesenseCollection, typesense_debug_model::TypesenseDebug,
    typesense_health_model::TypesenseHealth, typesense_metrics_model::TypesenseMetrics,
    typesense_stats_model::TypesenseStats, typesense_status_model::TypesenseStatus,
};

/// Thin HTTP client for the Typesense monitoring endpoints.
pub struct TypesenseClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl TypesenseClient {
    /// `http` is shared by every client so connections are pooled across targets.
    pub fn new(http: Client, base_url: String, api_key: String) -> Self {
        Self {
            http,
            base_url,
            api_key,
        }
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

    pub async fn status(&self) -> reqwest::Result<TypesenseStatus> {
        self.get_json("/status").await
    }

    /// Collection summaries without their schemas, which can be large and
    /// aren't needed for document counts.
    pub async fn collections(&self) -> reqwest::Result<Vec<TypesenseCollection>> {
        self.get_json("/collections?exclude_fields=fields").await
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
