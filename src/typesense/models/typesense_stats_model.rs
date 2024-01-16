

use serde_json::Value;
use serde_json::Map;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypesenseStats {
    pub delete_latency_ms: Option<f64>,
    pub delete_requests_per_second: Option<f64>,
    pub import_latency_ms: Option<f64>,
    pub import_requests_per_second: Option<f64>,
    pub latency_ms: Option<Map<String, Value>>,
    pub overloaded_requests_per_second: Option<f64>,
    pub pending_write_batches: Option<f64>,
    pub requests_per_second: Option<Map<String, Value>>,
    pub search_latency_ms: Option<f64>,
    pub search_requests_per_second: Option<f64>,
    pub total_requests_per_second: Option<f64>,
    pub write_latency_ms: Option<f64>,
    pub write_requests_per_second: Option<f64>,
}
