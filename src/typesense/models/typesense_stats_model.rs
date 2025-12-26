use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TypesenseStats {
    pub delete_latency_ms: f64,
    pub delete_requests_per_second: f64,
    pub import_latency_ms: f64,
    pub import_requests_per_second: f64,
    pub latency_ms: Map<String, Value>,
    pub overloaded_requests_per_second: f64,
    pub pending_write_batches: f64,
    pub requests_per_second: Map<String, Value>,
    pub search_latency_ms: f64,
    pub search_requests_per_second: f64,
    pub total_requests_per_second: f64,
    pub write_latency_ms: f64,
    pub write_requests_per_second: f64,
}
