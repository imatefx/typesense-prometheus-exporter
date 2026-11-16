pub mod models;

use std::time::Instant;

use reqwest::Client;
use serde::de::DeserializeOwned;

use models::{
    typesense_collection_model::TypesenseCollection, typesense_debug_model::TypesenseDebug,
    typesense_health_model::TypesenseHealth, typesense_metrics_model::TypesenseMetrics,
    typesense_stats_model::TypesenseStats, typesense_status_model::TypesenseStatus,
};

/// An error with its causes, e.g. `error sending request for url (...):
/// connection refused`, which reqwest's own message leaves out.
pub fn describe(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

/// Thin HTTP client for the Typesense monitoring endpoints.
pub struct TypesenseClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl TypesenseClient {
    /// `http` is shared by every client so connections are pooled across
    /// targets. A client is made per scrape with the key resolved once, so a
    /// scrape's requests never mix an old and a new key during a rotation.
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

    /// The key as a header value marked sensitive, so it is never printed if
    /// requests are ever logged.
    fn api_key_header(&self) -> reqwest::header::HeaderValue {
        let mut value = reqwest::header::HeaderValue::from_str(&self.api_key)
            .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
        value.set_sensitive(true);
        value
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> reqwest::Result<T> {
        // host:port only: the base URL never carries credentials.
        let target = self
            .base_url
            .split_once("://")
            .map_or(self.base_url.as_str(), |(_, rest)| rest);
        let started = Instant::now();
        let sent = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("X-TYPESENSE-API-KEY", self.api_key_header())
            .send()
            .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let response = match sent {
            Ok(response) => response,
            Err(err) => {
                tracing::debug!(endpoint = %path, %target, elapsed_ms, error = %describe(&err), "typesense request failed");
                return Err(err);
            }
        };
        tracing::debug!(
            endpoint = %path,
            %target,
            status = response.status().as_u16(),
            elapsed_ms,
            "typesense request"
        );
        response.error_for_status()?.json().await
    }
}
