use serde::Deserialize;
use serde_json::{Map, Value};

/// Response of Typesense's `/stats.json` endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TypesenseStats {
    /// Only reported by newer Typesense versions.
    #[serde(default)]
    pub cache_hit_ratio: f64,
    pub delete_latency_ms: f64,
    pub delete_requests_per_second: f64,
    pub import_latency_ms: f64,
    pub import_requests_per_second: f64,
    /// Per-endpoint latency, keyed by e.g. `"GET /collections/books/documents/search"`.
    pub latency_ms: Map<String, Value>,
    pub overloaded_requests_per_second: f64,
    pub pending_write_batches: f64,
    /// Per-endpoint request rate, same keys as `latency_ms`.
    pub requests_per_second: Map<String, Value>,
    pub search_latency_ms: f64,
    pub search_requests_per_second: f64,
    pub total_requests_per_second: f64,
    pub write_latency_ms: f64,
    pub write_requests_per_second: f64,
}

impl TypesenseStats {
    /// `(metric key, value)` pairs for the scalar stats, in export order.
    pub fn entries(&self) -> [(&'static str, f64); 12] {
        [
            ("cache_hit_ratio", self.cache_hit_ratio),
            ("delete_latency_ms", self.delete_latency_ms),
            (
                "delete_requests_per_second",
                self.delete_requests_per_second,
            ),
            ("import_latency_ms", self.import_latency_ms),
            (
                "import_requests_per_second",
                self.import_requests_per_second,
            ),
            (
                "overloaded_requests_per_second",
                self.overloaded_requests_per_second,
            ),
            ("pending_write_batches", self.pending_write_batches),
            ("search_latency_ms", self.search_latency_ms),
            (
                "search_requests_per_second",
                self.search_requests_per_second,
            ),
            ("total_requests_per_second", self.total_requests_per_second),
            ("write_latency_ms", self.write_latency_ms),
            ("write_requests_per_second", self.write_requests_per_second),
        ]
    }
}
