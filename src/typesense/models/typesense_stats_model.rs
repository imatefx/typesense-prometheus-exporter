

use serde_json::Value;
use serde_json::Map;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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



impl Default for TypesenseStats {
    fn default() -> TypesenseStats {
        TypesenseStats {
            delete_latency_ms: 0.0,
delete_requests_per_second: 0.0,
import_latency_ms: 0.0,
import_requests_per_second: 0.0,
latency_ms: Map::new(),
overloaded_requests_per_second: 0.0,
pending_write_batches: 0.0,
requests_per_second: Map::new(),
search_latency_ms: 0.0,
search_requests_per_second: 0.0,
total_requests_per_second: 0.0,
write_latency_ms: 0.0,
write_requests_per_second: 0.0,
        }
    }
}